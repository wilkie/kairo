# RUST.md

## Status

Draft implementation convention.

This document defines Rust conventions for Kairo implementation work. It should
be read alongside `PROJECT_LAYOUT.md`, `CORE_LIBRARY.md`, and `IDENTIFIERS.md`.

---

## 1. Purpose

Kairo Rust code should make specification errors difficult to represent.

The main goals are:

1. Strong domain types.
2. Explicit parsing at boundaries.
3. Deterministic formatting and serialization.
4. Small crates with intentional dependencies.
5. Clear separation between semantic validation and operational errors.

---

## 2. Workspace Conventions

Kairo should use a Cargo workspace rooted at the repository root.

Rust crates should live under:

```text
crates/
```

Preferred crate pattern:

- shared primitive/domain crates first
- higher-level semantic crates depend on primitives
- daemon, CLI, federation, and executor crates depend on semantic crates
- binaries should be thin wrappers over library crates

The CLI crate may be named `kairo-cli`, but the installed binary name is:

```text
kairo
```

Crate dependencies must remain acyclic.

---

## 3. Crate Boundaries

Core primitive crates should stay small and dependency-light.

Initial low-level crates may include:

- `kairo-core` or `kairo-ids` for IDs, typed references, hashes, and common
  domain primitives.
- `kairo-identity` for cryptographic identity and signature verification.
- `kairo-object` for `kairo.toml` and Object metadata.
- `kairo-statement` for statement envelopes and statement body schemas.

Higher-level crates should not leak implementation details back into core
primitive crates.

---

## 4. Type Discipline

Kairo IDs must be represented as newtypes, not raw `String`s, after parsing.

Example:

```rust
pub struct ObjectId(String);
pub struct SnapshotId(String);
pub struct StatementId(String);
pub struct BlobId(String);
pub struct ActorId(String);
```

Fields should be private by default.

Constructors must validate:

```rust
impl ObjectId {
    pub fn new(value: impl Into<String>) -> Result<Self, IdError>;
    pub fn as_str(&self) -> &str;
}
```

Stringly typed semantic code is not acceptable once data has crossed a parsing
boundary.

---

## 5. References

Typed references should be represented as enums.

Example:

```rust
pub enum KairoRef {
    Object(ObjectId),
    ObjectSnapshot {
        object: ObjectId,
        snapshot: SnapshotId,
    },
    Statement(StatementId),
    Blob(BlobId),
    Actor(ActorId),
}
```

The parser must accept only the forms defined by `IDENTIFIERS.md`.

It must reject unsupported alternate spellings rather than normalizing them.

---

## 6. Parsing and Formatting

Use `FromStr` for parsing canonical string forms.

Use `Display` for canonical formatting.

Required round-trip property:

```text
parse(display(value)) == value
```

Parsing should happen at system boundaries:

- CLI input
- API input
- manifest loading
- package import
- federation message ingestion
- store record loading

Internal code should pass typed values.

Do not duplicate reference parsing logic outside the shared parser.

---

## 7. Serialization

Use `serde` for structured serialization.

ID and reference newtypes must not deserialize invalid strings.

Use one of:

- `TryFrom<String>` / `From<T> for String`
- custom serde modules
- explicit wrapper DTOs

Normal API serialization and canonical cryptographic serialization are separate
concerns. Do not assume default `serde_json` output is canonical.

Canonical serialization rules must be defined by the relevant semantic spec
before they are used for hashing or signing.

---

## 8. Error Handling

Library crates should use structured errors.

Recommended:

```rust
#[derive(Debug, thiserror::Error)]
pub enum IdError {
    #[error("empty identifier")]
    Empty,

    #[error("invalid identifier encoding")]
    InvalidEncoding,

    #[error("invalid reference syntax")]
    InvalidReference,
}
```

Use `thiserror` for library errors.

Use `anyhow` only in binaries, tests, examples, and developer tooling.

Semantic validation outcomes must remain distinct from operational errors.

Example distinction:

- `ValidationStatus::Invalid`
- `ValidationStatus::Indeterminate`
- `CoreError::Io`
- `CoreError::UnsupportedSchema`

---

## 9. Visibility and Mutability

Default to private fields.

Expose read-only accessors unless mutation is part of the type's invariant.

Avoid public mutable structs for semantic domain data.

Prefer construction through validated builders or constructors for records with
non-trivial invariants.

---

## 10. Naming

Use Rust standard naming conventions:

- types and traits: `UpperCamelCase`
- functions and modules: `snake_case`
- constants: `SCREAMING_SNAKE_CASE`
- crates: `kebab-case`

Preferred type names:

- `ObjectId`
- `SnapshotId`
- `StatementId`
- `BlobId`
- `ActorId`
- `KairoRef`
- `ExternalRef` if a separate external reference type is needed

Preferred module names:

- `ids`
- `refs`
- `parse`
- `serde`
- `error`

---

## 11. Dependencies

Core primitive crates should avoid heavy dependencies.

Reasonable initial dependencies:

- `serde`
- `thiserror`
- `camino` where UTF-8 paths are needed

Avoid these in low-level core crates unless there is a strong reason:

- async runtimes
- HTTP clients/servers
- database clients
- CLI parsers
- logging frameworks with global initialization

Higher-level crates such as daemon, federation, executor, and CLI crates may use
heavier dependencies appropriate to their role.

---

## 12. Testing

Every parser and formatter must have table-driven tests.

Tests should cover:

- accepted bare ID payloads
- rejected malformed ID payloads
- accepted internal references
- accepted external `kairo:` references
- rejected unsupported alternate spellings
- `Display` / `FromStr` round trips
- serde deserialize failures for invalid IDs

Property tests may be added later for parser/formatter round trips.

Fixtures should live under:

```text
tests/fixtures/
```

when they are shared across crates or integration tests.

---

## 13. Formatting and Lints

Use `rustfmt` defaults.

The standard local verification commands should be:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Workspace lint configuration may be added once the Cargo workspace exists.

---

## 14. Unsafe Code

Unsafe Rust is not allowed in core semantic crates unless a design note explains:

1. Why it is necessary.
2. What invariant makes it sound.
3. How it is tested.
4. Why a safe alternative was rejected.

Most Kairo crates should not need unsafe code.

---

## 15. Implementation Checklist

Initial Rust implementation should provide:

1. Cargo workspace.
2. Low-level ID/reference crate or module.
3. Newtypes for Object, Snapshot, Statement, Blob, and Actor IDs.
4. Parser and formatter for internal typed references.
5. Parser and formatter for external `kairo:` references.
6. Serde integration for ID and reference types.
7. Structured parse errors.
8. Unit tests for accepted and rejected forms.
9. `cargo fmt`, `cargo clippy`, and `cargo test` passing.

---

End of `RUST.md`
