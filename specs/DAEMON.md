# DAEMON.md

## Status

Draft specification.

This document defines the Kairo daemon: the long-running local node process that
coordinates the core library, local store, federation layer, runtime executors,
policy engine, and local APIs.

This specification is intentionally prescriptive enough to guide implementation.

---

## 1. Purpose

The Kairo daemon is the operational process for a local Kairo node.

The daemon is responsible for:

1. Managing the local object store.
2. Exposing local APIs for CLI and web clients.
3. Coordinating snapshot resolution and validation through the core library.
4. Coordinating build and run planning through the core library.
5. Invoking runtime/build executors after policy approval.
6. Coordinating federation synchronization through the federation library.
7. Managing local policy, trust roots, pins, caches, and user preferences.
8. Providing status, diagnostics, logs, and task progress.

The daemon is not responsible for:

1. Defining object semantics.
2. Defining statement interpretation.
3. Defining authority semantics.
4. Defining DHT/federation protocol internals.
5. Implementing UI behavior.
6. Replacing the core library’s validation model.

---

## 2. Dependency Relationship

The daemon depends on:

- `CORE_LIBRARY.md`
- `STORE.md`
- `FEDERATION.md`
- `OBJECT.md`
- `STATEMENTS.md`
- `BUILD.md`
- `PLANNER.md`

The following specs depend on the daemon:

- `CLI.md`
- `WEB_CLIENT.md`
- any local API specification

The daemon must use the core library as the canonical source for:

- Snapshot validation
- Effective object state
- Authority evaluation
- Build planning
- Run planning
- Validation status terminology

The daemon may enforce additional local policy, but it must not reinterpret core validity.

---

## 3. Architectural Role

Conceptually:

```text
CLI / Web Client
       |
       v
   Kairo Daemon
       |
       +-- Core Library
       +-- Local Store
       +-- Federation Library
       +-- Runtime Executors
       +-- Policy Engine
       +-- Local API Server
```

The daemon is a coordinator. It should delegate semantic work to core, persistence
to store, network discovery/sync to federation, and execution to runtime executors.

---

## 4. Design Principles

### 4.1 Core is authoritative

If core reports a snapshot as invalid, conflicted, or indeterminate, the daemon
must not present it as valid.

### 4.2 Policy is local

The daemon may reject operations that core considers valid.

Examples:

- User does not trust an actor.
- Runtime requests network access.
- Build requires an unsupported VM.
- Object is valid but not allowed by local policy.

### 4.3 Execution is explicit

The daemon must not execute object content merely because an object was fetched,
indexed, inspected, or validated.

Execution requires an explicit build/run request and policy approval.

### 4.4 Long-running state is daemon-owned

Background tasks, caches, federation sessions, runtime processes, API listeners,
and progress tracking belong to the daemon, not the core library.

### 4.5 Deterministic semantics, operational nondeterminism

The daemon may perform nondeterministic operational work such as scheduling tasks,
retrying network requests, or selecting peers. It must not allow those operational
details to affect core semantic results.

---

## 5. Major Components

A conforming daemon should contain these components:

```text
Daemon
  StoreManager
  CoreService
  FederationService
  PolicyService
  RuntimeService
  TaskManager
  ApiService
  ConfigService
  LogService
```

### 5.1 StoreManager

Owns access to the local store.

Responsibilities:

1. Open, initialize, migrate, and lock the local store.
2. Provide store-backed provider traits to core.
3. Ingest objects, statements, blobs, and snapshot closures.
4. Maintain pins and local retention metadata.
5. Coordinate garbage collection.
6. Maintain indexes or request index maintenance from the store.

### 5.2 CoreService

Thin daemon wrapper around the core library.

Responsibilities:

1. Construct core provider adapters.
2. Call core validation, resolution, build planning, and run planning APIs.
3. Translate core results into daemon API responses.
4. Preserve core validation statuses exactly.

The CoreService must not duplicate core validation logic.

### 5.3 FederationService

Coordinates the federation library.

Responsibilities:

1. Peer discovery.
2. Remote object lookup.
3. Remote snapshot closure retrieval.
4. Remote statement/blob synchronization.
5. Publishing locally available objects or statements according to policy.
6. Reporting federation status to the daemon.

Federation semantics are defined by `FEDERATION.md`, not by this document.

### 5.4 PolicyService

Applies local policy.

Responsibilities:

