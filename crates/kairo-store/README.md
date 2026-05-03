# kairo-store

Local filesystem store for Kairo records. `FilesystemStore`
implements every persistence trait — `ActorStore`, `ObjectStore`,
`StatementStore`, `BranchResolver`, `VersionTagResolver`,
`TrustResolver`, `BlobStore` — plus the `kairo_identity::ActorResolver`
trait so `kairo-statement::verify` consumes the store directly.

Records live under `~/.kairo/` (override with `--store`), sharded
two levels deep using base58 chars from each id's payload. Materialized
indices for branches, version tags, and trust live alongside the
underlying signed statements; rebuilding from `statements/` is
correct-by-construction but not implemented in MVP.

Errors split semantic (`Missing`), fixity (`Corrupt`), and operational
(`Unavailable`) failures so callers can react differently.

**Position in the dependency stack:** sits above `kairo-core`,
`kairo-identity`, and `kairo-statement`. Depended on by `kairo-bundle`
and `kairo-cli`.

**Read more:** crate-level docs in `src/lib.rs`, `specs/STORE.md` §4
(MVP layout subsection has the full sharded directory tree), and
`memory/project_store_design.md` for the design rationale.
