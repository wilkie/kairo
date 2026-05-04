# Phase 1: Local Trust and Storage Core

Phase 1 of the Kairo implementation plan. The goal of this phase was to prove
that Kairo can create, identify, store, and verify signed object history
**locally** — the foundation every later phase composes against.

This phase is **complete** as of the latest commit on `main` (with the two
explicitly-deferred bullets in §2 noted as such). What follows is preserved as
a historical receipt of what shipped, not a live to-do list — the live plan
for the next chapter lives in `specs/PHASE_2.md` (forthcoming).

## Phase 1 Goal

Build a direct/local Kairo workflow that can:

1. Define an actor.
2. Define an object lineage.
3. Record object revisions.
4. Bind revisions to `kairo.toml` manifests.
5. Store statements and manifests locally.
6. Verify signatures, IDs, manifest hashes, and basic revision history.
7. Expose the workflow through the `kairo` CLI.

Federation, daemon orchestration, build execution, provider planning, and the
web client are out of scope for Phase 1 and pick up in Phase 2 onward.

## Already Started

- [x] Rust workspace scaffold.
- [x] Core ID types using sha256 multihash encoded as multibase base58btc.
- [x] Internal and external object/snapshot reference parsing.
- [x] Canonical primitive encoding helpers.
- [x] `ActorGenesis` identity model.
- [x] Ed25519 public key and signature verification.
- [x] `ActorResolver` boundary with an in-memory MVP resolver.
- [x] `ObjectGenesis` statement body and canonical object ID derivation.
- [x] `ObjectRevision` statement body and canonical statement ID derivation.
- [x] JSON interchange parsing for actor genesis and early object statements.
- [x] `kairo.toml` parsing and canonical manifest hashing.
- [x] CLI commands for manifest hashing/inspection and early revision checks.
- [x] Initial schema documentation for JSON and canonical forms.

## 1. Generic Statement Verification

- [x] Add a statement-verification API that uses `ActorResolver`.
- [x] Return structured outcomes for:
  - valid signature
  - invalid signature
  - missing actor data
  - unsupported key algorithm
  - malformed statement
- [x] Make verification generic over supported signed statement types.
- [x] Refactor CLI `revision verify-actor-genesis` to use the generic verifier.
- [x] Document the verification result model in `STATEMENTS.md` and `ACTORS.md`.

## 2. Local Store Crate

- [x] Scaffold `kairo-store`.
- [x] Define provider traits for statement, actor, object, and blob lookup.
- [x] Implement a filesystem store suitable for direct/local mode.
- [x] Persist actor genesis documents by derived `ActorId`.
- [x] Persist statements by `StatementId`.
- [ ] Index statements by object, actor, and statement type.
- [x] Add integrity checks on store read.
- [ ] Add store fixtures for tests and CLI examples.

## 3. Object Revision Validation

- [x] Define a structured `ObjectRevision` validation report.
- [x] Validate object ID consistency.
- [x] Validate revision parent references at the statement layer.
- [x] Validate manifest hash against parsed `kairo.toml`.
- [x] Validate optional `[kairo].object` consistency.
- [x] Represent missing Git/content data as indeterminate, not valid.
- [x] Document what `revision`, `parents`, and `manifest_hash` prove.

## 4. Object Initialization Workflow

- [x] Add CLI command to create or print an `ActorGenesis` document.
- [x] Add CLI command to create an `ObjectGenesis` statement.
- [x] Add CLI command to create an `ObjectRevision` statement from a local tree.
- [x] Decide how local private keys are represented for MVP signing.
- [x] Keep key storage simple but explicit; avoid pretending it is production key
      management.
- [x] Provide an end-to-end example object under `examples/`.

## 5. Statement Import and Inspection

- [x] Add CLI command to import actor genesis data into the local store.
- [x] Add CLI command to import object statements into the local store.
- [x] Add CLI command to inspect statements by ID.
- [x] Add CLI command to list statements for an object.
- [x] Add CLI JSON output for verification reports.
- [x] Ensure imported statements preserve signed bytes exactly where applicable.

## 6. Snapshot Frontier and Effective State

- [x] Define the minimal snapshot frontier model.
- [x] Decide how active statements are selected for the MVP.
- [x] Compute a first `SnapshotId` from object ID, statement frontier, and
      effective manifest state.
- [x] Document which fields affect snapshot identity.
- [x] Add tests proving snapshot IDs are deterministic.

### §6 supporting work (landed alongside)

- [x] `ObjectBranch` statement type — named, actor-scoped, mutable revision
      pointer with explicit `supersedes` chain (added in-place to v1
      alongside `CAPABILITIES.md` §6.2; chain precedence overrides the
      `(created_at, statement_id)` tiebreak, matching `ObjectVersionTag`).
- [x] `BranchResolver` trait + per-object materialized branch tip index in
      `kairo-store`.
- [x] `kairo branch set / show / list` CLI.
- [x] `kairo snapshot compute` CLI defaulting to creator-actor's "head"
      branch with `--actor`, `--name`, and `--statement` overrides.

## 7. Version Tags