1. Maintain trusted actors and trust roots.
2. Maintain local execution permissions.
3. Decide whether builds/runs may proceed.
4. Decide whether federation may publish local data.
5. Decide whether remote data may be stored, pinned, or executed.
6. Present policy decisions in structured form.

Policy may reject otherwise valid snapshots.

### 5.5 RuntimeService

Coordinates runtime and build executors.

Responsibilities:

1. Register available executors.
2. Match core-produced plans to executors.
3. Request policy approval before execution.
4. Start, monitor, and stop executions.
5. Capture logs, outputs, and generated artifacts.
6. Report runtime status to clients.

The RuntimeService must not invent build or run plans. It executes plans produced
by core, possibly after daemon-level policy augmentation.

### 5.6 TaskManager

Tracks long-running operations.

Examples:

- Fetch object
- Sync object
- Validate snapshot
- Build snapshot
- Run snapshot
- Import archive
- Export archive
- Garbage collection
- Index rebuild

Responsibilities:

1. Assign task IDs.
2. Track status and progress.
3. Persist durable task state where appropriate.
4. Support cancellation where safe.
5. Expose logs and diagnostics.

### 5.7 ApiService

Exposes local APIs for CLI, web client, and integrations.

Possible transports:

- Local HTTP
- Unix domain socket
- Named pipe
- gRPC
- JSON-RPC
- Embedded library mode

The transport may vary, but exposed semantics must remain aligned with this spec.

### 5.8 ConfigService

Loads and validates daemon configuration.

Configuration includes:

- Store path
- API bind settings
- Federation settings
- Runtime executor settings
- Trust roots
- Policy defaults
- Cache limits
- Logging settings

---

## 6. Daemon Lifecycle

### 6.1 Startup

On startup, the daemon must:

1. Load configuration.
2. Open and lock the local store.
3. Check store schema version.
4. Run required migrations or fail safely.
5. Initialize provider adapters.
6. Initialize core service.
7. Initialize policy service.
8. Initialize runtime executor registry.
9. Initialize federation service if enabled.
10. Start local API service.
11. Start background task scheduler.

Startup must fail safely if the store cannot be opened or if required migrations fail.

### 6.2 Shutdown

On shutdown, the daemon must:

1. Stop accepting new API requests.
2. Cancel or drain background tasks according to policy.
3. Stop active runtime processes where appropriate.
4. Flush store writes.
5. Close federation sessions.
6. Release store locks.
7. Exit with a clear status.

### 6.3 Restart

Daemon restart must not corrupt store state.

In-progress tasks must be either:

- Resumable
- Marked failed/interrupted
- Reconstructed from durable state

---

## 7. Local Store Management

The daemon must treat the store as the durable local source for:

- Object records
- Statements
- Blobs
- Snapshot closure cache entries
- Pins
- Local metadata
- Indexes

The daemon may maintain additional volatile caches, but core-visible data must be
recoverable from durable store data or fetched again from federation.

### 7.1 Store locking

A store opened in write mode must be protected by a process lock or equivalent
mechanism to prevent unsafe concurrent writers.

Multiple readers may be allowed if the store implementation supports them.

### 7.2 Ingestion

When ingesting remote or local data, the daemon must:

1. Store raw data without assuming semantic validity.
2. Record provenance metadata where available.
3. Avoid executing imported data.
4. Optionally perform cheap structural/hash checks.
5. Leave semantic validation to core.

### 7.3 Pins

Pins prevent garbage collection.

The daemon should support pins for:

- Objects
- Snapshots
- Blobs
- Dependency closures
- User-created local workspaces

Pins may be explicit user pins or implicit operational pins.

---

## 8. Core Integration

The daemon must call core for the following operations:

### 8.1 Inspect

Input:

- Object reference or snapshot reference
- Requested inspect mode

Process:

1. Locate or construct snapshot closure.
2. Call core validation/resolution for `Inspect`.
3. Return resolved metadata and validation status.

### 8.2 Verify

Input:

- Snapshot reference
- Purpose

Process:

1. Locate or construct snapshot closure.
2. Call core validation for requested purpose.
3. Return full validation result.

### 8.3 Build

Input:

- Snapshot reference or object reference
- Build selection options

Process:

1. Resolve snapshot closure for `Build`.
2. Call core validation for `Build`.
3. If valid, call core build planning.
4. Apply daemon policy.
5. Dispatch plan to RuntimeService if execution requested.

### 8.4 Run

Input:

- Snapshot reference or object reference
- Runtime selection options

