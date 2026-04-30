# CORE_LIBRARY.md

## Status

Draft specification.

This document describes the Kairo Core Library with enough precision to guide implementation. It is intentionally prescriptive. Later specifications for the daemon, CLI, web client, store, federation layer, and runtime executors must depend on the semantics defined here rather than redefining them.

---

## 1. Purpose

The Kairo Core Library is the canonical Rust implementation of Kairo object semantics.

The core library is responsible for:

1. Loading and interpreting Kairo object records.
2. Loading and interpreting statement sets.
3. Validating object snapshots.
4. Evaluating actor authority and capability chains.
5. Resolving effective object state from statements.
6. Producing build plans.
7. Producing run/execution plans.
8. Defining provider traits used by stores, federation clients, daemons, CLIs, and test harnesses.

The core library is not responsible for:

1. Long-running daemon lifecycle.
2. Network transport.
3. DHT routing.
4. Federation protocols.
5. Background synchronization.
6. Process execution.
7. VM/container/browser startup.
8. User-interface rendering.
9. Local object-store layout.

The core library may request data through explicit provider traits. The provider implementation may be local, remote, cached, synthetic, test-only, or federation-backed. The core library must not know or care which.

---

## 2. Design Principles

### 2.1 Determinism

Given identical inputs, the core library must produce identical outputs.

This applies to:

- Validation results
- Resolved object state
- Build plans
- Run plans
- Conflict reports
- Error classification

The core library must not use wall-clock time, network state, filesystem state, process state, random numbers, or provider arrival order as semantic inputs unless those values are explicitly represented in the supplied data.

### 2.2 Snapshot-first validation

The core library validates snapshots, not whole object histories by default.

A snapshot is a selected object state described by a statement frontier and a closure of required data. Full-history validation is optional audit behavior and must not be required for ordinary inspect/build/run/reproduce workflows.

### 2.3 Causal ordering, not global ordering

Kairo statements form a per-object causal DAG. Each actor’s statements for an object form a signed monotonic chain. There is no global total order of statements.

### 2.4 Explicit authority

No statement is authoritative merely because it exists, is signed, or is recent. A statement is authoritative only if the signer possessed the required capability at the point in the statement graph where the statement takes effect.

### 2.5 No implicit execution

Parsing, validation, resolution, planning, and provider access must not execute object content.

---

## 3. Relationship to Other Specifications

The core library consumes or interprets concepts defined by:

- `OBJECT.md`
- `BUILD.md`
- `STATEMENTS.md`
- `PLANNER.md`

The following specifications must depend on the core library:

- `STORE.md`
- `FEDERATION.md`
- `DAEMON.md`
- `CLI.md`
- `WEB_CLIENT.md`

### 3.1 Required dependency direction

`CLI.md`, `DAEMON.md`, and `WEB_CLIENT.md` must not define independent object validity semantics. They must call into the core library or faithfully expose results produced by it.

`STORE.md` may define local persistence mechanisms, but it must implement provider traits compatible with the core library.

`FEDERATION.md` may define remote discovery, synchronization, replication, and DHT behavior, but it must not redefine statement interpretation or authority semantics.

---

## 4. Rust Crate Structure

The implementation should be organized into modules similar to the following:

```text
kairo-core/
  src/
    lib.rs
    id.rs
    object.rs
    statement.rs
    snapshot.rs
    closure.rs
    authority.rs
    resolution.rs
    validation.rs
    build.rs
    run.rs
    provider.rs
    error.rs
    extension.rs
    version.rs
```

The exact layout may vary, but the implementation must preserve the conceptual separation between:

- Data model
- Provider traits
- Snapshot validation
- Authority evaluation
- Effective-state resolution
- Planning
- Error reporting

---

## 5. Domain Types

The core library must use strong domain types for identifiers. It must not pass semantically distinct identifiers as interchangeable strings.

Recommended Rust shapes:

```rust
pub struct ObjectId([u8; 32]);
pub struct ActorId([u8; 32]);
pub struct StatementId([u8; 32]);
pub struct BlobId([u8; 32]);
pub struct SnapshotId([u8; 32]);
pub struct BuildId([u8; 32]);
pub struct EnvironmentId([u8; 32]);
```

