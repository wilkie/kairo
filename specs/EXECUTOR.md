# EXECUTOR.md

## Status

Draft specification.

This document defines the Kairo executor system: the daemon-facing implementation
layer that prepares, runs, monitors, and tears down build and run plans produced by
the core library.

This specification is intentionally prescriptive enough to guide implementation.

---

## 1. Purpose

Executors are responsible for turning validated Kairo plans into real execution.

Executors consume plans produced by the core library and coordinated by the daemon.

Executors are responsible for:

1. Matching supported plans and environment descriptors.
2. Preparing runtime/build environments.
3. Enforcing or participating in sandbox policy.
4. Mounting or materializing required artifacts.
5. Running build/run/reproduce steps.
6. Streaming logs, status, and progress.
7. Capturing outputs and generated artifacts.
8. Reporting runtime errors and exit status.
9. Cleaning up temporary resources.
10. Producing execution records for daemon/store ingestion.

Executors are not responsible for:

1. Defining object semantics.
2. Validating snapshots.
3. Evaluating actor authority.
4. Choosing the correct build/run plan.
5. Overriding daemon policy.
6. Publishing to federation.
7. Mutating object history directly.
8. Treating unvalidated content as trusted.

---

## 2. Relationship to Other Specs

Executors depend on:

- `CORE_LIBRARY.md`
- `DAEMON.md`
- `STORE.md`
- `BUILD.md`
- `PLANNER.md`
- `OBJECT.md`

The daemon depends on executors for side-effectful build and run operations.

The core library produces plans. Executors consume plans.

```text
Validated Snapshot
      |
      v
Core Planner
      |
      v
BuildPlan / RunPlan / ExecutionPlan
      |
      v
Daemon Policy + Task Manager
      |
      v
Executor
      |
      v
Runtime Process / VM / Browser / Emulator / Container
```

Executors must not reinterpret core validation state.

---

## 3. Design Principles

### 3.1 Plans are authoritative input

Executors run plans. They do not invent plans.

An executor may reject a plan it cannot safely support, but it must not silently
change the semantic meaning of the plan.

### 3.2 Execution is explicit

Executors must only run after the daemon explicitly requests execution.

Fetching, importing, inspecting, indexing, validating, or planning must not execute
object content.

### 3.3 Sandboxing by default

Executors should prefer restricted execution environments.

Host access must be explicit and policy-approved.

### 3.4 Capability-driven execution

Runtime privileges must be described as capabilities.

Executors must receive daemon-approved capability decisions before execution.

### 3.5 Reproducibility is recorded

Executors must record enough environment, input, output, and runtime metadata to
support later inspection and reproduction attempts.

### 3.6 Cleanup is mandatory

Executors must clean up temporary resources unless configured to preserve them for
debugging, reproducibility, or user inspection.

---

## 4. Executor Categories

Kairo may support multiple executor categories.

### 4.1 Build executors

Build executors run build plans.

Examples:

- Container build executor
- Nix build executor
- Local process build executor
- VM build executor
- Language-specific build executor

### 4.2 Run executors

Run executors run interactive or non-interactive artifacts.

Examples:

- Browser/web app executor
- DOSBox executor
- QEMU/VM executor
- Native process executor
- Notebook executor
- Data viewer executor
- Game/emulator executor

### 4.3 Reproduction executors

Reproduction executors run stricter build/run workflows intended to verify output
equivalence or reproduce declared results.

A reproduction executor may internally use build and run executors.

---

## 5. Core Concepts

### 5.1 ExecutionPlan

An execution plan is produced by the core library.

It describes what should happen, not how a specific local machine implements it.

A plan may include:

- Snapshot reference
- Purpose
- Environment descriptor
- Entrypoint
- Steps
- Required artifacts
- Required blobs
- Dependency snapshots
- Expected outputs
- Runtime capabilities
- Reproducibility constraints

### 5.2 Executor

An executor is a daemon-registered implementation that can prepare and run some
class of plans.

### 5.3 PreparedExecution

A prepared execution is a concrete local realization of a plan.