Process:

1. Resolve snapshot closure for `Run`.
2. Call core validation for `Run`.
3. If valid, call core run planning.
4. Apply daemon policy.
5. Dispatch plan to RuntimeService if execution requested.

### 8.5 Reproduce

Input:

- Snapshot reference
- Reproduction mode

Process:

1. Resolve snapshot closure for `Reproduce`.
2. Call core validation for `Reproduce`.
3. Call core build/run planning as required.
4. Apply stricter reproducibility policy.
5. Dispatch executor if approved.

---

## 9. Snapshot Closure Acquisition

When an operation requires a snapshot closure, the daemon should attempt acquisition
in this order:

1. Check local snapshot closure cache.
2. Attempt to construct closure from local store data.
3. Query federation for missing statements/blobs/dependencies if enabled.
4. Store fetched data.
5. Re-run closure construction.
6. Return `Indeterminate` if required data remains missing.

The daemon must not fabricate closure completeness.

A closure claim from federation must be passed to core for validation; the daemon
must not treat it as proof of validity by itself.

---

## 10. Federation Coordination

The daemon owns federation policy and scheduling, but not federation protocol semantics.

The daemon may perform:

- Background peer discovery
- Background object sync
- On-demand object fetch
- On-demand snapshot closure fetch
- Blob prefetch
- Statement frontier lookup
- Publishing of local availability

### 10.1 Federation enablement

Federation may be disabled.

When disabled:

- The daemon must operate in local-only mode.
- Missing remote data results in indeterminate validation or not-found responses.
- Local core/store operations must still work.

### 10.2 Remote data trust

Remote data is untrusted until validated by core.

The daemon may store untrusted remote data, but must track provenance and must
not execute it unless validation and policy allow execution.

---

## 11. Runtime and Execution

The daemon may execute builds and runs only through explicit runtime executors.

### 11.1 Executor interface

Executors should implement an interface similar to:

```rust
pub trait Executor {
    fn can_execute(&self, plan: &ExecutionPlan) -> bool;
    fn prepare(&self, plan: &ExecutionPlan) -> Result<PreparedExecution, ExecutorError>;
    fn start(&self, prepared: PreparedExecution) -> Result<ExecutionHandle, ExecutorError>;
    fn stop(&self, handle: &ExecutionHandle) -> Result<(), ExecutorError>;
}
```

Separate build executors may use similar interfaces.

### 11.2 Execution preconditions

Before execution, the daemon must confirm:

1. Core validation status is `Valid` for the required purpose.
2. No unresolved conflicts affect execution.
3. Required artifacts and blobs are available.
4. Required runtime executor exists.
5. Local policy approves requested runtime capabilities.
6. User approval is obtained when required.

### 11.3 Runtime capabilities

Runtime plans may request capabilities such as:

- Filesystem read
- Filesystem write
- Network access
- Display access
- Audio access
- Input devices
- Browser APIs
- Emulator devices
- Host integration
- GPU access

The daemon must treat these as explicit policy decisions.

### 11.4 Sandboxing

The daemon should prefer sandboxed execution.

The daemon must not grant host access merely because an object requests it.

---

## 12. Policy

Policy is local and may be stricter than core validation.

### 12.1 Policy inputs

Policy decisions may consider:

- Actor trust
- Object identity
- Snapshot identity
- Validation status
- Runtime capabilities
- Federation provenance
- User configuration
- Workspace context
- Host platform
- Prior approvals

### 12.2 Policy outputs

Policy must produce structured decisions:

```rust
pub enum PolicyDecision {
    Allow,
    Deny { reason: String },
    RequireUserApproval { reason: String },
}
```

Policy denial must not be represented as core invalidity.

### 12.3 Trust roots

The daemon manages local trust roots and trusted actors.

Core may verify cryptographic authority; daemon policy decides whether local users
trust those roots for local operation.

---

## 13. Local API

The daemon should expose a structured API.

Recommended resource groups:

```text
/objects
/snapshots
/statements
/blobs
/builds
/runs
/tasks
/store
/federation
/policy
/config
```

The exact transport is implementation-defined, but API responses must preserve
core validation status and policy status distinctly.

### 13.1 Required API concepts

API responses involving validation must include:

- Snapshot reference
- Requested purpose
- Core validation status
- Validation issues
- Policy decision, if applicable
- Whether data was local, remote, cached, or incomplete where known

### 13.2 Streaming/progress

Long-running operations should expose progress through:

- Pollable task records
- Event stream
- WebSocket
- Server-sent events
- CLI-followable logs

---

## 14. Task Model

Long-running daemon operations must be represented as tasks.

```rust
pub struct Task {
    pub id: TaskId,
    pub kind: TaskKind,
    pub status: TaskStatus,
    pub progress: Option<TaskProgress>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub result: Option<TaskResult>,
}
```

```rust
pub enum TaskStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}
```

Task status is operational. It must not be confused with core validation status.

---

## 15. Configuration

Daemon configuration should include:

```toml
[store]
path = "..."

[api]
enabled = true
bind = "127.0.0.1:..."

[federation]
enabled = true

[policy]
default_network_access = "deny"

[runtime]
enabled = true
```

Actual keys may differ, but configuration must support:

- Store path
- API settings
- Federation enablement
- Runtime enablement
- Policy defaults
- Logging level
- Cache limits

---

## 16. Logging and Diagnostics

The daemon must provide operational diagnostics.

Logs should distinguish:

- Core validation issues
- Policy denials
- Provider errors
- Store errors
- Federation errors
- Runtime errors
- API errors

Sensitive data must not be logged by default.

---

## 17. Error Model

The daemon must distinguish:

1. Core validation result
2. Policy denial
3. Store error
4. Federation error
5. Runtime error
6. API/client error
7. Internal daemon error

Recommended shape:

```rust
pub enum DaemonError {
    Core(CoreError),
    Store(StoreError),
    Federation(FederationError),
    Policy(PolicyError),
    Runtime(RuntimeError),
    Api(ApiError),
    Internal(String),
}
```

Validation issues should not be collapsed into generic daemon errors.

---

## 18. Security Requirements

The daemon must:

1. Treat all remote data as untrusted.
2. Never execute data during fetch, import, indexing, or validation.
3. Require explicit execution requests.
4. Require core validation before execution.
5. Require policy approval before execution.
6. Avoid exposing unsafe local APIs to untrusted clients.
7. Protect local API endpoints.
8. Avoid leaking private local object data through federation.
9. Respect publication policy.
10. Sanitize paths and artifact names.
11. Prefer sandboxed runtime execution.
12. Log security-relevant denials and failures.

---

## 19. Concurrency Requirements

The daemon may process multiple operations concurrently.

It must ensure:

1. Store writes are safe.
2. Index updates are consistent.
3. Runtime processes are tracked.
4. Task cancellation is safe.
5. API responses remain coherent.
6. Core validation receives stable snapshot closure inputs.

The daemon must not mutate a snapshot closure while core is validating it.

---

## 20. Garbage Collection

The daemon may schedule garbage collection.

GC must respect:

- User pins
- Active tasks
- Active runtimes
- Cached snapshot closures still in use
- Store retention policy
- Federation publication policy

GC must not remove data required by a pinned snapshot closure.

---

## 21. Import and Export

The daemon should support import/export workflows.

### 21.1 Import

Import must:

1. Ingest data into the store.
2. Preserve provenance where available.
3. Avoid execution.
4. Optionally validate after import.
5. Report validation status separately from import success.

### 21.2 Export

Export may include:

- Object record
- Statement subset
- Full statement log
- Snapshot closure
- Blobs
- Dependency snapshots
- Provenance metadata

Export format is defined elsewhere or by a future archive/package spec.

---

## 22. Required Daemon Operations

A conforming daemon should support at least:

1. Start daemon.
2. Stop daemon.
3. Inspect object/snapshot.
4. Verify snapshot.
5. Fetch object/snapshot.
6. Sync object.
7. Build snapshot.
8. Run snapshot.
9. List local objects.
10. List tasks.
11. Read task status/logs.
12. Pin/unpin object or snapshot.
13. Show policy decision.
14. Show federation status.
15. Show store status.

---

## 23. Implementation Checklist

A conforming initial daemon implementation should include:

1. Config loader.
2. Store opener and migrator.
3. Store lock.
4. Core service wrapper.
5. Snapshot closure acquisition flow.
6. Validation API endpoint.
7. Inspect API endpoint.
8. Build planning endpoint.
9. Run planning endpoint.
10. Task manager.
11. Runtime executor registry.
12. Policy service.
13. Federation service stub or integration.
14. Local API server.
15. Logging and diagnostics.
16. Graceful shutdown.
17. Basic garbage-collection/pinning support.

---

End of `DAEMON.md`.
