# CORE_LIBRARY_SPEC.md

## Status

Superseded summary.

`CORE_LIBRARY.md` is the canonical core-library specification. This older short
document is retained only as historical orientation and must not override
`CORE_LIBRARY.md`.

---

## 1. Purpose

The Kairo Core Library is the canonical implementation of Kairo’s object semantics, validation model, and planning abstractions.

The core library:
- Interprets objects, statements, and builds
- Validates snapshots and authority
- Produces resolved object views
- Produces build and execution plans

The core library does NOT:
- Perform networking or federation
- Manage long-running processes
- Execute builds or environments
- Provide user interfaces

---

## 2. Scope and Non-Goals

### In Scope
- Object parsing and validation
- Statement DAG interpretation
- Snapshot resolution and validation
- Authority and capability evaluation
- Build and execution planning
- Trait-based provider interfaces

### Out of Scope
- DHT routing and peer discovery
- Federation protocols
- Background synchronization
- CLI behavior
- Daemon lifecycle management
- UI rendering
- Process execution (VMs, containers, browsers)

---

## 3. Relationship to Other Specs

The core library depends on:
- OBJECT.md
- BUILD.md
- STATEMENTS.md
- PLANNER.md

The following specs depend on the core library:
- CLI.md
- DAEMON.md
- WEB_CLIENT.md
- FEDERATION.md
- STORE.md

---

## 4. Core Concepts

### Object
A logical unit of archival content.

### Statement
An immutable, signed record forming a causal DAG.

### Actor
An identity capable of issuing signed statements.

### Snapshot
A selected state of an object defined by a statement frontier.

### SnapshotClosure
A purpose-scoped, closed set of data sufficient to validate and use a snapshot.

### Artifact
Content associated with an object (files, binaries, data).

### Environment
A runtime or build context.

---

## 5. Rust Crate Architecture

The core library is implemented in Rust with:
- Strong domain types
- Trait-based abstraction for IO
- No implicit side effects

---

## 6. Provider Traits

Core interacts with external systems via traits:

```rust
pub trait ObjectProvider {
    fn get_object(&self, id: &ObjectId) -> Result<Option<ObjectRecord>, CoreError>;
}

pub trait StatementProvider {
    fn get_statements(&self, id: &ObjectId) -> Result<StatementSet, CoreError>;
}

pub trait BlobProvider {
    fn get_blob(&self, id: &BlobId) -> Result<Option<Blob>, CoreError>;
}
```

---

## 7. Statement Model

Statements form:
- A per-object causal DAG
- Per-actor monotonic signed chains

Ordering is defined by:
- Actor chain (sequence)
- Explicit causal references

There is no global total order.

---

## 8. Snapshot Model

A snapshot is defined by:
- Object ID
- Statement frontier (latest statements per actor)

Snapshots are the primary unit of validation and execution.

---

## 9. Snapshot Closure Semantics

A snapshot closure guarantees that all required data is present for a specific purpose.

```rust
pub enum SnapshotPurpose {
    Inspect,
    Build,
    Run,
    Reproduce,
    ArchiveMirror,
}
```

Closure must include:
- Causal dependencies
- Authority chain
- Revocations and supersessions
- Required artifacts and blobs
- Required dependent object snapshots

---

## 10. Validation Model

Validation operates on snapshots.

```rust
pub enum ValidationStatus {
    Valid,
    Invalid,
    Conflicted,
    Indeterminate,
}
```

Indeterminate indicates insufficient closure.

---

## 11. Authority and Trust Evaluation

Authority is determined by:
- Capability grants
- Ownership chains
- Revocations

Rules:
- Authority must be provable via causal closure
- Revocations are not retroactive unless explicitly declared

---

## 12. Object Resolution

Core resolves:
- Effective object state
- Dependencies
- Artifacts
- Environments

---

## 13. Build and Run Planning

Core produces:

```rust
pub struct BuildPlan;
pub struct ExecutionPlan;
```

Core does NOT execute plans.

---

## 14. Extension Points

Core supports extension via traits:
- EnvironmentProvider
- Builder
- Planner
- Validator

---

## 15. Error Model

```rust
pub enum CoreError {
    ObjectNotFound,
    InvalidObject,
    TrustFailure,
    IncompleteData,
    UnsupportedEnvironment,
}
```

---

## 16. Security Requirements

- No implicit execution
- All data treated as untrusted until validated
- Signature verification required
- Strong typing to prevent misuse

---

## 17. Versioning and Compatibility

- Forward-compatible parsing required
- Unknown fields must be preserved
- Spec versions must be explicitly declared

---

## 18. Requirements for Dependent Specs

### CLI.md
Must map commands to core operations.

### DAEMON.md
Must use core as the authoritative interpreter.

### WEB_CLIENT.md
Must not define independent validation logic.

### FEDERATION.md
Provides data but not interpretation.

### STORE.md
Implements provider traits.

---

End of CORE_LIBRARY.md
