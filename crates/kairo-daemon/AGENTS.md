# AGENTS.md — kairo-daemon

## Adding a new HTTP route

Every read endpoint must land in three places: the axum router,
the `utoipa` OpenAPI doc, and the daemon-client. Skip any one and
the surface drifts (route works but isn't in the schema, schema
lists a route that 404s, schema has it but no Rust client method).

1. **Write the handler** under `src/api/handlers/<group>.rs`. Pattern:
   - Public `pub async fn handler(...)` returning
     `Result<ApiResult<T>, ApiError>` (or just `ApiResult<T>` if
     the handler can't fail).
   - Parse path/query strings into typed ids at the top via
     `id.parse().map_err(|e| ApiError::bad_request(...))?`.
   - Wrap blocking `FilesystemStore` calls in
     `tokio::task::spawn_blocking(move || store.foo(...))`. Map
     join errors with `ApiError::internal(...)`; map store errors
     with `map_store_error(error, "foo")`.
   - Produce the response body via the JSON DTO's
     `from_statement` / `from_body` constructor — never serialize
     the typed core value directly.

2. **Annotate with `#[utoipa::path(...)]`** immediately above the
   handler:
   - `path` uses `{name}` placeholders (OpenAPI form), not axum's
     `:name`. The two surfaces are wired separately and must agree.
   - `operation_id` must be unique across the whole document;
     follow the existing camelCase verb pattern (`getX`, `listX`,
     `getLatestX`).
   - `tag` matches one of the groups declared in
     `src/api/openapi.rs` (`system`, `actors`, `objects`, etc.).
     Adding a new tag requires adding it to the `tags(...)` list
     in `ApiDoc` too.
   - `body = T` describes the *inner* result type; the response-
     envelope wrapping is documented at the doc level. Use
     `body = Object` for a polymorphic / opaque payload.
   - List every status code the handler can produce: `200`, `400`,
     `404`, `500`. The error-envelope shape is implicit; just
     describe the code's meaning.

3. **Register the route in the router** in `src/api/mod.rs`:
   `.route("/api/v1/<path>", get(handlers::<group>::handler))`.
   Axum uses `:name` placeholders here.

4. **Add the path to the OpenAPI doc** in `src/api/openapi.rs`:
   list the handler function under `paths(...)` in the `ApiDoc`
   `#[openapi(...)]` annotation. utoipa won't pick it up
   automatically; the list is the source of truth.

5. **Register every new schema** under `components(schemas(...))`
   in the same `ApiDoc` annotation. If the response references a
   type that isn't yet in the doc, list it. Nested types referenced
   transitively still need to be in the list — `ToSchema` propagates
   at the type level, but `components.schemas.<name>` is the doc's
   index and only what's listed is rendered.

6. **Derive `ToSchema`** on any new DTO. Place rules:
   - Lightweight summary DTOs (one per endpoint group) live in
     `kairo-daemon-client::dto`.
   - JSON wrappers around signed statements live in
     `kairo-statement::json` and `kairo-identity::json`. Adding
     `ToSchema` upstream is correct for these — they are the wire
     contract.

7. **Add a client method** in `kairo-daemon-client::client`. Pattern:
   - Mirror the handler's path + parameters.
   - Deserialize through the envelope helper into `T`.
   - Surface client-side errors via `ClientError`.

8. **Regenerate the schema** and check it in:
   ```sh
   cargo run -p kairo-daemon -- dump-openapi --out openapi/kairo-daemon.json
   ```
   `cargo test -p kairo-daemon --test openapi_drift` is the
   safety net; it asserts the on-disk schema matches the live
   one and prints the regen command on failure.

9. **Write integration tests** in `tests/`:
   - Group with the existing test files
     (`by_id_handlers.rs`, `resolved_handlers.rs`,
     `blob_streaming.rs`) when the new endpoint fits one of those
     groups; otherwise add a new file.
   - Spin up an in-process daemon over a tempdir via the existing
     test harness, hit the endpoint through `kairo-daemon-client`,
     assert hit/miss/malformed-id behavior at minimum.
   - For shape-sensitive responses, also assert the JSON shape
     directly (not just the typed deserialization), so a future
     wire change is caught.

10. **Wire into `kairo-cli`** if a CLI command should route through
    the new endpoint in daemon mode. The probe-and-fall-back
    dispatch lives in `kairo-cli::dispatch`. Read commands branch
    on `Mode::Daemon` vs `Mode::Direct`; write commands stay
    direct (the v1 daemon is read-only).

## Envelope contract (do not break)

- Non-streaming responses are wrapped as
  `{ "ok": true,  "schema": "kairo.api.result.v1", "result": <T> }`
  on success and
  `{ "ok": false, "schema": "kairo.api.error.v1",  "error": {...} }`
  on failure. Handlers return `Result<ApiResult<T>, ApiError>`;
  the `IntoResponse` impl on each side does the wrapping. Don't
  serialize an envelope by hand.
- The blob endpoint (`GET /api/v1/blobs/{id}`) is the only
  exception: success is raw `application/octet-stream` bytes with
  no envelope. Errors still use the JSON envelope.
- Error codes are a closed `ApiErrorCode` enum in
  `src/api/envelope.rs`. Every new failure mode goes through one
  of those variants — extend the enum if no existing code fits,
  but match the wire-stable strings in `specs/API.md` §8.

## Concurrency and store access

- The daemon holds a single `Arc<FilesystemStore>` opened at
  startup. Every blocking store call must run in
  `tokio::task::spawn_blocking`; never call `store.get_*` directly
  from an async handler — it stalls the runtime.
- `kairo-store` is sync and HTTP-agnostic by design. Don't add
  HTTP-error mapping inside it — that's what
  `src/api/store_errors.rs` exists for.
- Concurrent CLI write traffic is the case the `§6` advisory locks
  cover; rely on the store's existing locking and don't add
  request-level locking in handlers.

## When in doubt

- The `utoipa` annotations are not the contract — the handler is.
  If a handler returns something the schema didn't promise, fix
  the schema, not the handler. The drift test catches mismatches
  on a clean run.
- The web client (Phase 2 §5) consumes the OpenAPI document.
  Treat the schema as a public surface: breaking changes belong
  in a v2 path prefix, not a v1 mutation.
- Async/sync split: `kairo-daemon` and `kairo-daemon-client` are
  the only async crates in the workspace. Don't reach for tokio
  primitives inside `kairo-store` or other sync crates; surface
  what you need through `spawn_blocking` here instead.
