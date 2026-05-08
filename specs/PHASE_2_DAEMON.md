# PHASE_2_DAEMON.md

## Status

Slice plan for the Phase 2 §2 daemon implementation. The shape
is already locked; this document decomposes the build into
reviewable commits.

References:

- `DECISIONS.md` §10 — the eight MVP sub-decisions.
- `DECISIONS.md` §11 — two-process architecture, axum+tokio.
- `DAEMON.md`, `API.md`, `CLI.md` — long-term contracts with
  v1 surface tagged inline.
- `PHASE_2.md` §2 — top-level bullet list (this spec is the
  decomposition of those bullets).

Out of scope (deferred to later phases): federation,
RuntimeService, PolicyService, TaskManager, GC, OpenAPI
generation, web server, mutation/execution endpoints,
TCP/TLS, sandboxing, structured-JSON logs, rolling log files.

---

## 1. Crate Layout

Two new crates land in this phase:

```text
crates/kairo-daemon         binary + lib (axum app)
crates/kairo-daemon-client  lib only (HTTP-over-Unix-socket)
```

`kairo-cli` gains a dependency on `kairo-daemon-client`.
`kairo-daemon` and `kairo-daemon-client` are the only async
crates in the workspace; everything else stays sync.

Async runtime: `tokio` (multi-threaded scheduler).
Framework: `axum` (handlers) + `tower-http` (middleware).
HTTP client: `hyper` directly (Unix socket connector). Avoid
pulling in `reqwest` for v1 — its TCP/TLS surface is wasted
weight.
Blocking store calls: `tokio::task::spawn_blocking`.

---

## 2. Cross-cutting Conventions

### 2.1 JSON envelope

All non-streaming responses use the `API.md` §7 envelopes:

```json
{ "ok": true,  "schema": "kairo.api.result.v1", "result": {...} }
{ "ok": false, "schema": "kairo.api.error.v1",  "error":  {...} }
```

Handler-level helpers wrap the envelope; handlers return a
typed `Result<T, ApiError>` and the helper serializes it.

### 2.2 Error mapping

`ApiError` carries an `API.md` §8 code plus an HTTP status:

```text
not_found              -> 404
bad_request            -> 400
store_error            -> 500
internal_error         -> 500
```

`kairo-store` errors map through a `From<StoreError>`
implementation in the daemon crate (not in `kairo-store`,
which stays sync and HTTP-agnostic).

### 2.3 Content negotiation

V1 endpoints serve `application/json` for everything except
`GET /api/v1/blobs/{id}`, which serves
`application/octet-stream` with chunked transfer encoding.
No `Accept` header parsing in v1 — clients get JSON or
octet-stream by route.

### 2.4 Logging

`tracing` subscriber with structured-text formatter:

```text
2026-05-07T12:34:56Z INFO  kairo_daemon::server bound socket=/path/daemon.sock
2026-05-07T12:34:57Z INFO  kairo_daemon::http   GET /api/v1/version 200 1.2ms
```

A `tower_http::trace` request layer logs method, path, status,
and duration at INFO; no request bodies, no headers. Errors
log at WARN with the error code.

### 2.5 Test harness

`kairo-daemon` tests use a helper that:

1. Builds a `FilesystemStore` over a `tempfile::TempDir`.
2. Spawns the daemon on an ephemeral socket path inside the
   temp dir.
3. Returns a `kairo-daemon-client::Client` pointing at it.

The same helper backs the daemon-side handler tests and the
`kairo-daemon-client` integration tests. CLI integration
tests call into this harness from `kairo-cli` test code via a
re-export from `kairo-test-support`.

---

## 3. Slice Sequence

### Slice 1 — Crate scaffolding

**Ships:**

- `crates/kairo-daemon` with `[[bin]]` + lib targets, deps on
  `tokio`, `axum`, `hyper`, `tower`, `tower-http`, `tracing`,
  `tracing-subscriber`, `serde`, `serde_json`, `thiserror`,
  `kairo-store`.