The byte length and encoding may be revised by the identifier specification, but the core library must preserve type distinction.

String encodings used for display, TOML, JSON, URLs, or CLI arguments must be parsed into strong types before semantic processing.

---

## 6. Trust-state Wrappers

The implementation should distinguish unverified data from verified data.

Recommended pattern:

```rust
pub struct Unverified<T>(pub T);
pub struct Verified<T>(pub T);
```

Parsing should produce unverified values. Validation should produce verified values or structured validation failures.

The implementation must not expose APIs that make it easy to accidentally treat unverified statements, objects, or snapshots as trusted.

---

## 7. Provider Traits

The core library obtains external data through provider traits.

### 7.1 ObjectProvider

```rust
pub trait ObjectProvider {
    fn get_object(&self, id: &ObjectId) -> Result<Option<ObjectRecord>, CoreError>;
}
```

Returns the object record for an object ID.

Returning `Ok(None)` means the provider does not have the object. It must not be treated as proof that the object does not exist globally.

### 7.2 StatementProvider

```rust
pub trait StatementProvider {
    fn get_statements(
        &self,
        object_id: &ObjectId,
        query: StatementQuery,
    ) -> Result<StatementSet, CoreError>;
}
```

Returns a statement set for an object.

A statement set may be partial. Completeness must be represented explicitly using snapshot closure metadata.

### 7.3 BlobProvider

```rust
pub trait BlobProvider {
    fn get_blob(&self, id: &BlobId) -> Result<Option<Blob>, CoreError>;
}
```

Returns blob content or metadata.

The core library must verify blob integrity against expected hashes before considering blob data usable.

### 7.4 SnapshotProvider

A convenience provider may be used for snapshot-oriented workflows:

```rust
pub trait SnapshotProvider {
    fn get_snapshot_closure(
        &self,
        reference: &SnapshotRef,
        purpose: SnapshotPurpose,
    ) -> Result<Option<SnapshotClosure>, CoreError>;
}
```

This trait is optional but recommended for daemon, store, and federation integration.

### 7.5 Provider requirements

Providers:

1. Must not be trusted implicitly.
2. May return incomplete data.
3. May return stale data.
4. May return internally inconsistent data.
5. Must not cause core semantics to become nondeterministic.

The core library must validate all provider-supplied data before relying on it.

---

## 8. Core Engine

The core library should expose an orchestration type similar to:

```rust
pub struct CoreEngine<OP, SP, BP> {
    pub objects: OP,
    pub statements: SP,
    pub blobs: BP,
}
```

Where:

```rust
OP: ObjectProvider
SP: StatementProvider
BP: BlobProvider
```

The engine may additionally accept registries for validators, planners, builders, environment providers, and policy evaluators.

Recommended operations:

```rust
impl<OP, SP, BP> CoreEngine<OP, SP, BP>
where
    OP: ObjectProvider,
    SP: StatementProvider,
    BP: BlobProvider,
{
    pub fn resolve_snapshot(
        &self,
        closure: SnapshotClosure,
    ) -> Result<ResolvedSnapshot, CoreError>;

    pub fn validate_snapshot(
        &self,
        closure: SnapshotClosure,
        purpose: SnapshotPurpose,
    ) -> Result<ValidationResult, CoreError>;

    pub fn plan_build(
        &self,
        closure: SnapshotClosure,
    ) -> Result<BuildPlan, CoreError>;

    pub fn plan_run(
        &self,
        closure: SnapshotClosure,
    ) -> Result<RunPlan, CoreError>;
}
```

The engine must not execute plans.

---

## 9. Statement Model

### 9.1 Statement graph

Statements form a per-object causal DAG.

Each statement must include:

```rust
pub struct Statement {
    pub id: StatementId,
    pub object_id: ObjectId,
    pub actor_id: ActorId,
    pub actor_seq: u64,
    pub previous_actor_statement: Option<StatementId>,
    pub causal_parents: Vec<StatementId>,
    pub kind: StatementKind,
    pub issued_at: Option<Timestamp>,
    pub body_hash: Hash,
    pub signature: Signature,
}
```