- [x] Define a `VersionTag` statement (`ObjectVersionTag`, actor-scoped,
      latest-wins, semver-validated, with explicit `supersedes` chain
      and bind/revoke shapes).
- [x] Add canonical documentation and JSON schema.
- [x] Validate that version tags point to an object revision (the MVP
      target type) — bind statements reference an `ObjectRevision`
      `StatementId`; revocation statements reference a prior tag via
      `supersedes` and have `target = null`.
- [x] Add CLI support for binding, revoking, listing, resolving, and
      walking history of version tags (`kairo tag bind|revoke|show|list|history`).
- [x] Keep version names separate from content identity (consumers that
      need stability pin to the resolved `StatementId`/`SnapshotId`,
      not to the version string).

## 8. Store-Backed Verification CLI

- [x] Add `kairo verify object --object <id>` for direct/local mode
      (mirrors `snapshot compute` flag shape: `--actor`, `--name`,
      `--statement`, plus `--manifest` for binding check).
- [x] Resolve actors through the local store (via `FilesystemStore`'s
      `ActorResolver` impl).
- [x] Resolve object statements through the local store (genesis
      via `ObjectStore`, frontier via `BranchResolver` or
      `--statement` pin, revision via `StatementStore`).
- [x] Verify statement signatures and object revision manifest
      bindings (via `verify_envelope_statement` and
      `validate_object_revision`).
- [x] Report valid, invalid, and indeterminate results clearly with a
      single overall verdict aggregated from per-check statuses
      (worst-of: any INVALID → INVALID; any INDETERMINATE → INDETERMINATE;
      else VALID). Until §11, the strongest reachable verdict is
      INDETERMINATE because the content layer is unimplemented.
- [x] Support stable JSON output for automation (`--json`).

## 9. Package / Bundle Format

- [x] Define a minimal export bundle for actors, statements, manifests,
      and referenced blobs (`crates/kairo-bundle`; directory layout
      `manifest.json` + `actors/` + `objects/` + `statements/` +
      `blobs/`; `manifest.schema = "kairo.bundle.v1"`). One bundle root
      per call: a single object's full known statement history, every
      signing actor, every referenced blob.
- [x] Add import validation for bundle structure and hashes.
      `import_bundle` re-derives every record's id from its bytes,
      verifies blob hashes against `OBJECT_MANIFEST_DOMAIN` (the only
      blob domain MVP bundles ship), checks every statement's signing
      actor is in the bundle, and rejects unsupported schema versions.
- [x] Preserve statement IDs and signatures across import/export.
      The `kairo-statement::json` DTOs round-trip canonical bytes
      losslessly; `revision create` now also persists the manifest
      blob into the store so bundles include the bytes signed-into the
      revision.
- [x] Keep bundle trust separate from local trust policy. `ActorTrust`
      statements are intentionally excluded from object bundles — trust
      is first-person and shipping it inside an object bundle would
      invite reading peer opinions as authority. A separate
      trust-bundle type can land later.

Future work tracked here: optional `git/` subdirectory carrying the
referenced Git history (`git_history.included = true`), with import
populating a `~/.kairo/git/` managed mirror so verification reaches
`VALID` from the bundle alone. The MVP manifest already declares the
expected commit ids in `git_history.expected_commits` so recipients
know exactly which Git history to obtain externally; the schema is
forward-compatible with the included-Git case. Tar/zip transport,
deterministic export, snapshot-closure / archive-mirror / execution-
record bundle types, and bundle-level signing all remain deferred to
later work tracked in `specs/PACKAGE.md`.

## 10. Trust Policy MVP

- [x] Define local trust records for actors (`ActorTrust` statement
      type — `schemas/canonical/actor-trust-v1.md`,
      `schemas/json/actor-trust-v1.schema.json`,
      `kairo_statement::ActorTrustBody`).
- [x] Distinguish cryptographic validity from local trust
      (`TrustEvaluation` is independent of `SignatureStatus` and
      `ActorResolution`; trust never overrides cryptographic validity
      and vice versa).