- `crates/kairo-daemon-client` with deps on `tokio`, `hyper`,
  `hyperlocal` (or hand-rolled Unix connector), `serde`,
  `serde_json`, `thiserror`.
- Workspace `Cargo.toml` updated; both crates listed.
- A do-nothing `kairo-daemon` binary that prints a banner and
  exits, plus a stub `Client` that compiles.
- One end-to-end smoke test that verifies the workspace builds.

**Exit criteria:** `cargo check --workspace` and `cargo test
--workspace` pass; both new crates appear in the workspace.

**Deferred:** any actual HTTP, sockets, handlers.

---

### Slice 2 — Daemon HTTP skeleton + Unix socket bind

**Ships:**

- `kairo-daemon::serve(config) -> Result<(), Error>`:
  1. Open `FilesystemStore` (sync, wrapped in `Arc`).
  2. Create the socket path; refuse to start if the socket
     exists and is live (probe: try to `connect`, if it
     succeeds another daemon is up — error out; if it fails,
     unlink and continue).
  3. Bind the Unix socket at `<store>/daemon.sock`, mode
     0600.
  4. Write `<store>/daemon.pid` (atomic via tempfile +
     rename).
  5. Build axum router with two handlers: `GET
     /api/v1/version`, `GET /api/v1/status`.
  6. Run the server.
  7. On `SIGTERM` / `SIGINT`: stop accepting, drain in-flight,
     close socket, unlink PID, exit 0.
- `tracing` subscriber installed at process start; structured-
  text formatter on stderr. `tower_http::trace::TraceLayer`
  on the router.
- Two endpoints implemented:
  - `GET /api/v1/version` → daemon, api, core, store versions.
  - `GET /api/v1/status` → daemon-running, store path, store
    schema version, PID. (No federation/runtime/task fields.)
- A binary entrypoint at `crates/kairo-daemon/src/main.rs`
  that takes `--store <path>` and calls `serve`.

**Exit criteria:**

- `kairo-daemon --store /tmp/store` binds the socket and
  responds to `curl --unix-socket /tmp/store/daemon.sock
  http://localhost/api/v1/version`.
- Sending SIGTERM cleans up PID file and socket.
- Tests cover: bind succeeds, double-start refuses, SIGTERM
  drains, version/status return well-formed envelopes.

**Deferred:** any read endpoints beyond version/status; the
CLI lifecycle verbs (still in slice 4).

---

### Slice 3 — `kairo-daemon-client` basics

**Ships:**

- `Client::new(socket_path)` — constructs a client bound to a
  Unix socket path. Internally a hyper client with a Unix
  connector and a small connection pool.
- `Client::probe(timeout) -> bool` — tries `GET /api/v1/status`
  with a short timeout; returns `true` iff the daemon answers
  with 2xx. Used by the CLI's probe-and-fall-back dispatch.
- `Client::version() -> Result<VersionInfo, ClientError>`.
- `Client::status() -> Result<StatusInfo, ClientError>`.
- DTO types under `kairo-daemon-client::dto` shared between
  daemon (server-side) and client (deserializer). Both crates
  import the DTOs from `kairo-daemon-client` to keep a single
  source of truth — server adds a tiny adapter layer if
  needed.
- `ClientError` enum: `Connect`, `Timeout`, `Http(status,
  code)`, `Decode`, `Transport`. Maps API error codes to a
  typed enum for callers.

**Exit criteria:**

- Tests in `kairo-daemon-client` spin up an in-process daemon
  via the test harness, hit it through the client, assert
  version/status round-trip.
- `Client::probe` returns `false` for a missing socket and a
  hung socket within the timeout.

**Deferred:** any endpoint beyond version/status; streaming
methods (slice 7).

---

### Slice 4 — CLI lifecycle commands

**Ships:**

- `kairo daemon start [--store <path>]` — runs the daemon
  in the foreground (calls `kairo_daemon::serve`). Forwards
  `--store` and the user's umask.