The exact serialized shape is defined by `STATEMENTS.md`; this structure describes required semantics.

### 9.2 Actor chains

For a given `(object_id, actor_id)` pair:

1. `actor_seq` must be strictly increasing.
2. The first statement in the actor chain must have either:
   - `actor_seq == 0`, or
   - the canonical first sequence value defined by `STATEMENTS.md`.
3. Every non-initial statement must reference the actor’s previous statement through `previous_actor_statement`.
4. The referenced previous statement must:
   - Exist in the snapshot closure, or
   - Be outside the closure only if the closure explicitly states that the predecessor is irrelevant for the requested purpose.
5. If a required previous statement is missing, validation must return `Indeterminate`.
6. If an actor chain is internally contradictory, validation must return `Invalid`.

### 9.3 Causal parents

`causal_parents` are explicit dependencies required to interpret a statement.

For every included statement:

1. Every causal parent required by the statement kind must be present in the snapshot closure.
2. If a required causal parent is missing, validation must return `Indeterminate`.
3. If a causal parent belongs to a different object, it must be represented through a dependent object snapshot reference.
4. Cycles in causal dependencies are invalid unless a future spec explicitly defines a cyclic construct.

### 9.4 Timestamps

`issued_at` is metadata.

The core library must not use timestamps to decide statement order, authority, conflict resolution, or validity unless a statement kind explicitly defines timestamp semantics.

---

## 10. Snapshot Model

A snapshot is a selected state of an object.

```rust
pub struct SnapshotRef {
    pub object_id: ObjectId,
    pub frontier: StatementFrontierSet,
}
```

A statement frontier identifies the statement heads used to define the snapshot.

```rust
pub struct StatementFrontierSet {
    pub entries: Vec<StatementFrontier>,
}

pub struct StatementFrontier {
    pub actor_id: ActorId,
    pub statement_id: StatementId,
    pub actor_seq: u64,
}
```

The frontier is not necessarily the latest global object state. It is the state selected for a particular operation.

### 10.1 Snapshot identity

A `SnapshotId` should be derived from:

1. Object ID.
2. Canonicalized frontier.
3. Snapshot purpose, if purpose-specific identity is required.
4. Dependency snapshot references, if included.

The canonicalization rules must be deterministic.

### 10.2 Latest/current snapshots

A snapshot may claim to represent the latest or current effective state. Such a claim requires stronger closure than an ordinary historical snapshot.

For a historical snapshot, later statements are irrelevant unless the snapshot closure explicitly includes them or the selected statement semantics depend on them.

For a latest/current snapshot, the closure must prove that no relevant later statement changes the effective state for the requested purpose.

---

## 11. Snapshot Closure

### 11.1 Definition

A snapshot closure is the complete set of data needed to validate and use a snapshot for a stated purpose.

```rust
pub struct SnapshotClosure {
    pub snapshot: SnapshotRef,
    pub purpose: SnapshotPurpose,
    pub object: ObjectRecord,
    pub statements: Vec<Statement>,
    pub dependencies: Vec<DependentSnapshot>,
    pub artifacts: Vec<ArtifactRecord>,
    pub blobs: Vec<BlobRecord>,
    pub closure_claim: ClosureClaim,
}
```

### 11.2 Snapshot purposes

```rust
pub enum SnapshotPurpose {
    Inspect,
    Build,
    Run,
    Reproduce,
    ArchiveMirror,
}
```

### 11.3 Closure claim

```rust
pub enum ClosureClaim {
    ClaimedClosed,
    Partial,
    FullObjectLog,
}
```

`ClaimedClosed` means the provider claims the closure is sufficient for the requested purpose.

`Partial` means the closure is known to be incomplete.

`FullObjectLog` means the closure contains the full known statement log for the object and may support audit workflows.

The core library must still verify internal closure properties. A provider claim is not sufficient by itself.

### 11.4 Closure requirements common to all purposes

Every snapshot closure must include:

1. The object record.
2. All statements named in the snapshot frontier.
3. All causal ancestors required to interpret frontier statements.
4. All actor-chain predecessors required to validate included statements.
5. All authority statements required to evaluate included statements.
6. All revocation statements that affect included authority statements.
7. All supersession statements that affect selected effective state.
8. All dependent object snapshots required by selected statements.
9. All artifact records required by selected statements.
10. All blob records required by selected artifacts.

