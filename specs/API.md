# API.md

## Status

Draft specification.

This document defines the long-term Kairo daemon API contract used
by the CLI, web client, generated TypeScript API client,
integrations, and developer tools.

The Phase 2 §2 implementation ships a subset — see `DECISIONS.md`
§10 (read-only `/api/v1/...` endpoints, Unix socket only) and §11
(two-process architecture: the `kairo-daemon` process is the
trusted Unix-socket-only API; the `kairo-web` process — Phase 2 §5,
deferred — is the TCP / browser-facing surface that translates
authenticated requests into daemon calls). Endpoint groups in
sections §10–§22 are tagged **[v1]** for in-scope and **[post-v1]**
for deferred.

This specification is intentionally prescriptive enough to guide
implementation once each piece lands.

---

## 1. Purpose

The Kairo API exposes daemon capabilities over a local or controlled network
interface.

The API is responsible for exposing:

1. Daemon status and configuration.
2. Object and snapshot inspection.
3. Snapshot validation.
4. Fetch and sync operations.
5. Build, run, and reproduce operations.
6. Task creation, progress, logs, and results.
7. Store status and maintenance operations.
8. Federation search/status operations.
9. Policy checks, approvals, and denials.
10. Executor/runtime status.
11. Import/export operations.

The API is not responsible for:

1. Defining Kairo object semantics.
2. Defining statement interpretation.
3. Defining validation rules.
4. Executing without daemon policy approval.
5. Replacing core, store, federation, or executor specs.

---

## 2. Relationship to Other Specs

The API depends on:

- `DAEMON.md`
- `CORE_LIBRARY.md`
- `STORE.md`
- `FEDERATION.md`
- `EXECUTOR.md`
- `CLI.md`
- `WEB_CLIENT.md`
- `OBJECT.md`
- `STATEMENTS.md`
- `BUILD.md`
- `PLANNER.md`

The API must expose core-derived validation results faithfully.

The API must preserve distinctions between:

- Core validation status
- Daemon policy decision
- Operational task status
- API transport errors
- Runtime/executor errors
- Federation errors
- Store errors

---

## 3. API Contract Strategy

The long-term daemon API contract is described by OpenAPI. OpenAPI
is the canonical cross-language contract between:

```text
kairo-daemon (Rust, Unix socket)
  -> kairo-cli (Rust, daemon-mode dispatch via kairo-daemon-client)
  -> kairo-web  (Phase 2 §5; TCP, browser-facing; translates to daemon-client)
       -> TypeScript/React web client
       -> generated API clients
       -> integrations
```

**[v1] OpenAPI is deferred.** See `DECISIONS.md` §10.3 and §11.
The v1 daemon's only client is `kairo-cli`, both sides are Rust,
the contract is internal, and hand-coded handlers ship the same
working API as a generator would. OpenAPI lands when Phase 2 §5
web-client work crystallizes the external shape; the Rust generator
choice (utoipa / aide / okapi / schemars + custom) is locked then.

Recommended endpoints (long-term):

```text
GET /api/openapi.json    # post-v1
GET /api/v1/version      # v1
GET /api/v1/status       # v1
```

### 3.1 Rust OpenAPI generation

**[post-v1].** The Rust daemon will generate or maintain OpenAPI
using a Rust-compatible system such as:

- utoipa
- aide
- okapi
- schemars plus custom OpenAPI generation

The exact library is not mandated, but the generated schema must
be suitable for TypeScript client generation. Library choice is
deferred to Phase 2 §5 when the web-client toolchain is concrete.

### 3.2 TypeScript consumption

**[post-v1].** The TypeScript web client should consume the
OpenAPI schema using:

- `openapi-typescript`
- `openapi-fetch` or a thin custom fetch wrapper
- Zod validation for important response envelopes and errors

Hand-written TypeScript DTOs must not replace generated API types
when generated types are available.

---

## 4. API Style

The daemon API uses HTTP with JSON.

Base path:

```text
/api/v1
```

Content type:

```text
application/json
```

