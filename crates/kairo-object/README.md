# kairo-object

Pure (no-I/O) object validation primitives: `ObjectManifest` parsing
from `kairo.toml`, `validate_object_revision` and its
`ObjectRevisionValidationReport` (object-consistency, manifest-binding,
parent, content checks), `Snapshot` computation, `CommitLookup` for
the content layer.

The validator is deliberately pure — callers fetch the genesis,
manifest, and Git commit lookup; missing inputs are reported as
`*NotProvided` / `Indeterminate` rather than as failure. The store-
backed `kairo verify object` command is the primary caller.

**Position in the dependency stack:** sits above `kairo-core`,
`kairo-identity`, and `kairo-statement`. Depended on by `kairo-bundle`
and `kairo-cli`.

**Read more:** crate-level docs in `src/lib.rs`, plus
`specs/OBJECT.md` and `specs/STATEMENTS.md` §6.2 for the validation
report semantics.