If any required item is missing and no contradiction is found, validation must return `Indeterminate`.

### 11.5 Inspect closure

For `Inspect`, closure must be sufficient to display a resolved object summary.

It must include:

1. Object identity.
2. Display metadata selected by the snapshot.
3. Authority facts needed to mark the displayed state as verified or unverified.

If authority facts are missing, the object may be displayed as unverified, but validation for verified inspection must return `Indeterminate`.

### 11.6 Build closure

For `Build`, closure must include:

1. All build declarations selected by the snapshot.
2. All source artifacts required by those builds.
3. All dependency snapshots required by those builds.
4. All environment declarations required to plan the build.
5. All planner statements required to select a build path.
6. Authority facts for build-affecting statements.
7. Revocations and supersessions affecting selected build statements.

### 11.7 Run closure

For `Run`, closure must include:

1. All runtime artifact declarations.
2. All selected build outputs, if the run depends on a build.
3. All required environment declarations.
4. All dependency snapshots.
5. All planner statements required to select a run path.
6. Authority facts for run-affecting statements.
7. Revocations and supersessions affecting selected run statements.

### 11.8 Reproduce closure

For `Reproduce`, closure must include everything required for `Build` and `Run`, plus:

1. Exact artifact hashes.
2. Exact build input hashes.
3. Exact environment identifiers or reproducible environment descriptors.
4. Dependency snapshot IDs.
5. Planner versions or planner content hashes.
6. Any declared nondeterminism or irreproducibility metadata.

### 11.9 ArchiveMirror closure

For `ArchiveMirror`, closure should include the full object log, all known artifacts, all known blobs, all known dependency references, and all known statement branches.

This is the only standard purpose that normally expects full object history.

---

## 12. Validation Result

Validation must return structured results.

```rust
pub struct ValidationResult {
    pub status: ValidationStatus,
    pub issues: Vec<ValidationIssue>,
    pub resolved_snapshot: Option<ResolvedSnapshot>,
}
```

```rust
pub enum ValidationStatus {
    Valid,
    Invalid,
    Conflicted,
    Indeterminate,
}
```

### 12.1 Valid

`Valid` means:

1. Required closure is present for the requested purpose.
2. All required signatures verify.
3. All actor chains are valid.
4. All causal dependencies are present and acyclic.
5. All required authority checks pass.
6. No unresolved conflicts affect the requested purpose.
7. All required artifact and blob hashes verify.

### 12.2 Invalid

`Invalid` means a contradiction or violation is proven.

Examples:

- Invalid signature.
- Incorrect content hash.
- Broken actor chain where the required predecessor is present and inconsistent.
- Statement claims a previous statement that belongs to another actor chain.
- Unsupported or malformed statement kind.
- Actor definitely lacks required authority.
- Blob hash mismatch.

### 12.3 Conflicted

`Conflicted` means two or more valid authoritative statements produce incompatible effective state for the requested purpose and no deterministic resolution rule resolves them.

### 12.4 Indeterminate

`Indeterminate` means the supplied data is not sufficient to decide validity for the requested purpose, and no invalidity or conflict has been proven.

Examples:

- Missing causal parent.
- Missing authority grant.
- Missing relevant revocation information.
- Missing artifact record.
- Missing dependency snapshot.
- Missing blob hash.

---

## 13. Validation Algorithm

The core library must validate a snapshot closure using the following conceptual pipeline.

Implementations may optimize, but observable results must be equivalent.

### 13.1 Algorithm

Given `(SnapshotClosure closure, SnapshotPurpose purpose)`:

1. Verify object identity.
   - Ensure `closure.object.id == closure.snapshot.object_id`.

2. Index statements by `StatementId`.

3. Verify frontier membership.
   - Every statement named in the frontier must be present.

4. Verify statement object identity.
   - Every statement in the main closure must target the closure object unless represented as part of a dependent snapshot.

5. Verify signatures.
   - Every statement signature must verify against its actor identity.

