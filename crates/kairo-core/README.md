# kairo-core

Foundational types shared by every other crate in the workspace:
content-addressed identifiers (`ActorId`, `ObjectId`, `StatementId`,
`BlobId`, `SnapshotId`), `KairoRef`, `Timestamp`, and the canonical
encoding primitives (`encode_str`, `encode_option`, `encode_list`,
domain-tagged `multihash`-style id derivation).

No domain logic lives here — kairo-core defines *how* identity is
derived from canonical bytes; every other crate composes those
primitives into specific record types.

**Position in the dependency stack:** leaf. kairo-core depends on no
other workspace crate.

**Read more:** crate-level docs in `src/lib.rs`, plus
`specs/IDENTIFIERS.md` and `schemas/canonical/README.md` for the
canonical encoding model.
