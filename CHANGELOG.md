# Changelog

All notable changes to Kairo are recorded here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), with one
addition: a **`Wire format`** section calls out changes to canonical
bytes (which determine `ActorId` / `ObjectId` / `StatementId`). See
[`VERSIONING.md`](./VERSIONING.md) for the policy.

This project adheres to pre-1.0 [Cargo semver](https://doc.rust-lang.org/cargo/reference/semver.html):
`0.x.0` for breaking changes (Rust API or wire format), `0.x.y` for
additions and fixes.

This file was created mid-flight — entries before its introduction are
reconstructed from git history and grouped at coarse-grained slice level
rather than per-commit.

## [Unreleased]

### Added

- **Cold-storage attestation keys (`ACTORS.md` §5.5).** New per-actor
  attestation key set declared in `ActorGenesis.attestation_keys`,
  used to sign emergency key events without touching the operational
  signing key. New body types: `ActorEmergencyKeyRotation`,
  `ActorEmergencyKeyRevocation`, `ActorAttestationKeyAdd`,
  `ActorAttestationKeyRevocation`. Recovery flow lets an operator
  rotate to a fresh active signing key after compromise of the
  operational key.
- **M-of-N attestation thresholds (`ACTORS.md` §5.5.3).** New
  `attestation_threshold: u8` on `ActorGenesis`. Attestation-surface
  envelopes carry `signatures: Vec<Signature>` (≥ threshold distinct
  attestation-set signers). New body type
  `ActorAttestationThresholdChange` with the asymmetric authority
  rule (raises require `max(current, new)` distinct sigs, lowers
  require `current`).
- **`MultiSignedStatement<B>` envelope** in `kairo-statement`, used by
  every attestation-surface body. Sorted, distinct-`key_id`
  signatures; `verify_envelope_multi_statement` checks threshold +
  per-signature membership in the attestation set.
- **CLI verb tree for attestation keys.**
  - `kairo actor recover-key {sign,prepare,submit}` — emergency
    key rotation via cold-storage attestation key.
  - `kairo actor add-attestation-key {sign,prepare,submit}` —
    append a new attestation key to the actor's set.
  - `kairo actor revoke-attestation-key {sign,prepare,submit}` —
    retract a previously-held attestation key.
  - `kairo actor change-attestation-threshold {sign,prepare,submit}` —
    mutate the M-of-N quorum.
  - `kairo actor co-sign --prepared <path> --actor <id>
    --attestation-key-seed <path>` — append a single signature to a
    partial multi-sig envelope.
  - `kairo actor key-history` extended with attestation adds,
    revocations, and the threshold trajectory (with per-event
    `quorum_at_event`).
- **Capability / delegation model (`CAPABILITIES.md`).** Body types
  `ActorCapabilityGrant` and `ActorCapabilityRevocation`, with scope
  (Object / Actor), statement-kind allowlist, delegation depth, and
  expiration constraints. `evaluate_capability` resolves chained
  grants with cycle detection. New CLI verb tree:
  `kairo capability {grant,revoke,list}`.
- **First-person trust (`STATEMENTS.md` §4.2e).** Body type
  `ActorTrust` with `Trusted` / `Untrusted` / `withdraw`
  decisions, parameterized by the asking truster. `verify object`
  surfaces trust as `trusted` / `untrusted` / `unknown` /
  `unevaluated`. New CLI verb tree:
  `kairo trust {grant,block,withdraw,show,list,history}`.
- **Object branches and version tags.** Body types `ObjectBranch`
  and `ObjectVersionTag`, with cross-actor `supersedes` chains.
  Snapshot identity over the resolved frontier. New CLI verbs:
  `kairo branch {set,show,list}`, `kairo tag
  {bind,revoke,show,list,history}`, `kairo snapshot compute`.
- **Object bundles (`PACKAGE.md`).** Directory-format export/import
  carrying actors, objects, statements, blobs, and a manifest with
  expected Git commit references. New CLI verbs:
  `kairo bundle {export,import}`. Git history is _not_ in the bundle
  in v1; the manifest declares which commits the receiver needs.
- **End-to-end object verification.** `kairo verify object` rolls up
  six independent dimensions (genesis fixity, frontier resolution,
  signature, actor consistency, manifest binding, content layer)
  into a single `VALID` / `INDETERMINATE` / `INVALID` verdict.
  Optional `--as <id>` to evaluate trust from a specific truster.
- **Git integration (`kairo-git`).** Read-only `gix`-backed
  repository handle used by `verify object` to look up the storage
  commit and validate the manifest against the commit's tree.
- **Property tests** for canonical-encoding determinism + JSON DTO
  round-trip across every body type
  (`kairo-statement/tests/property_tests.rs`,
  `kairo-identity/tests/property_tests.rs`).
- **README walkthrough integration test**
  (`examples_readme_walkthrough_round_trip` in `kairo-cli/src/tests.rs`)
  mirrors `examples/README.md` step-for-step so the walkthrough doesn't
  bit-rot when CLI verbs change.
- **Threat model** (`specs/THREAT_MODEL.md`) consolidating the
  security argument: assets, adversaries, defended attacks with
  mechanism cross-references, residual risk, explicit non-goals.
- **MSRV: Rust 1.95.** Pinned in `[workspace.package].rust-version`.

### Changed

- Renamed `kairo actor recover-key import` and `kairo actor
  add-attestation-key import` to `submit` for symmetry with
  `prepare` / `co-sign`. The same verb finalizes an envelope
  whether it was assembled inline (`--signature <path>`) or through
  the cosign flow.
- `kairo actor recover-key prepare` and `add-attestation-key
  prepare` now emit a partial envelope with `signatures: []`
  (previously a placeholder-signature hack). `co-sign` and `submit`
  append signatures into the empty array; `submit` constructs the
  `MultiSignedStatement` and verifies the threshold.
- `kairo-cli`'s `main.rs` split into focused modules: `cli` (clap
  definitions), `error` (`CliError`), `format` (verification
  reports), `store_paths` (path resolution), `commands/` (per-verb
  runners), `tests` (the integration test block). 10458-line
  single file → 4640-line dispatcher with sibling modules.

### Wire format

Items in this section change the canonical bytes a body hashes to,
or otherwise alter the on-the-wire shape. Each is a `0.x.0` bump.

- **`ActorGenesis` v1 schema gained
  `attestation_keys: Vec<PublicKey>` and `attestation_threshold:
  u8`.** Both participate in canonical bytes, so the resulting
  `ActorId` differs from any pre-attestation-keys actor body. Every
  existing actor in pre-introduction stores must be re-derived;
  there is no compatibility shim because nothing was deployed yet.
- **Attestation-surface envelopes now carry `signatures` (array)
  instead of `signature` (single).** Affects the JSON shape for
  `ActorEmergencyKeyRotation`, `ActorEmergencyKeyRevocation`,
  `ActorAttestationKeyAdd`, `ActorAttestationKeyRevocation`. The
  multi-sig envelope's canonical bytes exclude signatures (same as
  the single-sig envelope), so the body StatementId is unchanged.
- **New body type
  `ActorAttestationThresholdChange`** added to the canonical
  schema set; carries `new_threshold: u8`. See
  `schemas/canonical/actor-attestation-threshold-change-v1.md`.
- **`ObjectBranch` v1 gained an optional `supersedes`
  field.** Pre-supersedes branch records remain
  parseable (the field is `None`); new records that set it derive
  a different StatementId.

### Fixed

- *(none recorded — defects fixed during initial development are
  squashed into the implementing slice's entry above.)*

[Unreleased]: https://example.invalid/kairo