6. Verify statement body hashes.
   - Every statement body hash must match the canonical serialized body.

7. Verify actor chains.
   - For each actor chain, check sequence monotonicity and previous-statement links.

8. Verify causal closure.
   - Every required causal parent must be present or represented by a dependent snapshot.

9. Detect graph cycles.
   - Causal cycles are invalid.

10. Build the authority graph.
    - Interpret root authority, ownership statements, capability grants, delegations, and revocations.

11. Evaluate statement authority.
    - For each statement, determine whether the actor had the capability required by the statement kind at that graph point.

12. Apply revocations and supersessions.
    - Mark statements as active, revoked, superseded, or historically valid as appropriate.

13. Resolve effective state.
    - Apply active authoritative statements according to deterministic statement-kind rules.

14. Detect conflicts.
    - Identify incompatible active statements that lack a deterministic resolution.

15. Verify artifact and blob requirements.
    - Confirm all required artifacts and blobs are present and hashes match.

16. Verify dependency snapshot requirements.
    - Recursively validate required dependent snapshots for their required purposes.

17. Return `ValidationResult`.

### 13.2 Priority of statuses

If multiple issues exist, status should be selected in this priority order:

1. `Invalid`
2. `Conflicted`
3. `Indeterminate`
4. `Valid`

This means proven invalidity dominates missing data.

---

## 14. Authority Evaluation

### 14.1 Root authority

Every object must have a root authority established by object creation or an equivalent root statement defined by `STATEMENTS.md`.

No non-root statement may be authoritative unless it can trace required authority back to a valid root authority.

### 14.2 Capabilities

A capability grants an actor permission to issue one or more statement kinds for an object or object subtree.

A capability should include:

```rust
pub struct Capability {
    pub object_scope: ObjectScope,
    pub statement_kinds: Vec<StatementKindDiscriminant>,
    pub grantor: ActorId,
    pub grantee: ActorId,
    pub constraints: Vec<CapabilityConstraint>,
}
```

### 14.3 Capability evaluation

For each statement:

1. Determine required capability for the statement kind.
2. Locate a valid capability path from root authority to the statement actor.
3. Ensure all grants in the path were valid before or at the statement’s causal position.
4. Ensure no relevant revocation invalidates the capability before the statement.
5. Ensure constraints are satisfied.
6. If the capability path is missing but could exist outside the closure, result is `Indeterminate`.
7. If no valid capability path can exist given the closure, result is `Invalid`.

### 14.4 Delegation

Capabilities may be delegable or non-delegable.

If a capability is non-delegable, a grantee may use it but may not grant it to another actor.

Delegation chains must be explicit in the statement graph.

### 14.5 Revocation

Revocations are statements.

By default:

1. Revocations apply only to statements causally after the revocation.
2. Revocations do not retroactively invalidate earlier statements.
3. Retroactive invalidation requires an explicit retroactive revocation statement kind.
4. Retroactive revocation must itself require stronger authority than ordinary revocation.
5. Historical snapshots that predate a revocation remain valid unless retroactive revocation semantics apply.

### 14.6 Supersession

Supersession replaces the effective contribution of a prior statement without deleting it from history.

A superseded statement remains historically valid but inactive for effective-state resolution when the supersession is included in the selected snapshot.

---

## 15. Effective State Resolution

### 15.1 ResolvedSnapshot

```rust
pub struct ResolvedSnapshot {
    pub snapshot: SnapshotRef,
    pub object_id: ObjectId,
    pub effective_object: ResolvedObject,
    pub active_statements: Vec<StatementId>,
    pub inactive_statements: Vec<InactiveStatement>,
    pub conflicts: Vec<Conflict>,
}
```

### 15.2 Resolution order

The core library must apply statements in a deterministic topological order.

Ordering keys:

1. Causal parent order.
2. Actor chain order.
3. Statement ID as deterministic tie-breaker for independent statements.

Tie-breaking must not be used to hide semantic conflicts. It may only provide deterministic iteration order.

### 15.3 Statement-kind semantics

Each statement kind must define one of the following merge behaviors:

1. Set/additive.
2. Replacement.
3. Unique singleton.
4. Ordered list append.
5. Revocation.
6. Supersession.
7. Capability grant.
8. Capability revocation.
9. Custom resolver.