Examples:

- Container created but not started
- VM image assembled
- DOS disk image prepared
- Browser sandbox session created
- Temporary working directory materialized

### 5.4 ExecutionSession

An active or completed execution instance.

### 5.5 ExecutionRecord

A durable record of what happened.

Execution records are suitable for store ingestion and later inspection.

---

## 6. Executor Registry

The daemon should maintain an executor registry.

```rust
pub struct ExecutorRegistry {
    executors: Vec<Box<dyn Executor>>,
}
```

The registry is responsible for:

1. Registering available executors.
2. Listing executor capabilities.
3. Matching plans to compatible executors.
4. Reporting unsupported plans.
5. Providing deterministic selection where possible.

### 6.1 Executor matching

Executor selection must consider:

1. Plan purpose.
2. Environment descriptor.
3. Required runtime capabilities.
4. Host platform.
5. Executor availability.
6. Policy constraints.
7. User preference or configuration.

If multiple executors can run a plan, selection must be deterministic unless the
user or daemon policy explicitly selects one.

The daemon must expose selected executor identity in task/status output.

---

## 7. Executor Interface

Recommended Rust-facing trait:

```rust
pub trait Executor: Send + Sync {
    fn id(&self) -> ExecutorId;

    fn describe(&self) -> ExecutorDescriptor;

    fn can_execute(&self, plan: &ExecutionPlan) -> ExecutorCompatibility;

    fn prepare(
        &self,
        request: PrepareRequest,
    ) -> Result<PreparedExecution, ExecutorError>;

    fn start(
        &self,
        prepared: PreparedExecution,
    ) -> Result<ExecutionHandle, ExecutorError>;

    fn stop(
        &self,
        handle: &ExecutionHandle,
        mode: StopMode,
    ) -> Result<(), ExecutorError>;

    fn inspect(
        &self,
        handle: &ExecutionHandle,
    ) -> Result<ExecutionStatus, ExecutorError>;
}
```

### 7.1 ExecutorDescriptor

```rust
pub struct ExecutorDescriptor {
    pub id: ExecutorId,
    pub name: String,
    pub version: String,
    pub supported_purposes: Vec<ExecutionPurpose>,
    pub supported_environments: Vec<EnvironmentMatcher>,
    pub supported_capabilities: Vec<RuntimeCapability>,
    pub host_requirements: Vec<HostRequirement>,
}
```

### 7.2 ExecutorCompatibility

```rust
pub enum ExecutorCompatibility {
    Compatible,
    CompatibleWithWarnings(Vec<String>),
    Incompatible { reasons: Vec<String> },
}
```

Compatibility checks must not execute object content.

### 7.3 PrepareRequest

```rust
pub struct PrepareRequest {
    pub plan: ExecutionPlan,
    pub artifacts: Vec<ResolvedArtifact>,
    pub blobs: Vec<ResolvedBlob>,
    pub policy: ApprovedExecutionPolicy,
    pub working_directory: WorkingDirectory,
    pub task_context: TaskContext,
}
```

`ApprovedExecutionPolicy` must be produced by the daemon policy service. Executors
must not self-approve capabilities.

### 7.4 StopMode

```rust
pub enum StopMode {
    Graceful,
    Force,
    Kill,
}
```

---

## 8. Execution Lifecycle

Execution follows this lifecycle:

```text
planned
  -> policy-approved
  -> matched
  -> prepared
  -> started
  -> running
  -> completed / failed / cancelled
  -> outputs captured
  -> cleaned up
  -> recorded
```

### 8.1 Planned

Core produces the plan.

### 8.2 Policy-approved

The daemon evaluates policy and user approval.

No executor may start before policy approval.

### 8.3 Matched

The daemon selects a compatible executor.

### 8.4 Prepared

The executor creates local resources needed to run the plan.

Preparation may include:

- Creating temporary directories
- Materializing blobs
- Creating disk images
- Preparing containers
- Creating VM overlays
- Installing declared environment components
- Preparing browser sandbox sessions