- `kairo daemon status [--store <path>]` — probes the
  socket via `daemon_client::probe`. If reachable, calls
  `Client::status` and prints PID, store path, version,
  schema version. Otherwise prints "not running" and exits
  9 (`daemon_unavailable`) only when `--daemon` is set;
  otherwise exits 0 with not-running output.
- `kairo daemon stop [--store <path>] [--wait]` — reads
  `<store>/daemon.pid`, sends SIGTERM. With `--wait`, polls
  the socket until the daemon stops or the timeout
  expires (default 10s). Errors on missing PID file.
- Help output and exit codes per `CLI.md` §7 / §10.

**Exit criteria:**

- End-to-end CLI test: `kairo daemon start &; kairo daemon
  status; kairo daemon stop --wait` round-trips against a
  fresh tempdir store and exits cleanly.
- `kairo daemon status` without a running daemon returns
  not-running output and the right exit code.
- Tests for the `--store` flag wiring and PID file races
  (e.g., stale PID file with no live process).

**Deferred:** `daemon restart`, `daemon logs` — both post-v1
per `CLI.md` §10.

---

### Slice 5 — By-id read handlers

**Ships:**

Server-side handlers (each wraps a single `FilesystemStore`
read inside `spawn_blocking`):

- `GET /api/v1/actors/{actor_id}` — returns the actor genesis
  envelope.
- `GET /api/v1/objects/{object_id}` — returns the object
  genesis envelope.
- `GET /api/v1/statements/{statement_id}` — returns the
  statement envelope by id (works across statement types).

Client-side methods on `daemon_client::Client` matching each
handler. DTOs reuse the canonical envelope shape from
`kairo-canonical` / `kairo-store` (re-exported through
`kairo-daemon-client::dto` so the client compiles
independently of `kairo-store`).

`not_found` mapping: `StoreError::NotFound` → 404 with
`{"code": "not_found"}`.

**Exit criteria:**

- Handler tests for hit, miss, and malformed-id paths.
- Round-trip integration tests (server ←→ client) for each
  of the three endpoints.
- Existing direct-mode CLI commands remain unchanged.

**Deferred:** indexing-backed listing endpoints; CLI dispatch
(slice 8).

---

### Slice 6 — Branch / tag / trust / capability handlers

**Ships:**

- `GET /api/v1/branches/{object_id}` — list of `ObjectBranch`
  statements for the object (one per `(actor, name)` head).
  Backed by `FilesystemStore::list_branches_for_object` (or
  the existing per-object scan).
- `GET /api/v1/branches/{object_id}/{name}/latest` — the
  conventional or named branch's latest head (already a
  store primitive).
- `GET /api/v1/version-tags/{object_id}/{version}` — calls
  `FilesystemStore::latest_version_tag` (which already honors
  cross-actor `supersedes` via the capability evaluator).
- `GET /api/v1/trust/{by_actor}/{of_actor}` — current
  first-person `ActorTrust` opinion (head only — history is
  post-v1).
- `GET /api/v1/capabilities/{grantor_id}` — the grantor's
  capability heads (audit query — one head per
  `(grantee, scope)` triple, mirroring `kairo capability
  list --grantor`).
- Matching client methods + DTOs.

**Exit criteria:**

- Handler tests cover hit/miss + the cross-actor flip
  (capability-honored tag resolution returns the successor's
  envelope, not the predecessor).
- Round-trip integration tests for each endpoint.

**Deferred:** trust history, branch / tag history, listing
across all objects.

---

### Slice 7 — Streaming blob handler

**Ships:**

- `GET /api/v1/blobs/{blob_id}` — opens the blob via
  `FilesystemStore::read_blob` (the existing streaming reader)
  and pipes the bytes into the response body. `Content-Type:
  application/octet-stream`. Chunked transfer (axum default
  for `Body::from_stream`).
- The blob reader runs on a `spawn_blocking` task that drives
  a `tokio::sync::mpsc` channel; the response body wraps the
  receiver. No buffering of the full blob in memory.