Examples:

- `AddTag`: set union.
- `AddArtifact`: additive unless artifact identity conflicts.
- `SetTitle`: replacement; concurrent unresolved replacements conflict.
- `SetLicense`: singleton; concurrent unresolved values conflict.
- `GrantCapability`: modifies authority graph.
- `RevokeCapability`: modifies authority graph.
- `SupersedeStatement`: deactivates target for effective state.

### 15.4 Conflict rules

A conflict exists when:

1. Two or more active authoritative statements affect the same semantic field.
2. Their effects are incompatible.
3. Neither causally supersedes the other.
4. No statement-kind resolver or authority rule resolves the incompatibility.

Conflicts must be reported explicitly.

The core library must not silently choose a winner based on timestamp, provider order, or arrival order.

---

## 16. Artifact and Blob Validation

Artifacts and blobs are content-addressed or hash-verified.

For every artifact required by a snapshot purpose:

1. Its declaration must be present.
2. Its referenced blobs must be present or resolvable.
3. Its expected hashes must be present.
4. Blob content or metadata must match expected hashes before use.

If a required blob is absent, validation is `Indeterminate`.

If a required blob is present but hash verification fails, validation is `Invalid`.

---

## 17. Dependency Snapshots

Objects may depend on other object snapshots.

A dependency must reference:

```rust
pub struct ObjectSnapshotRef {
    pub object_id: ObjectId,
    pub snapshot: SnapshotRef,
    pub required_purpose: SnapshotPurpose,
}
```

The core library must validate dependency snapshots for the required purpose before considering the parent snapshot valid for a purpose that depends on them.

Dependency validation may be recursive. Implementations must detect dependency cycles.

Dependency cycles are invalid unless a future spec explicitly defines cycle semantics.

---

## 18. Build Planning

The core library produces build plans from validated or sufficiently resolvable snapshots.

```rust
pub struct BuildPlan {
    pub snapshot: SnapshotRef,
    pub steps: Vec<BuildStep>,
    pub inputs: Vec<ArtifactRef>,
    pub outputs: Vec<ArtifactRef>,
    pub environment: EnvironmentDescriptor,
    pub dependencies: Vec<ObjectSnapshotRef>,
}
```

A build plan must be deterministic for a given validated snapshot.

The core library must not execute build steps.

If a build cannot be planned because required closure is missing, planning must return `Indeterminate` or an equivalent structured planning error.

If a build cannot be planned because the object is invalid or conflicted, planning must fail with the corresponding validation status.

---

## 19. Run Planning

The core library produces run plans from validated or sufficiently resolvable snapshots.

```rust
pub struct RunPlan {
    pub snapshot: SnapshotRef,
    pub entrypoint: Entrypoint,
    pub environment: EnvironmentDescriptor,
    pub artifacts: Vec<ArtifactRef>,
    pub dependencies: Vec<ObjectSnapshotRef>,
    pub required_capabilities: Vec<RuntimeCapability>,
}
```

The core library must not launch the runtime.

Runtime capabilities must be explicit. Examples:

- Filesystem read
- Filesystem write
- Network access
- Display access
- Audio access
- Input device access
- Browser APIs
- Emulator devices

The daemon or runtime executor decides whether to allow these capabilities under local policy.

---

## 20. Environment Resolution

The core library may describe required environments and may invoke environment-provider traits to resolve descriptors into plans.

```rust
pub trait EnvironmentProvider {
    fn resolve_environment(
        &self,
        descriptor: &EnvironmentDescriptor,
    ) -> Result<ResolvedEnvironment, CoreError>;
}
```

Environment providers must not start environments during resolution.

Environment resolution produces descriptions, plans, or handles that can later be executed by daemon/runtime components.

---

## 21. Extension Points

The core library may support extension through registries and traits.

Recommended extension traits:

```rust
pub trait StatementValidator {
    fn validate_statement(
        &self,
        statement: &Statement,
        context: &ValidationContext,
    ) -> Result<Vec<ValidationIssue>, CoreError>;
}

pub trait StateResolver {
    fn apply_statement(
        &self,
        state: &mut ResolutionState,
        statement: &Verified<Statement>,
    ) -> Result<(), CoreError>;
}

pub trait Planner {
    fn plan(
        &self,
        snapshot: &ResolvedSnapshot,
        purpose: SnapshotPurpose,
    ) -> Result<Plan, CoreError>;
}
```

Extension behavior must be deterministic.

Extensions must not perform implicit execution during validation or planning.

---

## 22. Error Model

The core library must distinguish validation outcomes from operational errors.

Recommended top-level error shape:

```rust
pub enum CoreError {
    Provider(ProviderError),
    Parse(ParseError),
    UnsupportedVersion(Version),
    UnsupportedFeature(String),
    InternalInvariantViolation(String),
}
```

Validation failures should usually be represented as `ValidationIssue`, not `CoreError`.

Examples:

```rust
pub enum ValidationIssueKind {
    MissingObject,
    MissingStatement,
    MissingCausalParent,
    MissingAuthorityFact,
    InvalidSignature,
    InvalidHash,
    BrokenActorChain,
    UnauthorizedStatement,
    ConflictingStatements,
    MissingArtifact,
    MissingBlob,
    DependencyCycle,
    UnsupportedStatementKind,
}
```

---

## 23. Versioning and Compatibility

All serialized objects, statements, build specs, planner specs, and snapshot closures must declare compatible spec versions.

The core library must reject unsupported required features.

The core library may preserve unknown fields for round-tripping, but must not assign semantic meaning to unknown fields.

Unknown optional fields must not cause validation failure.

Unknown required fields or required feature flags must cause validation to return `Invalid` or an unsupported-feature error.

---

## 24. Security Requirements

The core library must:

1. Treat all provider data as untrusted.
2. Verify signatures before authority evaluation.
3. Verify hashes before using content.
4. Avoid implicit code execution.
5. Avoid path traversal when interpreting artifact paths.
6. Avoid ambient authority.
7. Avoid global mutable state.
8. Avoid nondeterministic semantic behavior.
9. Make runtime capabilities explicit in plans.
10. Keep validation separate from execution.

The core library should be safe to use against malicious object data.

---

## 25. Requirements for Dependent Specs

### 25.1 STORE.md

`STORE.md` must define local persistence in terms of core provider traits.

It must specify how local records map to:

- `ObjectProvider`
- `StatementProvider`
- `BlobProvider`
- `SnapshotProvider`

### 25.2 FEDERATION.md

`FEDERATION.md` must define how remote peers locate and transport data. It must not define independent validation semantics.

Federation implementations may produce `SnapshotClosure` values, but the core library validates whether those closures are sufficient.

### 25.3 DAEMON.md

`DAEMON.md` must use the core library as the authority for:

- Snapshot validation
- Effective object state
- Build planning
- Run planning
- Runtime capability declarations

The daemon may enforce local policy beyond core validation.

### 25.4 CLI.md

`CLI.md` must express commands in terms of core operations.

For example:

- `kairo inspect` maps to snapshot resolution and inspect validation.
- `kairo build` maps to build snapshot validation and build planning.
- `kairo run` maps to run snapshot validation and run planning.
- `kairo verify` maps to explicit validation.

### 25.5 WEB_CLIENT.md

`WEB_CLIENT.md` must display core validation state accurately.

The web client must distinguish:

- Verified
- Invalid
- Conflicted
- Indeterminate
- Unverified preview

It must not present unverified or indeterminate state as verified.

---

## 26. Implementation Checklist

A conforming initial implementation should provide:

1. Strong identifier types.
2. Parser structures for object and statement records.
3. Provider traits.
4. Snapshot and snapshot-closure types.
5. Signature verification interface.
6. Hash verification interface.
7. Actor-chain validation.
8. Causal-closure validation.
9. Authority graph construction.
10. Capability and revocation evaluation.
11. Effective-state resolver.
12. Conflict detector.
13. Artifact/blob verifier.
14. Dependency snapshot validator.
15. Build planner interface.
16. Run planner interface.
17. Structured validation results.
18. Structured errors.
19. Deterministic test fixtures.

---

End of `CORE_LIBRARY.md`.