Preparation must not run object entrypoints unless the plan explicitly defines a
preparation step and policy approves it.

### 8.5 Started

The executor starts the runtime process/session.

### 8.6 Running

The executor streams logs, status, progress, and interactive IO.

### 8.7 Completed/failed/cancelled

The executor records terminal status.

### 8.8 Outputs captured

The executor captures declared outputs and optionally discovered outputs.

### 8.9 Cleaned up

Temporary resources are removed unless preservation is requested.

### 8.10 Recorded

The daemon records execution metadata and may ingest outputs into the store.

---

## 9. Capability Model

Runtime capabilities describe privileges requested by a plan.

Examples:

```rust
pub enum RuntimeCapability {
    Display,
    Audio,
    KeyboardInput,
    PointerInput,
    GamepadInput,
    FilesystemRead { scope: FilesystemScope },
    FilesystemWrite { scope: FilesystemScope },
    Network { mode: NetworkMode },
    Gpu,
    Camera,
    Microphone,
    ClipboardRead,
    ClipboardWrite,
    HostProcess,
    HostDevice { device: String },
}
```

### 9.1 Capability enforcement

Executors must enforce daemon-approved capabilities to the extent supported by
the host and runtime.

If an executor cannot enforce a requested denial, it must report this before
execution and must not run unless policy explicitly allows the weaker isolation.

### 9.2 Capability escalation

Executors must not grant additional capabilities at runtime without daemon policy
approval.

If a runtime requests additional capability, execution must pause, fail, or ask
the daemon for a new policy decision.

### 9.3 Capability reporting

Execution status must expose granted capabilities and denied capabilities.

---

## 10. Artifact Materialization

Executors receive resolved artifacts and blobs from the daemon/store.

Executors must verify or rely on daemon-verified hashes before materialization.

Artifact materialization may include:

- Copying files
- Creating read-only mounts
- Creating writable overlays
- Creating disk images
- Creating virtual media
- Creating HTTP-served static directories
- Creating browser-accessible blob URLs

### 10.1 Read-only by default

Input artifacts should be mounted read-only unless the plan requires mutation.

### 10.2 Writable outputs

Writable locations must be explicitly declared.

Executors should isolate writable output paths from input artifact paths.

### 10.3 Path safety

Executors must sanitize artifact paths.

Artifact paths must not escape assigned working directories or mount roots.

---

## 11. Environment Preparation

Executors implement concrete environment preparation.

Environment descriptors come from core plans.

Examples:

- `container:image`
- `vm:image`
- `dosbox:profile`
- `browser:static-web`
- `node:version`
- `python:notebook`
- `emulator:system`

Executors may reject unsupported environments.

Executors must report environment identity and versions in execution records.

### 11.1 Environment immutability

For reproducible workflows, environment inputs should be immutable or content-addressed.

If an environment is mutable, remote, or host-dependent, the executor must record
that fact.

### 11.2 First-class web environments

Browser/web-object execution should be handled as an executor category.

A web executor may prepare:

- Static asset server
- Browser iframe sandbox
- Service worker isolation
- Origin isolation
- API bridge to daemon-approved resources

The browser executor must not grant arbitrary host access.

---

## 12. Logging and Output Streams

Executors must expose structured output streams.

Recommended stream kinds:

```rust
pub enum ExecutionStreamKind {
    Stdout,
    Stderr,
    Log,
    Diagnostic,
    Progress,
    Artifact,
    RuntimeEvent,
}
```

Logs should be associated with timestamps where useful, but timestamps must not
affect semantic validity.

Interactive runtimes may expose additional streams:

- Display frames
- Audio
- Input events
- Serial console
- Emulator debug output
- Browser console logs

---

## 13. Task Integration

Every execution must be associated with a daemon task.

Executors must report:

- Preparation started/completed
- Execution started
- Progress updates
- Logs
- Output artifact discoveries
- Completion/failure/cancellation
- Cleanup status

Task status is operational and must not be confused with core validation status.

---

## 14. Output Capture

Executors may capture:

1. Declared build outputs.
2. Declared run outputs.
3. Logs.
4. Screenshots.
5. Recordings.
6. Generated data files.
7. Runtime metadata.
8. Reproduction comparison results.

### 14.1 Declared outputs

Declared outputs are defined by the plan.

If a declared output is missing at completion, execution should fail unless the
plan marks it optional.

### 14.2 Discovered outputs

Discovered outputs may be recorded as supplemental artifacts.

They must not silently replace declared outputs.

### 14.3 Store ingestion

The daemon, not the executor alone, decides whether to ingest outputs into the
store and whether to create new statements.

Executors produce output records; daemon/core semantics decide how they become
archival data.

---

## 15. Execution Records

Executors must produce an execution record.

Recommended structure:

```rust
pub struct ExecutionRecord {
    pub execution_id: ExecutionId,
    pub task_id: TaskId,
    pub executor_id: ExecutorId,
    pub executor_version: String,
    pub plan_hash: Hash,
    pub snapshot: SnapshotRef,
    pub purpose: ExecutionPurpose,
    pub started_at: Timestamp,
    pub completed_at: Option<Timestamp>,
    pub status: ExecutionTerminalStatus,
    pub environment: ResolvedEnvironmentRecord,
    pub granted_capabilities: Vec<RuntimeCapability>,
    pub inputs: Vec<ExecutionInputRecord>,
    pub outputs: Vec<ExecutionOutputRecord>,
    pub logs: Vec<LogRef>,
    pub diagnostics: Vec<Diagnostic>,
}
```

Execution records are operational evidence. They do not by themselves modify object
truth unless incorporated through valid statements.

---

## 16. Reproducibility

Executors should support reproducibility metadata.

For reproducible execution, record:

1. Plan hash.
2. Snapshot ID.
3. Input artifact hashes.
4. Dependency snapshot IDs.
5. Environment image/hash/version.
6. Executor ID and version.
7. Host platform details.
8. Runtime capability grants.
9. Output artifact hashes.
10. Known nondeterminism.

### 16.1 Strict reproduction mode

In strict reproduction mode, executors must reject plans that depend on:

- Floating environment versions
- Unpinned network resources
- Host-global mutable state
- Uncontrolled timestamps/randomness, unless declared
- Unsupported nondeterministic devices

If strict reproduction cannot be guaranteed, the executor must report why.

---

## 17. Interactive Execution

Interactive executors may expose sessions.

Examples:

- Browser app session
- Emulator display/audio/input
- VM console
- Notebook kernel
- Game runtime

Interactive sessions must:

1. Be associated with daemon task/session IDs.
2. Expose granted capabilities.
3. Support stop/cancel where possible.
4. Avoid granting host access outside policy.
5. Keep UI/web-client surfaces separate from semantic validation.

---

## 18. Browser/Web Executor

A browser/web executor should support web-based objects and data viewers.

Responsibilities:

1. Serve or expose selected artifacts under an isolated origin or sandbox.
2. Apply Content Security Policy where possible.
3. Use iframe sandboxing or equivalent isolation.
4. Provide daemon-mediated APIs only when explicitly approved.
5. Avoid exposing local filesystem paths.
6. Capture browser console logs.
7. Report runtime events.

The browser/web executor must not treat arbitrary HTML/JS artifacts as safe inline
content merely because they are web-native.

---

## 19. Emulator Executor

An emulator executor should support retro or hardware-specific artifacts.

Examples:

- DOSBox
- QEMU
- MAME
- custom emulator cores

Responsibilities:

1. Prepare virtual media from artifacts.
2. Configure emulator devices.
3. Provide display/audio/input streams.
4. Capture emulator logs and outputs.
5. Record emulator version/configuration.
6. Enforce capability boundaries where possible.

Emulator configuration must be derived from the plan and approved policy.

---

## 20. Container/VM Executor

Container and VM executors should support build and run plans requiring isolated
modern environments.

Responsibilities:

1. Resolve images or base environments.
2. Verify image identity where possible.
3. Mount inputs read-only.
4. Mount outputs in controlled writable locations.
5. Apply network policy.
6. Apply resource limits.
7. Capture logs and outputs.
8. Clean up containers/VM overlays.

Host Docker socket or equivalent privileged access must not be exposed to object
content unless explicitly approved by policy.

---

## 21. Local Process Executor

A local process executor may exist for development or trusted workflows.

It is inherently less isolated.

Requirements:

1. Must be disabled by default or clearly marked as unsafe.
2. Must require explicit policy approval.
3. Must clearly display host access risk.
4. Must record host environment details.
5. Must not be used for untrusted remote objects by default.

---

## 22. Resource Limits

Executors should support resource limits.

Examples:

- CPU time
- Wall-clock time
- Memory
- Disk space
- Network access
- GPU access
- Process count
- File descriptor count

If a requested resource limit cannot be enforced, the executor must report this
before execution.

---

## 23. Cancellation and Failure

Executors must support cancellation where possible.

Cancellation modes:

1. Graceful stop.
2. Forced stop.
3. Kill/terminate.

Failures must be structured.

```rust
pub enum ExecutorErrorKind {
    UnsupportedPlan,
    PolicyMismatch,
    PreparationFailed,
    StartFailed,
    RuntimeFailed,
    OutputCaptureFailed,
    CleanupFailed,
    CapabilityNotEnforceable,
    Internal,
}
```

Cleanup failures must be reported even if execution otherwise completed.

---

## 24. Security Requirements

Executors must:

1. Never run without daemon request.
2. Never run without policy approval.
3. Never reinterpret validation status.
4. Treat artifacts as untrusted.
5. Sanitize paths.
6. Avoid ambient host authority.
7. Prefer read-only inputs.
8. Isolate writable outputs.
9. Enforce or report capability limits.
10. Avoid exposing secrets to object content.
11. Avoid exposing daemon control channels unless mediated.
12. Avoid granting network access by default.
13. Avoid unsafe local execution by default.
14. Clean up temporary resources.
15. Record execution provenance.

---

## 25. Error and Status Model

Execution status must distinguish:

```rust
pub enum ExecutionStatus {
    Preparing,
    Ready,
    Running,
    Completed(ExecutionResult),
    Failed(ExecutorError),
    Cancelled,
    Interrupted,
}
```

Terminal execution result:

```rust
pub struct ExecutionResult {
    pub exit_code: Option<i32>,
    pub outputs: Vec<ExecutionOutputRecord>,
    pub diagnostics: Vec<Diagnostic>,
}
```

A successful process exit does not imply semantic success unless the plan defines
that exit status as sufficient.

---

## 26. API/Daemon Integration

The daemon should expose executor-related API concepts:

- Available executors
- Executor compatibility check
- Execution preparation status
- Execution task status
- Runtime sessions
- Logs
- Outputs
- Capability grants
- Stop/cancel controls

The web client and CLI must interact through daemon APIs, not directly with
executor internals.

---

## 27. Testing Requirements

Executor implementations should include tests for:

1. Compatibility checks.
2. Rejection of unsupported plans.
3. Artifact materialization.
4. Path traversal prevention.
5. Capability denial.
6. Output capture.
7. Cleanup after success.
8. Cleanup after failure.
9. Cancellation.
10. Reproducibility metadata.
11. Log streaming.
12. Resource limit behavior where supported.

Test fixtures must avoid requiring privileged host access unless explicitly marked.

---

## 28. Implementation Checklist

A conforming initial executor system should provide:

1. Executor trait/interface.
2. Executor registry.
3. Compatibility matching.
4. Prepare/start/stop/inspect lifecycle.
5. Runtime capability model.
6. Artifact materialization utilities.
7. Working-directory management.
8. Log streaming.
9. Output capture.
10. Execution records.
11. Task integration.
12. Policy integration.
13. Cleanup handling.
14. At least one safe baseline executor.
15. Clear rejection for unsupported environments.
16. Structured executor errors.
17. Tests for path and capability safety.

---

End of `EXECUTOR.md`.