- Client method `Client::blob(blob_id) -> impl AsyncRead`
  (or returns a `hyper::Body` directly — pick whichever keeps
  the call site simple in `kairo-cli`).

**Exit criteria:**

- Tests with multi-MB blobs verify that the daemon's
  resident memory does not grow with blob size (a sampling
  test, not a strict cap).
- `not_found` mapping for missing blob ids.
- Round-trip stream test: write blob through the store, read
  it back through the client, assert byte equality.

**Deferred:** blob metadata route, `Range` headers, content-
type sniffing, access policy.

---

### Slice 8 — CLI dispatch wiring + integration polish

**Ships:**

- Global flag parsing in `kairo-cli`: `--daemon`,
  `--direct`, `--offline`. Default: probe-and-fall-back.
- A small `cli::dispatch` module that, for read commands,
  builds a `daemon_client::Client`, probes the socket, and
  either:
  - returns `Mode::Daemon(Client)` if reachable,
  - returns `Mode::Direct(Store)` otherwise.
  Write commands ignore the daemon entirely (`Mode::Direct`
  always).
- Wire at least three read commands through the dispatch:
  `kairo object show`, `kairo branch show`, and `kairo trust
  show`. Each calls the daemon path when in `Mode::Daemon`,
  the existing direct path otherwise. Output is identical
  in both modes (same DTO shape).
- Concurrent integration test: spawn a daemon over a tempdir
  store, run a CLI write command (`kairo branch set`) in
  direct mode while a CLI read command (`kairo branch show`)
  hits the daemon — verify both succeed (advisory locks
  from §6 already cover the race; this test is the proof).
- PHASE_2.md §2 close-out: tick the remaining bullets, add a
  short "what shipped" summary referencing this spec.

**Exit criteria:**

- `kairo branch show <object>` works in both daemon and
  direct mode against the same store.
- `--daemon` against a missing socket returns exit 9 with a
  helpful message.
- `--direct` ignores the socket entirely.
- Concurrent test passes.
- All PHASE_2 §2 bullets are checked.

**Deferred:** wiring the rest of the read commands through
the dispatch (each is a one-line follow-up once the pattern
is in place — track as polish, not a §2 blocker); SSE/event
streaming; the `kairo web` verb tree.

---

## 4. Deliberate Gaps

The following are explicitly out of v1 and have no slice in
this plan; each is its own decision in a later phase:

- **Auth.** Filesystem perms only. No tokens, no per-request
  auth, no rate limiting.
- **TCP / TLS.** Lands with `kairo-web` (Phase 2 §5).
- **OpenAPI.** Hand-coded handlers; generator choice deferred
  to Phase 2 §5 (see `DECISIONS.md` §10.3).
- **Mutation / execution.** Writes stay direct in v1; the
  daemon is read-only.
- **Tasks / streaming events.** No SSE, no WebSocket, no
  NDJSON. Single streaming endpoint is `/blobs/{id}` chunked
  transfer.
- **Federation, runtime, policy, GC.** Each gates on its own
  Phase 2 sub-section.

---

## 5. Risk Notes

- **Async/sync split.** Two crates are async; the rest of
  the workspace stays sync. The split is contained behind
  `spawn_blocking`. If a third crate ever needs async, that
  is the moment to revisit.
- **Socket path collisions.** Slice 2's "is another daemon
  already up?" probe is the safety net; rely on filesystem
  perms and the PID file for the rest.
- **DTO ownership.** DTOs live in `kairo-daemon-client` so
  the client crate is self-contained. The daemon imports
  them. Avoid a third "shared types" crate — the client is
  small enough to be the source of truth.
- **Test flakiness around socket lifecycle.** Tests must use
  per-test tempdir sockets (no shared paths) and must always
  clean up the daemon task on drop. The test harness in §2.5
  exists to make this a single helper, not per-test
  boilerplate.

---

End of `PHASE_2_DAEMON.md`.