**[v1] transport: Unix domain socket only**, at
`<store>/daemon.sock` (mode 0600). HTTP framed over the socket;
no TCP, no TLS, no named pipes. The `kairo-daemon-client` Rust
crate (consumed by `kairo-cli`'s daemon-mode dispatch) speaks
HTTP-over-Unix-socket; future Rust web-server impls may use the
same crate.

**[post-v1]** Streaming-progress endpoints (long-running tasks,
build/run logs, federation sync events) will use one of:

- Server-Sent Events
- WebSocket
- newline-delimited JSON
- chunked HTTP streams

V1 has one streaming endpoint — `GET /api/v1/blobs/{id}` — which
streams response bodies as raw bytes (chunked transfer encoding).
No SSE / WebSocket / NDJSON in v1.

**[post-v1] additional transports.** TCP/TLS, named pipes, gRPC,
JSON-RPC, and embedded-library mode are all possible additions.
The TCP listener specifically lands with `kairo-web` (Phase 2 §5):
the web-server terminates TCP/TLS in its own process and forwards
to the daemon via Unix socket, preserving the daemon's
trusted-only inbound surface (see `DECISIONS.md` §11).

---

## 5. Versioning

The API version must be explicit.

Recommended versioning:

```text
/api/v1/...
```

API responses should include schema/version metadata where useful.

Breaking changes require a new major API version.

Non-breaking additions may include:

- New optional fields
- New endpoints
- New enum values, if clients are required to handle unknown values safely

Clients must treat unknown enum values as unsupported or unknown rather than
crashing.

---

## 6. Common DTOs

### 6.1 Identifier DTOs

Identifiers are serialized as strings.

Examples:

```json
{
  "object_id": "z6MkObject...",
  "snapshot_id": "z6MkSnapshot...",
  "statement_id": "z6MkStatement...",
  "actor_id": "z6MkActor...",
  "blob_id": "z6MkBlob..."
}
```

The exact encoding is defined by the identifier specification.

Typed ID fields contain bare ID payloads. Clients must not infer semantics from
payload spelling. Fields that carry standalone references, such as `ref`, use
the typed-reference grammar from `IDENTIFIERS.md`.

### 6.2 SnapshotRef

```json
{
  "object_id": "z6MkObject...",
  "frontier": [
    {
      "actor_id": "z6MkActor...",
      "statement_id": "z6MkStatement...",
      "actor_seq": 12
    }
  ]
}
```

### 6.3 SnapshotPurpose

Allowed values:

```text
inspect
build
run
reproduce
archive_mirror
```

Clients must handle unknown future values gracefully.

### 6.4 ValidationStatus

Allowed values:

```text
valid
invalid
conflicted
indeterminate
```

`unverified` may be used by API endpoints that return previews without core
validation.

### 6.5 PolicyDecision

Allowed values:

```text
allow
deny
require_user_approval
```

### 6.6 TaskStatus

Allowed values:

```text
queued
running
succeeded
failed
cancelled
interrupted
```

### 6.7 LocalityStatus

Allowed values:

```text
local
remote
cached
partial
missing
fetching
unknown
```

Locality is not validity.

---

## 7. Response Envelopes

All API responses should use stable response envelopes.

### 7.1 Success envelope

```json
{
  "ok": true,
  "schema": "kairo.api.result.v1",
  "result": {}
}
```

### 7.2 Error envelope

```json
{
  "ok": false,
  "schema": "kairo.api.error.v1",
  "error": {
    "code": "validation_indeterminate",
    "message": "Snapshot closure is incomplete.",
    "details": {}
  }
}
```

### 7.3 Task-or-result envelope

Operations may return either an immediate result or a task reference.

```json
{
  "ok": true,
  "schema": "kairo.api.task_or_result.v1",
  "result": {
    "kind": "task",
    "task": {
      "task_id": "task_..."
    }
  }
}
```

or:

```json
{
  "ok": true,
  "schema": "kairo.api.task_or_result.v1",
  "result": {
    "kind": "result",
    "value": {}
  }
}
```

---

## 8. Error Model

The API must distinguish error categories.

Recommended error codes:

```text
bad_request
unauthorized
forbidden
not_found
conflict
unsupported_feature
daemon_unavailable
store_error
federation_error
runtime_error
executor_error
policy_denied
validation_invalid
validation_conflicted
validation_indeterminate
decode_error
internal_error
```

Validation statuses should be represented as structured validation results when
possible, not only as transport errors.

HTTP status codes should be used consistently, but clients must use structured
error codes for program behavior.

Recommended mapping:

```text
400 bad_request
401 unauthorized
403 forbidden / policy_denied
404 not_found
409 conflict / validation_conflicted
422 validation_invalid / validation_indeterminate
500 internal_error
503 daemon_unavailable
```

---

## 9. Authentication and Local Security

The two-process architecture from `DECISIONS.md` §11 splits authn/
authz across distinct trust boundaries:

### 9.1 `kairo-daemon` (the trusted process)

**[v1]** The daemon listens **only** on a Unix domain socket at
`<store>/daemon.sock` (mode 0600). Authentication is **filesystem
permissions**: anyone who can `connect(2)` to the socket is fully
trusted, identical to a `kairo-cli` direct-mode caller. No bearer
tokens, no per-request auth, no rate limiting, no CORS — the
daemon never sees untrusted input by design.

This relies on the host OS enforcing socket file permissions
correctly. The daemon's parent directory (`<store>/`) is created
with the user's default umask; an operator who weakens permissions
on `<store>/` weakens the trust boundary.

The daemon does not expose a TCP listener in v1. There is no
`bind 127.0.0.1` shape — TCP exposure requires the `kairo-web`
process below.

### 9.2 `kairo-web` (the public-facing process, post-v1)

**[Phase 2 §5, deferred].** The web-server terminates TCP (and
TLS, eventually), serves the SPA bundle, and translates approved
requests into Unix-socket calls to the daemon via the
`kairo-daemon-client` crate. All untrusted-input handling
(authentication, CORS, rate limiting, request validation,
CSRF) lives here, not in the daemon.

Authentication modes for the web-server (subject to §5 design):

1. Bearer token.
2. Session cookie.
3. OS-integrated authentication.
4. (Default for local-first) Loopback-only origin check + cookie.

The web-server runs as the same user as the daemon by default;
privilege separation (different user, sandbox, container) is
possible but optional.

### 9.3 Mutation, execution, policy

**[post-v1]** The API protects mutation and execution endpoints
behind daemon policy approval (DAEMON.md §12). At minimum,
dangerous operations require daemon policy approval even if the
API request is authenticated. v1 has no mutation/execution
endpoints, so this section is informational until those land.

---

## 10. Endpoint Groups

Long-term endpoint groups, tagged by phase:

```text
/system          [v1] /version, /status only; /openapi.json post-v1
/actors          [v1] read-only
/objects         [v1] genesis read; /list and /summary post-v1
/statements      [v1] read-only
/branches        [v1] list + latest
/version-tags    [v1] read-only
/trust           [v1] read-only
/capabilities    [v1] read-only
/blobs           [v1] streaming read
/snapshots       [post-v1] depends on snapshot-resolution surface
/tasks           [post-v1] depends on TaskManager
/fetch /sync     [post-v1] depends on Phase 2 §4 federation
/builds /runs /reproduce
                 [post-v1] depends on Phase 2 §7 build/run
/import /export  [post-v1] depends on bundle work + TaskManager
/store           [post-v1] store maintenance / GC
/federation      [post-v1] depends on Phase 2 §4
/policy          [post-v1] depends on PolicyService
/executors       [post-v1] depends on Phase 2 §7
/config          [post-v1]
```

The `[v1]` groups together comprise the ~11 read-only endpoints
locked in `DECISIONS.md` §10.4. Sections §11–§22 below describe the
long-term endpoint shape; `[v1]` shipping behavior is summarized
inline where the v1 surface differs.

---

## 11. System Endpoints

**[v1] surface:** v1 ships flat `GET /api/v1/version` and
`GET /api/v1/status` (no `/system/` prefix). Status excludes
federation, runtime/executor, and task fields — those subsystems
do not exist in v1. The `/api/openapi.json` discovery endpoint is
post-v1.

### 11.1 Get API version

```http
GET /api/v1/system/version
```

Returns daemon version and API version.

Response result:

```json
{
  "daemon_version": "0.1.0",
  "api_version": "v1",
  "core_version": "0.1.0",
  "store_version": "0.1.0"
}
```

### 11.2 Get daemon status

```http
GET /api/v1/system/status
```

Returns:

- Daemon running status
- Store status
- Federation status
- Runtime/executor status
- Active task counts
- API bind info, where safe

In v1 this returns only daemon running status, store path/schema-
version, and PID. No federation/runtime/task fields.

---

## 12. Object Endpoints

**[v1] surface:** v1 ships only `GET /api/v1/objects/{object_id}`,
returning the object's genesis record (the same shape `kairo
object show` prints in direct mode). `/list`, `/summary`, and
`/inspect` are post-v1; they depend on indexing and snapshot-
resolution surface that v1 doesn't have. The `/objects/{id}/
statements` listing in §14.1 is also post-v1 — v1 surfaces
individual statements via `/statements/{id}` only.

### 12.1 List local objects

```http
GET /api/v1/objects
```

Query parameters:

```text
q
limit
cursor
locality
validation_status
```

Returns object summaries.

Search/list results may be unverified unless validation data is included.

### 12.2 Get object summary

```http
GET /api/v1/objects/{object_id}
```

Returns object metadata known to the daemon.

The response must indicate whether the returned state is:

- Verified
- Unverified
- Partial
- Derived from a specific snapshot

### 12.3 Inspect object

```http
POST /api/v1/objects/{object_id}/inspect
```

Request:

```json
{
  "snapshot": null,
  "purpose": "inspect",
  "fetch_missing": false
}
```

Returns an inspect result.

If `fetch_missing` is true, the daemon may create a fetch task.

---

## 13. Snapshot Endpoints

**[post-v1].** All snapshot endpoints depend on the snapshot-
resolution surface (frontier resolution, closure status,
validation runner) that lands with `CORE_LIBRARY.md` snapshot
work. v1 has no `/snapshots/...` route. Branch and version-tag
reads in v1 are accessed directly via `/branches/...` and
`/version-tags/...`, not via snapshot resolution.

### 13.1 Resolve snapshot

```http
POST /api/v1/snapshots/resolve
```

Request:

```json
{
  "object_ref": "object:z6MkObject...",
  "selector": {
    "kind": "latest"
  },
  "purpose": "inspect"
}
```

Selectors may include:

```text
latest
snapshot_id
frontier
tag
name
```

The daemon must report ambiguity rather than guessing.

### 13.2 Get snapshot

```http
GET /api/v1/snapshots/{snapshot_id}
```

Returns snapshot metadata, frontier, locality, and known closure status.

### 13.3 Validate snapshot

```http
POST /api/v1/snapshots/{snapshot_id}/validate
```

Request:

```json
{
  "purpose": "run",
  "fetch_missing": false
}
```

Returns:

```json
{
  "snapshot_id": "z6MkSnapshot...",
  "purpose": "run",
  "validation": {
    "status": "valid",
    "issues": []
  },
  "policy": null,
  "resolved_snapshot": {}
}
```

If validation requires remote data and `fetch_missing` is true, the daemon may
return a task reference.

### 13.4 Get snapshot closure status

```http
GET /api/v1/snapshots/{snapshot_id}/closure?purpose=run
```

Returns whether the daemon believes local closure data is complete, partial,
missing, or unknown.

This endpoint does not replace core validation.

---

## 14. Statement Endpoints

**[v1] surface:** v1 ships only `GET /api/v1/statements/{statement_id}`,
returning the statement record by ID. The per-object listing
(§14.1) and snapshot statement-graph (§14.3) are both post-v1
— they depend on indexing and snapshot resolution.

V1 also exposes statement-shaped reads through three dedicated
routes (each backed by FilesystemStore lookups, not the
statements index):

- `GET /api/v1/branches/{object_id}` — list ObjectBranch
  statements for the object
- `GET /api/v1/branches/{object_id}/{name}/latest` — latest
  branch head by `(created_at, statement_id)`
- `GET /api/v1/version-tags/{object_id}/{version}` —
  ObjectVersionTag statement for that version
- `GET /api/v1/trust/{by}/{of}` — ActorTrust statement
- `GET /api/v1/capabilities/{grantor}` — capability statements
  signed by the grantor

Each returns the underlying statement envelope; clients that
want the body decode it themselves.

### 14.1 List statements for object

```http
GET /api/v1/objects/{object_id}/statements
```

Query parameters:

```text
actor_id
limit
cursor
include_body
```

Returns statements known locally.

Statements returned by this endpoint are not necessarily valid or authoritative.

### 14.2 Get statement

```http
GET /api/v1/statements/{statement_id}
```

Returns statement record and provenance if known.

### 14.3 Get statement graph

```http
GET /api/v1/snapshots/{snapshot_id}/statement-graph
```

Returns graph data suitable for visualization.

The graph must indicate whether it is complete for the requested purpose.

---

## 15. Blob and Artifact Endpoints

**[v1] surface:** v1 ships a single streaming-bytes endpoint:

```http
GET /api/v1/blobs/{blob_id}
```

It streams the raw blob bytes with `Content-Type:
application/octet-stream` and chunked transfer encoding
(see §4). No metadata sub-route, no snapshot artifacts route,
no access-policy enforcement beyond the Unix-socket
filesystem-perms boundary.

### 15.1 Get blob metadata

```http
GET /api/v1/blobs/{blob_id}
```

Returns blob metadata, size, hash, availability, and media type where known.

### 15.2 Download blob

```http
GET /api/v1/blobs/{blob_id}/content
```

Returns blob content.

The daemon must enforce access policy.

### 15.3 List snapshot artifacts

```http
GET /api/v1/snapshots/{snapshot_id}/artifacts
```

Query parameters:

```text
purpose
```

Returns artifact records relevant to the snapshot/purpose.

---

## 16. Fetch Endpoints

**[post-v1].** Depends on Phase 2 §4 federation. v1 has no
`/fetch` routes. Object/blob fetch in v1 happens out-of-band
via `kairo git ...` (Phase 2 §1) or bundle import (CLI direct
mode); the daemon serves only what is already on disk.

### 16.1 Fetch object or snapshot

```http
POST /api/v1/fetch
```

Request:

```json
{
  "ref": "object:z6MkObject...",
  "purpose": "run",
  "with_blobs": true,
  "with_dependencies": true,
  "pin": false
}
```

Returns task or immediate result.

Fetch must not imply validation.

### 16.2 Get fetch preview

```http
POST /api/v1/fetch/preview
```

Returns what would be fetched, if known.

---

## 17. Sync Endpoints

**[post-v1].** Depends on Phase 2 §4 federation.

### 17.1 Sync object

```http
POST /api/v1/sync
```

Request:

```json
{
  "object_id": "z6MkObject...",
  "direction": "pull",
  "include_statements": true,
  "include_blobs": false,
  "include_closures": true,
  "dry_run": false
}
```

Allowed directions:

```text
pull
push
both
```

Sync must obey federation and publication policy.

---

## 18. Build Endpoints

**[post-v1].** Depends on Phase 2 §7 build/run + RuntimeService
+ TaskManager + PolicyService.

### 18.1 Plan build

```http
POST /api/v1/builds/plan
```

Request:

```json
{
  "snapshot_id": "z6MkSnapshot...",
  "target": null,
  "fetch_missing": false
}
```

Returns build plan if validation succeeds.

If validation is not valid, returns structured validation result.

### 18.2 Start build

```http
POST /api/v1/builds
```

Request:

```json
{
  "snapshot_id": "z6MkSnapshot...",
  "target": null,
  "executor_id": null,
  "policy_overrides": {}
}
```

Process:

1. Validate snapshot for build.
2. Produce build plan.
3. Check policy.
4. Create daemon task.
5. Dispatch executor if allowed.

Returns task reference or policy approval requirement.

---

## 19. Run Endpoints

**[post-v1].** Depends on Phase 2 §7 build/run + RuntimeService
+ TaskManager + PolicyService.

### 19.1 Plan run

```http
POST /api/v1/runs/plan
```

Request:

```json
{
  "snapshot_id": "z6MkSnapshot...",
  "entrypoint": null,
  "fetch_missing": false
}
```

Returns run plan if validation succeeds.

### 19.2 Start run

```http
POST /api/v1/runs
```

Request:

```json
{
  "snapshot_id": "z6MkSnapshot...",
  "entrypoint": null,
  "executor_id": null,
  "requested_capabilities": [],
  "policy_overrides": {}
}
```

Returns task/session reference or policy approval requirement.

### 19.3 Stop run

```http
POST /api/v1/runs/{run_id}/stop
```

Request:

```json
{
  "mode": "graceful"
}
```

Allowed modes:

```text
graceful
force
kill
```

---

## 20. Reproduce Endpoints

**[post-v1].** Depends on Phase 2 §7 build/run.

### 20.1 Plan reproduction

```http
POST /api/v1/reproduce/plan
```

Request:

```json
{
  "snapshot_id": "z6MkSnapshot...",
  "strict": true
}
```

Returns reproduction plan and reproducibility warnings.

### 20.2 Start reproduction

```http
POST /api/v1/reproduce
```

Request:

```json
{
  "snapshot_id": "z6MkSnapshot...",
  "strict": true,
  "executor_id": null
}
```

Returns task reference or policy approval requirement.

---

## 21. Task Endpoints

**[post-v1].** Depends on TaskManager (DAEMON.md §5.6),
deferred. v1 has no long-running operations and therefore no
task surface. Streaming endpoints (§21.5) also depend on the
SSE/WebSocket transport that v1 doesn't ship.

### 21.1 List tasks

```http
GET /api/v1/tasks
```

Query parameters:

```text
status
kind
limit
cursor
```

### 21.2 Get task

```http
GET /api/v1/tasks/{task_id}
```

Returns task status, progress, related object/snapshot, and result/error if
available.

### 21.3 Cancel task

```http
POST /api/v1/tasks/{task_id}/cancel
```

Requests cancellation.

### 21.4 Get task logs

```http
GET /api/v1/tasks/{task_id}/logs
```

Query parameters:

```text
cursor
limit
stream
```

### 21.5 Stream task events

```http
GET /api/v1/tasks/{task_id}/events
```

Recommended transport:

```text
text/event-stream
```

Events should include:

```text
task.updated
task.log
task.progress
task.completed
task.failed
task.cancelled
```

Task status is operational and must not be confused with validation status.

---

## 22. Import and Export Endpoints

**[post-v1].** Depends on bundle work + TaskManager. v1 import/
export happens via `kairo bundle ...` and `kairo git ...` in
direct CLI mode; the daemon does not expose import/export over
HTTP in v1.

### 22.1 Import

```http
POST /api/v1/import
```

May be multipart or JSON depending on source.

Request examples:

```json
{
  "source": {
    "kind": "local_path",
    "path": "/path/to/archive"
  },
  "pin": true,
  "verify": false
}
```

Returns task or immediate result.

Import success does not imply validation success.

### 22.2 Export

```http
POST /api/v1/export
```

Request:

```json
{
  "ref": "object:z6MkObject...:snapshot:z6MkSnapshot...",
  "mode": "snapshot_closure",
  "include_blobs": true,
  "include_dependencies": true
}
```

Returns task or downloadable artifact reference.

---

## 23. Store Endpoints

**[post-v1].** Store maintenance (verify, GC, rebuild-index)
depends on long-running task surface and GC design (DAEMON.md
§20). v1 surfaces store identity via `GET /api/v1/status` only.

### 23.1 Store status

```http
GET /api/v1/store/status
```

Returns store path, schema version, size estimates, index status, and GC status.

### 23.2 Verify store

```http
POST /api/v1/store/verify
```

Creates a verification task.

### 23.3 Garbage collect

```http
POST /api/v1/store/gc
```

Request:

```json
{
  "dry_run": true
}
```

GC must respect pins and active tasks.

### 23.4 Rebuild index

```http
POST /api/v1/store/rebuild-index
```

Creates index rebuild task.

---

## 24. Federation Endpoints

**[post-v1].** Depends on Phase 2 §4 federation +
FederationService (DAEMON.md §5.3).

### 24.1 Federation status

```http
GET /api/v1/federation/status
```

Returns enabled/disabled state, peer counts, sync state, and diagnostics.

### 24.2 List peers

```http
GET /api/v1/federation/peers
```

### 24.3 Search federation

```http
GET /api/v1/federation/search?q=...
```

Returns unverified search results unless validation is explicitly performed.

Search results must include verification/locality metadata.

### 24.4 Publish object

```http
POST /api/v1/federation/publish
```

Request:

```json
{
  "object_id": "z6MkObject...",
  "scope": "availability"
}
```

Publishing must obey policy.

### 24.5 Unpublish object

```http
POST /api/v1/federation/unpublish
```

---

## 25. Policy Endpoints

**[post-v1].** Depends on PolicyService (DAEMON.md §5.4). v1
has no mutation/execution endpoints, so no policy surface is
needed.

### 25.1 Get policy status

```http
GET /api/v1/policy
```

Returns policy configuration summary.

### 25.2 Check policy

```http
POST /api/v1/policy/check
```

Request:

```json
{
  "snapshot_id": "z6MkSnapshot...",
  "purpose": "run",
  "capabilities": []
}
```

Returns policy decision.

### 25.3 List approval requests

```http
GET /api/v1/policy/approvals
```

### 25.4 Approve request

```http
POST /api/v1/policy/approvals/{request_id}/approve
```

### 25.5 Deny request

```http
POST /api/v1/policy/approvals/{request_id}/deny
```

Policy decisions must remain distinct from core validation.

---

## 26. Executor Endpoints

**[post-v1].** Depends on Phase 2 §7 build/run + RuntimeService.

### 26.1 List executors

```http
GET /api/v1/executors
```

Returns executor descriptors and availability.

### 26.2 Check executor compatibility

```http
POST /api/v1/executors/compatibility
```

Request:

```json
{
  "snapshot_id": "z6MkSnapshot...",
  "purpose": "run",
  "executor_id": null
}
```

Returns compatible executors and rejection reasons.

### 26.3 Runtime sessions

```http
GET /api/v1/executors/sessions
GET /api/v1/executors/sessions/{session_id}
POST /api/v1/executors/sessions/{session_id}/stop
```

Runtime session data must not expose unsafe host details by default.

---

## 27. Config Endpoints

**[post-v1].** Daemon config in v1 is read at startup from a
fixed path (DAEMON.md §6.1) and not exposed over HTTP. Live
config introspection/mutation lands with ConfigService.

### 27.1 Get config summary

```http
GET /api/v1/config
```

Must redact secrets.

### 27.2 Validate config

```http
POST /api/v1/config/validate
```

### 27.3 Update config

```http
PATCH /api/v1/config
```

Optional.

Config mutation must be policy-protected and may require daemon restart.

---

## 28. DTO: ValidationResult

Validation result DTO:

```json
{
  "status": "valid",
  "issues": [],
  "resolved_snapshot": {}
}
```

Validation issue DTO:

```json
{
  "kind": "missing_authority_fact",
  "severity": "error",
  "message": "Missing capability grant for actor.",
  "statement_id": "z6MkStatement...",
  "actor_id": "z6MkActor...",
  "details": {}
}
```

Severity values:

```text
info
warning
error
```

Issue kind values should mirror `CORE_LIBRARY.md`.

---

## 29. DTO: Task

```json
{
  "task_id": "task_...",
  "kind": "build",
  "status": "running",
  "progress": {
    "current": 3,
    "total": 10,
    "message": "Building artifact"
  },
  "created_at": "2026-04-30T00:00:00Z",
  "updated_at": "2026-04-30T00:00:01Z",
  "related": {
    "object_id": "z6MkObject...",
    "snapshot_id": "z6MkSnapshot..."
  },
  "result": null,
  "error": null
}
```

Task timestamps are operational metadata.

---

## 30. DTO: PolicyDecision

```json
{
  "decision": "require_user_approval",
  "reason": "Run requests network access.",
  "requested_capabilities": [
    {
      "kind": "network",
      "mode": "outbound"
    }
  ],
  "approval_request_id": "approval_..."
}
```

---

## 31. DTO: Plan Responses

Build/run/reproduce plan responses must include:

1. Plan ID or hash.
2. Snapshot reference.
3. Purpose.
4. Required artifacts.
5. Required environment.
6. Requested capabilities.
7. Dependency snapshots.
8. Validation result.
9. Policy precheck result where available.

---

## 32. Streaming Events

**[post-v1].** No SSE/WebSocket/NDJSON event streams in v1. The
only v1 streaming is raw blob bytes via chunked transfer on
`GET /api/v1/blobs/{id}` (§4, §15).

Streaming event envelope:

```json
{
  "schema": "kairo.api.event.v1",
  "event": "task.progress",
  "task_id": "task_...",
  "seq": 42,
  "time": "2026-04-30T00:00:00Z",
  "data": {}
}
```

Event sequence numbers must be monotonically increasing per stream.

Clients should be able to reconnect and resume when supported.

---

## 33. Pagination

List endpoints should support cursor pagination.

Request:

```text
?limit=50&cursor=...
```

Response:

```json
{
  "items": [],
  "next_cursor": null
}
```

Offset pagination may be supported for diagnostics but cursor pagination is
preferred for large stores.

---

## 34. Idempotency

Mutation endpoints that create tasks should support idempotency keys.

Recommended header:

```text
Idempotency-Key: <client-generated-key>
```

The daemon should return the same task/result for repeated equivalent requests
with the same key.

---

## 35. Concurrency

The API must remain coherent under concurrent requests.

Requirements:

1. Snapshot validation receives stable closure inputs.
2. Store mutations are atomic from API perspective.
3. Task creation is idempotent where requested.
4. Task status updates are monotonic.
5. Cancel requests are safe to repeat.

---

## 36. Security Requirements

The long-term API must:

1. Bind locally by default.
2. Protect mutation endpoints.
3. Protect execution endpoints.
4. Never execute content during inspect/fetch/import/list/search.
5. Require daemon policy for build/run/reproduce execution.
6. Avoid leaking secrets in config/status responses.
7. Avoid exposing local filesystem paths unnecessarily.
8. Sanitize path-like inputs.
9. Treat remote/federated data as untrusted.
10. Preserve validation/policy/task distinctions.

**[v1] surface:** items 1, 4, 6, 7, 9, and 10 apply directly
(read-only daemon, Unix socket only, no execution paths). Items
2, 3, 5, and 8 are non-applicable in v1: there are no mutation,
execution, or path-input endpoints. The mutation/execution
shape lands with the relevant post-v1 services.

---

## 37. Implementation Checklist

The long-term checklist for a conforming API is:

1. OpenAPI generation or maintained schema.
2. `/system/version`.
3. `/system/status`.
4. Object list/get/inspect endpoints.
5. Snapshot resolve/get/validate endpoints.
6. Task list/get/events/logs/cancel endpoints.
7. Fetch endpoint.
8. Build plan/start endpoints.
9. Run plan/start/stop endpoints.
10. Store status endpoint.
11. Federation status/search endpoints.
12. Policy check/approval endpoints.
13. Executor list/compatibility endpoints.
14. Stable response envelopes.
15. Stable error envelopes.
16. Cursor pagination for lists.
17. Task/event streaming.
18. Authentication/local API protection.
19. TypeScript client generation test.
20. API integration tests.

### 37.1 v1 implementation checklist

The Phase 2 §2 daemon ships a narrower subset:

1. `GET /api/v1/version`.
2. `GET /api/v1/status`.
3. `GET /api/v1/actors/{id}`.
4. `GET /api/v1/objects/{id}`.
5. `GET /api/v1/statements/{id}`.
6. `GET /api/v1/branches/{object}`.
7. `GET /api/v1/branches/{object}/{name}/latest`.
8. `GET /api/v1/version-tags/{object}/{version}`.
9. `GET /api/v1/trust/{by}/{of}`.
10. `GET /api/v1/capabilities/{grantor}`.
11. `GET /api/v1/blobs/{id}` (chunked streaming).
12. Stable success/error envelopes (§7).
13. Unix socket transport at `<store>/daemon.sock` (mode 0600).
14. Filesystem-perms authn (§9.1) — no per-request auth.
15. API integration tests (handler-level + over-socket).

OpenAPI generation, TypeScript client, pagination, task/event
streaming, and any mutation/execution items from §37 are
post-v1.

---

End of `API.md`.
