# kairo-daemon

Long-running local service that fronts a Kairo store over an
HTTP+JSON API on a Unix domain socket. The daemon is the only
process that holds an open `FilesystemStore` for read access
served to multiple CLI clients; writes always go direct from
the CLI (see `specs/CLI.md` §3.3).

The crate ships:

- A `kairo-daemon` binary (`src/main.rs`) — foreground only;
  `kairo daemon start` invokes it with the user's `--store`.
- A library (`src/lib.rs`) exposing `serve(Config)` and the
  router used by tests and (post-v1) the web server.

## Phase

Phase 2 §2 (daemon) shipped through 8 slices. Phase 2 §5 (web
client) is starting; see `specs/PHASE_2_WEB_CLIENT.md`. Decisions:
`specs/DECISIONS.md` §10 (daemon MVP shape), §11 (two-process
architecture), §12 (web client v1 shape).

## Surface

11 read endpoints under `/api/v1/`:

```text
GET /api/v1/version
GET /api/v1/status
GET /api/v1/actors/{id}
GET /api/v1/objects/{id}
GET /api/v1/statements/{id}
GET /api/v1/branches/{object}
GET /api/v1/branches/{object}/{name}/latest
GET /api/v1/version-tags/{object}/{version}
GET /api/v1/trust/{by}/{of}
GET /api/v1/capabilities/{grantor}
GET /api/v1/blobs/{id}                              (streaming)
```

Plus the OpenAPI document itself:

```text
GET /api/v1/openapi.json
```

The daemon-client crate (`crates/kairo-daemon-client`) is the
Rust consumer; the web client (`frontend/`, Phase 2 §5) consumes
the OpenAPI document via `openapi-typescript`-generated types.

## OpenAPI contract

The schema lives in two synchronized places:

- **Source of truth:** `utoipa` annotations on handlers
  (`#[utoipa::path]`) and DTOs (`#[derive(ToSchema)]`), composed
  into `ApiDoc` in `src/api/openapi.rs`. The live schema is
  served at `GET /api/v1/openapi.json`.
- **Checked-in artifact:** `openapi/kairo-daemon.json` at the
  repo root. The web client and any external SDK generators read
  this file.

The two are kept in sync with two pieces of machinery:

```sh
# Regenerate the on-disk schema after a handler/DTO change.
cargo run -p kairo-daemon -- dump-openapi --out openapi/kairo-daemon.json
```

```sh
# Drift test: fails if the on-disk schema doesn't match the live one.
cargo test -p kairo-daemon --test openapi_drift
```

## Adding a route

The full agent-facing checklist is in
[`AGENTS.md`](./AGENTS.md). Short version for humans:

1. Add the handler under `src/api/handlers/<group>.rs`. It returns
   `Result<ApiResult<T>, ApiError>` and runs blocking store calls
   inside `tokio::task::spawn_blocking`.
2. Annotate it with `#[utoipa::path(...)]` — `path = "/api/v1/..."`,
   a unique `operation_id`, a `tag`, and a `responses(...)` list
   covering every status the handler can return.
3. Register the route in the axum router in `src/api/mod.rs`
   (`.route("/api/v1/...", get(handlers::<group>::handler))`).
4. Add the handler function to `paths(...)` in `ApiDoc` in
   `src/api/openapi.rs`. Add any new DTOs to
   `components(schemas(...))` in the same place.
5. Derive `ToSchema` on any new DTO. JSON wrappers around signed
   statements live upstream in `kairo-statement::json` and
   `kairo-identity::json`; lightweight summaries live in
   `kairo-daemon-client::dto`.
6. Add a corresponding method on `kairo_daemon_client::Client`.
7. Run `cargo run -p kairo-daemon -- dump-openapi --out
   openapi/kairo-daemon.json` and commit the result alongside the
   code change. The drift test will fail in CI if you forget.
8. Add an integration test under `tests/` that exercises hit,
   miss, and malformed-id paths against an in-process daemon.

## How utoipa fits together

utoipa is a derive-macro crate; there is no runtime scanning. The
schema is built once at the call site of `ApiDoc::openapi()`.
Three primitives:

- **`#[derive(ToSchema)]`** on a struct/enum generates a JSON
  Schema description of the type. Honors `serde` attributes
  (`rename`, `skip`, etc.). Nested fields must also implement
  `ToSchema`, transitively.
- **`#[utoipa::path(...)]`** on a handler function records the
  per-operation metadata (method, path, params, responses, tags,
  operation_id). The macro doesn't inspect the function signature
  — it stores what you wrote in a const at the function's site.
- **`#[derive(OpenApi)]`** on a marker struct (`ApiDoc`) is the
  bag: it lists which annotated handlers and which schemas to
  include in the document. Nothing is auto-discovered. Forgetting
  to list a handler silently drops it from the schema.

The axum router and the OpenAPI `paths(...)` list are wired
separately — the macro doesn't know about axum, so they can drift
if you only update one. The drift test catches schema-vs-disk
drift; route-vs-schema drift is on you and the test plan.

## Verification

```sh
cargo fmt --check
cargo clippy -p kairo-daemon -p kairo-daemon-client --all-targets -- -D warnings
cargo test -p kairo-daemon -p kairo-daemon-client
```

The full workspace check is the same surface as `AGENTS.md` at
the repo root — no daemon-specific extras.
