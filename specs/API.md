# API.md

## Status

Draft specification.

This document defines the Kairo daemon local API. The API is the contract used by
the CLI, web client, generated TypeScript API client, integrations, and developer
tools to communicate with a running Kairo daemon.

This specification is intentionally prescriptive enough to guide implementation.

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

The daemon API must be described by OpenAPI.

OpenAPI is the canonical cross-language contract between:

```text
Rust daemon
  -> CLI client
  -> TypeScript/React web client
  -> generated API clients
  -> integrations
```

The daemon should either:

1. Serve its OpenAPI document at runtime, or
2. Emit the OpenAPI document as a build artifact, or
3. Do both.

Recommended endpoints:

```text
GET /api/openapi.json
GET /api/version
GET /api/status
```

### 3.1 Rust OpenAPI generation

The Rust daemon should generate or maintain OpenAPI using a Rust-compatible system
such as:

- utoipa
- aide
- okapi
- schemars plus custom OpenAPI generation

The exact library is not mandated, but the generated schema must be suitable for
TypeScript client generation.

### 3.2 TypeScript consumption

The TypeScript web client should consume the OpenAPI schema using:

- `openapi-typescript`
- `openapi-fetch` or a thin custom fetch wrapper
- Zod validation for important response envelopes and errors

Hand-written TypeScript DTOs must not replace generated API types when generated
types are available.

---

## 4. API Style

The daemon API should use HTTP with JSON.

Recommended base path:

```text
/api/v1
```

Recommended content type:

```text
application/json
```

Streaming endpoints may use:

- Server-Sent Events
- WebSocket
- newline-delimited JSON
- chunked HTTP streams

The API may also be exposed through alternate local transports, such as Unix
domain sockets or named pipes, but the semantic contract should remain identical.

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

The daemon API may be exposed only locally by default.

Recommended default bind:

```text
127.0.0.1
```

The daemon must not expose an unauthenticated control API on public interfaces by
default.

Supported authentication modes may include:

1. Local trusted loopback only.
2. Bearer token.
3. Session cookie.
4. Unix socket permissions.
5. OS-integrated authentication.

The API must protect mutation and execution endpoints.

At minimum, dangerous operations must require daemon policy approval even if the
API request is authenticated.

---

## 10. Endpoint Groups

Recommended endpoint groups:

```text
/system
/objects
/snapshots
/statements
/blobs
/tasks
/fetch
/sync
/builds
/runs
/reproduce
/import
/export
/store
/federation
/policy
/executors
/config
```

---

## 11. System Endpoints

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

---

## 12. Object Endpoints

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

The API must:

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

---

## 37. Implementation Checklist

A conforming initial API implementation should provide:

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

---

End of `API.md`.