- [x] Add CLI commands to trust/untrust an actor locally
      (`kairo trust grant|block|withdraw|show|list|history`,
      auto-supersedes the truster's prior opinion when chaining).
- [x] Include trust status in verification reports
      (`kairo verify object` learned `--as <by-actor>` /
      `--no-as`; auto-picks the sole keystore actor when neither is
      given, errors when ambiguous; trust line / JSON field reports
      `trusted | untrusted | unknown | unevaluated` plus the truster
      that produced the verdict).
- [x] Avoid treating trust as object authority (trust is informational —
      `aggregate_overall_status` does not consult it; trust shapes UI /
      consumer decisions, not validation outcomes).

Sharded `trust/<XX>/<YY>/<trusted-actor-id>.json` (see
`crates/kairo-store/src/trust.rs`) is keyed by the trusted actor so
federation aggregation ("what does the world say about Y?") is O(1);
`list_trust(by_actor)` in the MVP walks the trust directory to filter
by truster. The deferred work is a parallel per-truster reverse index
when that scan becomes hot.

Federation-related trust work (forgetting peer opinions, propagating
trust across a node) stays deferred — see §9 (bundles). The
capability-style delegation that lets one actor supersede another's
claims has since landed in `specs/CAPABILITIES.md`; both the
`ObjectVersionTag` and `ObjectBranch` resolver flips honor cross-actor
`supersedes` when a covering capability evaluates to `Held` (Phase 2
§3 in `PHASE_2.md`). The branch flip rides an in-place addition to
`ObjectBranch v1` rather than a v2 schema bump, since the system was
not yet deployed.

## 11. Git Content Integration

- [x] Decide the exact MVP format for Git revision IDs (`git:sha256:<commit>`,
      strict prefix; non-prefixed revisions are reported as content-layer
      `Indeterminate`).
- [x] Read the current commit and parents from a local Git repository
      (`kairo-git` crate built on `gix`, read-only operations).
- [x] Verify that an `ObjectRevision.revision` exists locally
      (`ContentLayerCheck::CommitNotFound` when absent).
- [x] Verify declared parents against Git commit parents where available
      (set-equality; `ContentLayerCheck::ParentMismatch` on disagreement).
      Parent ordering is intentionally not enforced.
- [x] Treat missing repository data as indeterminate
      (`ContentLayerCheck::Indeterminate` when `--no-repo` or when the
      revision lacks the `git:sha256:` prefix).
- [x] Discover the repo from the working directory (`gix::discover`),
      fall back to `--repo <path>` for explicit control, with `--no-repo`
      to opt out.
- [x] Read `kairo.toml` from the commit's tree as the default manifest
      source (`--manifest <path>` remains as an override).
- [x] `verify object` can now reach `VALID` end-to-end (genesis +
      signature + actor + object consistency + manifest binding +
      content layer).

The future `~/.kairo/git/` managed mirror and bundle-import-of-Git-pack
are deferred to §9 (bundles) and the federation/daemon work; this
section delivered just the read-only verification path against the
user's working repo.

## 12. Documentation Pass

- [x] Keep `specs/STATEMENTS.md`, `specs/ACTORS.md`, `specs/OBJECT.md`, and
      `specs/STORE.md` aligned with implementation. STATEMENTS §6.2 content-
      layer claim and §kairo-verify-object caller note refreshed; ACTORS §6.2
      already updated alongside §10; OBJECT §2.3 / §Tags-vs-branches
      verified accurate; STORE §4 gained an MVP layout subsection that
      catalogs the actual sharded directories under `~/.kairo/`.
- [x] Add schema docs for each new statement before or alongside code.
      All six statement types (`ActorGenesis`, `ObjectGenesis`,
      `ObjectRevision`, `ObjectBranch`, `ObjectVersionTag`, `ActorTrust`)
      have both `schemas/canonical/<type>-v1.md` and
      `schemas/json/<type>-v1.schema.json`.
- [x] Keep `specs/CLI.md` aligned with actual command names and output.
      §5.1 lists every command implemented in `kairo-cli` and explains
      the trust + `verify object --as` semantics.
- [x] Add an MVP walkthrough from actor creation through object
      verification. `examples/README.md` now drives the full flow:
      actor → object → revision → branch → tag → snapshot →
      `verify object` → trust opinion → re-verify with trust resolved.
      Spec spec sections it does not duplicate are linked from the
      "What this demonstrates" footer.

## Carry-Over Bullets

Two items in §2 are explicitly deferred rather than complete; both are
performance/ergonomics rather than correctness gaps and can be picked up
inside Phase 2 once a real consumer needs them:

- [ ] Index statements by object, actor, and statement type. The
      materialized-index pattern in `kairo-store` only adds an index when a
      query consumer demands it. Branches, version tags, and trust each
      have their indices today; "all statements by actor X" and
      "all statements of type T" do not yet have a query consumer.
- [ ] Add store fixtures for tests and CLI examples. Tests currently
      rebuild fixtures inline; a shared fixture crate would reduce
      duplication once Phase 2 starts adding test surface against the
      existing crates.

## Phase 2 Candidates

Surfaces deliberately deferred from Phase 1, presented here as the menu Phase
2 will draw from. The list is descriptive (what could come next) not
prescriptive (what must) — `specs/PHASE_2.md` will pick a focused slice of
this, the way Phase 1 picked a focused slice of the long-term spec.

- Daemon-backed workflows.
- `~/.kairo/git/` managed Git mirror (unblocks self-contained bundles and
  the Git-included bundle path declared as forward-compat in §9).
- Federation protocols.
- Search indexes.
- Provider objects and capability resolution.
- Build execution.
- Run execution.
- Web client.
- Key rotation and revocation semantics beyond the initial actor key.
- Multi-actor authority and delegation. The capability model
  (`specs/CAPABILITIES.md`) has since landed in Phase 2 §3 and unlocked
  cross-actor `supersedes` for both `ObjectVersionTag` and
  `ObjectBranch` (the latter via an in-place addition to v1, since the
  system was not yet deployed). `ActorTrust` cross-actor `supersedes`
  was deliberately kept invalid (Decision B in `CAPABILITIES.md` §9).
