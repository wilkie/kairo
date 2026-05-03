# kairo-statement

Signed statement types and the verification model. Defines the
`StatementBody` trait and every body type Kairo currently signs:
`ObjectGenesisBody`, `ObjectRevisionBody`, `ObjectBranchBody`,
`ObjectVersionTagBody`, and `ActorTrustBody`. Wraps each in
`UnsignedStatement` / `SignedStatement` envelopes with a `Signature`
and a derived `StatementId`.

Verification lives in `verify`: `verify_envelope_statement` resolves
the envelope actor through an `ActorResolver` and reports signature
status, actor resolution, and trust evaluation independently;
`evaluate_trust` folds an `ActorTrust` chain leaf into a
`TrustEvaluation`. Trust is informational and never overrides
cryptographic validity.

The `json` submodule defines round-trippable JSON DTOs used by the
store, bundle, and CLI for on-disk and on-wire interchange.

**Position in the dependency stack:** sits above `kairo-core` and
`kairo-identity`. Depended on by `kairo-object`, `kairo-store`,
`kairo-bundle`, `kairo-git`, and `kairo-cli`.

**Read more:** crate-level docs in `src/lib.rs`, `specs/STATEMENTS.md`
(§4 catalog, §6 verification model), and the per-type schema docs
under `schemas/canonical/` and `schemas/json/`.
