# Phase 2: Plan

Phase 2 of the Kairo implementation plan. Phase 1 (`specs/PHASE_1.md`) shipped
the local trust and storage core; Phase 2 catalogs the next-step surfaces and
tracks which ones land. Unlike Phase 1's "do all of these" shape, Phase 2 is
**a menu** — items are independent enough that we can sequence by appetite,
defer entire sections to Phase 3+, or pull bullets across sections if a
focused slice makes more sense than a whole numbered chunk.

## Phase 2 Goal

Move Kairo from "a single user can prove signed object history locally" to a
position where the next strategic surfaces — federation, multi-actor
authority, and runtime/build execution — can be designed and implemented
without rework of the core. The shortest path to that position requires:

1. Closing the storage story (managed Git mirror; multi-process safety).
2. Standing up the long-running services that downstream surfaces depend on
   (daemon).
3. Doing the spec-first design work for the items the long-term system needs
   but the MVP could legally defer (capability model, federation protocol).
4. Hardening what's already shipped (integration tests, property tests,
   release-engineering basics, threat model).

A focused Phase 2 might pick **one closing item + one foundational service +
one spec-first design pass** rather than attempting all twelve sections.

## Catalog

### 1. `~/.kairo/git/` Managed Git Mirror

Pairs with Phase 1 §9 and §11 deferred work. The MVP's `kairo verify object`
walks upward to find the user's working Git repo; bundles do not carry Git
history. A managed mirror at `~/.kairo/git/` would:

- [ ] Decide layout: single bare repo per node, per-object bare repo, or a
      packed-object store. Document the choice in `specs/STORE.md`.
- [ ] Add `kairo-git` write paths (or a sibling crate) for fetching commits
      from a remote URL or from a Git pack file.
- [ ] Teach `verify object` to consult the managed mirror as a fallback (or
      primary, depending on layout) before the cwd-discovered repo.
- [ ] Flip `BundleGitHistory.included` to `true` in a future bundle version,
      ship a `git/` subdirectory with a Git pack, and ingest into the
      managed mirror on import — making bundles self-contained for the
      VALID verdict.
- [ ] Add CLI: `kairo git fetch --object <id> --remote <url>` (or similar)
      and `kairo git mirror status`.

**Why it matters:** unblocks self-contained bundles; required by federation
to serve Git data without depending on the user's working tree.

### 2. Daemon

Long-running local service that coordinates store access, federation,
policy, scheduling, and (eventually) build/run execution. `specs/DAEMON.md`
and `specs/API.md` already describe the intended shape; this section is
about implementation.

- [ ] Reconcile `specs/DAEMON.md` and `specs/API.md` with what's been built
      since the spec was written; mark stale assumptions and update.
- [ ] Stand up a minimal `kairo-daemon` binary that exposes a local Unix-
      socket API for store reads.
- [ ] Add `kairo daemon start | status | stop` to the CLI.
- [ ] Decide protocol: HTTP+JSON, custom framed binary, or gRPC. Document
      the choice in `specs/API.md`.
- [ ] Implement `kairo` CLI's daemon-mode dispatch (`specs/CLI.md` §3.1
      already describes the mode-selection rules).
- [ ] Multi-process safety: file locks in `kairo-store` (see §6) become
      mandatory once daemon + CLI may write concurrently.

**Why it matters:** every downstream surface (web client, federation-as-
service, daemon-mode CLI) depends on the daemon existing.

### 3. Capability / Delegation Model

**Status: implemented.** Phase 1 deferred cross-actor authority claims
(`ObjectVersionTag`'s cross-actor `supersedes` was recorded but not honored;
multi-maintainer flows were unsupported). The capability model now lives in
`specs/CAPABILITIES.md` and ships end-to-end:

- [x] `specs/CAPABILITIES.md` defines `Capability { scope, statement_kinds,
      delegable, constraints }` with locked decisions A–G in §9.
- [x] First-person sharding (Decision A): per-grantor index in
      `kairo-store::capabilities`; per-object reverse index in
      `kairo-store::capabilities_by_object` for the §6.1 evaluator's hot
      path.
- [x] `ObjectVersionTag` cross-actor `supersedes` is honored by
      `kairo-store::FilesystemStore::latest_version_tag` when a covering
      capability evaluates to `Held` at the successor's `created_at`
      (`CAPABILITIES.md` §6.2).
- [x] `ActorTrust` cross-actor `supersedes` stays invalid even with
      capabilities (Decision B in §9) — trust is first-person opinion;
      indirect trust as a tiebreaker is the right primitive there.
- [x] Key rotation (`CAPABILITIES.md` §7): grants anchor on `ActorId` and
      survive routine rotation; the opt-in `KeyPinned` constraint binds a
      grant to a specific signing key for high-stakes delegations.
- [x] CLI: `kairo capability grant / revoke / list`.
- [x] `ObjectBranch` cross-actor `supersedes` (parallel to the version-
      tag flip). Added `supersedes` to `ObjectBranch v1` in place — the
      system was not yet deployed, so no `StatementId` migration was
      needed and the v2 schema bump originally planned in §12 is no
      longer required. Resolver is
      `kairo-store::FilesystemStore::latest_branch` /
      `list_branches` via `walk_authorized_branch_chain`; CLI is
      `kairo branch set` (auto-chains on put). See `CAPABILITIES.md` §6.2.

**Why it matters (now realized):** federation policy
(`specs/POLICY.md`) and multi-maintainer workflows can build on the
authority oracle (`evaluate_capability` in `kairo-statement::verify`)
without inventing their own.

### 4. Federation Protocol

`specs/FEDERATION.md` exists and describes the long-term design. Phase 2
turns the spec into something a Kairo node can actually do.

- [ ] Reconcile `specs/FEDERATION.md` with implementation reality (what
      bundles already cover, what trust already covers).
- [ ] Define the on-wire transport (likely HTTP + bundle stream, possibly
      `application/vnd.kairo.bundle` content type).
- [ ] Decide which bundle types federate: object bundles, trust bundles,
      capability bundles, snapshot-closure bundles.
- [ ] Implement push (`kairo federate push --to <url>`) and pull (`kairo
      federate pull --from <url> --object <id>`).
- [ ] Implement peer discovery and trust propagation policy
      (`specs/POLICY.md`) — explicitly *opt-in*; importing a peer's bundle
      never auto-trusts that peer.
- [ ] Forgetting peer opinions: the `forget` operation deferred during
      Phase 1 §10 trust work.

**Why it matters:** validates the entire bundle + trust design under real
multi-node use; surfaces wrinkles before users hit them.

**Depends on:** §1 (Git mirror) for serving Git data, §2 (daemon) for
running federation as a service, possibly §3 (capability model) for
cross-actor authority during sync.

### 5. Web Client

`specs/WEB_CLIENT.md` describes the long-term TypeScript/React surface.
Phase 2 stands up the minimum needed for browse/inspect.

- [ ] Decide MVP scope: read-only inspector? Or also expose
      object/revision/branch/tag/trust write paths via the daemon API?
- [ ] Reconcile `specs/WEB_CLIENT.md` and `specs/API.md` with current
      implementation (esp. statement types added in Phase 1).
- [ ] Stand up project structure for the client (separate workspace? same
      monorepo? new directory?).
- [ ] Implement object browser (genesis + branches + tags + revision
      history).
- [ ] Implement verify-object UI surface that calls the daemon.

**Why it matters:** primary user-facing surface for the federation /
archival use case.

**Depends on:** §2 (daemon) — there is no web client without a daemon API.

### 6. Multi-Process Safety / File Locks

The current `FilesystemStore` is single-process; concurrent writers race on
read-modify-write of materialized indices (branches, version tags, trust).
Note added throughout the store doc: "Multi-process safety is not yet
enforced; concurrent writers can race on read-modify-write."

- [ ] Decide locking strategy: per-record advisory locks, per-shard locks,
      or one store-wide lock. Document the choice.
- [ ] Use `fs2` or similar for cross-platform `flock`.
- [ ] Add tests that exercise concurrent put_*/get_* across processes.
- [ ] Decide failure mode for lock-acquisition timeout (error vs. retry).

**Why it matters:** required the moment §2 ships — daemon + CLI will run
concurrently against the same store.

### 7. Build / Run Execution

`specs/BUILD.md`, `specs/EXECUTOR.md`, `specs/PLANNER.md`,
`specs/ENVIRONMENTS.md` describe the long-term build and run model. Phase 2
is the spec-first pass plus a minimum executable.

- [ ] Reconcile the four specs with each other and with what's been
      implemented since they were written.
- [ ] Decide MVP scope: just declarative build description in `kairo.toml`?
      Or actual build invocation (`kairo build --object <id>`)?
- [ ] Implement deterministic build planning (planner consults the snapshot
      frontier and resolved dependencies).
- [ ] Implement an MVP executor (probably native subprocess; container /
      VM executors are larger work).
- [ ] Add execution-record bundle type (deferred from `specs/PACKAGE.md`
      §4.4).

**Why it matters:** moves Kairo from "verify history" to "do work" — the
core value proposition for science / build-reproducibility users.

**Depends on:** §3 (capability model) if executors need scoped permissions.

### 8. Provider Objects and Capability Resolution

`specs/PLANNER.md` and `specs/ENVIRONMENTS.md` describe provider objects
(objects that declare they can provide a tool/runtime/library/environment).
Required by §7 if builds need toolchains.

- [ ] Decide MVP scope of provider declaration in `kairo.toml`.
- [ ] Implement provider resolution against the local store.
- [ ] Add CLI: `kairo provider list / show / resolve`.

**Why it matters:** the dependency-resolution backbone for builds.

**Depends on:** §3 (capability model) if "I can provide X" is itself a
capability claim.

### 9. Search Indexes

`specs/SEARCH.md` describes a long-term search/discovery layer. Phase 2 is
the MVP slice.

- [ ] Decide what's searchable: actors, objects, statements, blobs?
- [ ] Decide indexer: SQLite? Tantivy? Plain inverted-file?
- [ ] Implement the index alongside the materialized indices already in
      `kairo-store`.
- [ ] Add CLI: `kairo search <query>`.

**Why it matters:** discoverability for federated content; secondary to the
trust + verify story.

### 10. Key Rotation and Revocation

`specs/ACTORS.md` §5.4 / §5.5 describe the actor key chain (active /
rotated / revoked). Phase 1 only deals with each actor's *initial*
key. Real-world security needs rotation and revocation.

**Spec slice (committed first):**

- [x] `ActorKeyRotation` statement type spec
      (`schemas/canonical/actor-key-rotation-v1.md`,
      `schemas/json/actor-key-rotation-v1.schema.json`,
      `STATEMENTS.md` §4.2f).
- [x] `ActorKeyRevocation` statement type spec
      (`schemas/canonical/actor-key-revocation-v1.md`,
      `schemas/json/actor-key-revocation-v1.schema.json`,
      `STATEMENTS.md` §4.2g).
- [x] `ACTORS.md` §5.5 key chain — active-key-at-causal-position
      and revocation-status-at-causal-position composed into one §6.1
      verification rule.
- [x] `CAPABILITIES.md` §7.2 made enforceable: `KeyPinned` is no
      longer just declarative, the §10 impl slice will wire its
      enforcement through `evaluate_capability`.

**Impl slice:**

- [x] `kairo-statement::{ActorKeyRotationBody, ActorKeyRevocationBody}`
      with canonical encoding + JSON DTOs.
- [x] Extend `kairo-identity::ActorResolver` (or add a sibling trait)
      with `active_key_at(actor, at)` and
      `is_key_revoked_at(actor, key_id, at)` — returning the §5.5 query
      results. `MemoryActorResolver` and `FilesystemStore` impls.
- [x] Per-actor key-event index in `kairo-store` (sharded on
      `actor_id`, mirroring trust): `put_actor_key_rotation`,
      `put_actor_key_revocation`, with the materialized index that
      drives the two resolver queries.
- [x] Update `verify_envelope_statement` to consume
      `signature.key_id` and the new resolver methods (the field is
      currently recorded but ignored).
- [x] `KeyPinned` constraint enforcement in
      `kairo-statement::verify::evaluate_capability` — collapses to
      `CapabilityEvaluation::Revoked` when the pinned key is revoked
      at the evaluated causal position.

**CLI slice:**

- [x] `kairo actor rotate-key --actor <id> [--keys <path>]` — generates
      a fresh signing key, signs and persists an `ActorKeyRotation`
      using the prior active key, and stores the new key in the
      keystore alongside the prior one (so the actor retains the
      ability to verify historical statements).
- [x] `kairo actor revoke-key --actor <id> --key <key-id>
      [--retroactive] [--reason <text>] [--brick-actor]` — signs and
      persists an `ActorKeyRevocation` using the actor's current
      active key. Refuses to revoke the only active key (which would
      brick the actor per `ACTORS.md` §5.5.1) unless `--brick-actor`
      is passed; the help text points operators at `actor rotate-key`
      as the safe alternative.
- [x] `kairo actor key-history --actor <id> [--json]` — diagnostic
      surface listing the key chain (genesis-initial + rotations) and
      revocation set in causal order.

**Why it matters:** baseline security hygiene; required for any
real-world multi-year actor identity. Also retires the `KeyPinned`
deferred bullet in `CAPABILITIES.md` §8.

### 11. Bundle Extensions

Deferred from Phase 1 §9 (`specs/PACKAGE.md` "MVP slice").

- [ ] Optional `git/` subdirectory in bundles + import side ingestion into
      the §1 managed mirror (paired work).
- [ ] Tar/zip archive transport (`*.kairo.tar` per `specs/PACKAGE.md` §5.2).
- [ ] Deterministic export (`specs/PACKAGE.md` §17).
- [ ] Snapshot-closure bundle type (`specs/PACKAGE.md` §4.2).
- [ ] Archive-mirror bundle type (`specs/PACKAGE.md` §4.3).
- [ ] Execution-record bundle type (`specs/PACKAGE.md` §4.4 — paired with
      §7).
- [ ] Trust-bundle type (transport per-truster opinions independently of
      object bundles).
- [ ] Bundle-level signature (`specs/PACKAGE.md` §24).
- [ ] Per-truster reverse trust index (currently `list_trust(by_actor)`
      walks the trust dir).
- [ ] Materialized-index rebuild from `statements/` (today every index
      depends on always going through `put_*`).

**Why it matters:** the bundle MVP shipped a deliberately narrow slice;
this is the explicit roadmap for filling out the long-term `PACKAGE.md`
shape as real consumers need it.

### 12. Branches, Tags, Trust v2

Statement-type evolution that Phase 1 explicitly deferred.

- [x] `ObjectBranch` `supersedes` chain. **Cancelled as a v2 schema
      bump**: since the system was not yet deployed when §3 landed, the
      `supersedes` field was added to `ObjectBranch v1` in place. No
      `StatementId` migration was needed. The cross-actor flip rides
      the same in-place edit (see §3 above and `CAPABILITIES.md` §6.2).
- [x] `ObjectVersionTag` cross-actor `supersedes` honored by the resolver.
      Implemented in `kairo-store::FilesystemStore::latest_version_tag` /
      `list_version_tags` per `specs/CAPABILITIES.md` §6.2.
- [ ] `ActorTrust` `forget` operation (federation concern — flush peer
      opinions from the local node without re-publishing a withdrawal).
- [ ] Schema bump migration tooling for the v1 → v2 transitions (still
      needed for any *future* schema evolution that lands post-deployment).

**Why it matters:** statement schemas are content-addressed, so v2
migrations are real work post-deployment. The capability model in §3
landed before the system was deployed, so the branch `supersedes`
addition was free; later schema evolutions will need the migration
tooling tracked above.

### 13. Polish: Tests, Property Tests, Release Engineering, Threat Model

Hardening what Phase 1 shipped.

- [ ] Programmatic integration test that runs `examples/README.md`
      end-to-end (so the walkthrough doesn't bit-rot when commands change).
- [ ] Property tests for canonical-encoding determinism (`proptest` over
      every body type's `CanonicalEncode` impl: parse → re-encode →
      byte-equal).
- [ ] Property tests for JSON DTO round-tripping (random body → JSON →
      back → equal).
- [ ] Statement-type indexing (carry-over from Phase 1 §2).
- [ ] Store fixtures crate (carry-over from Phase 1 §2; Phase 2 starts
      adding test surface that benefits from shared fixtures).
- [ ] Versioning policy: pick semver discipline, document MSRV, write a
      `CHANGELOG.md`.
- [x] Threat model document — `specs/THREAT_MODEL.md`. Drafted as a
      consolidation of the security argument scattered across the
      spec set: assets, adversaries, defended-against attacks (with
      mechanism cross-references and residual risk), explicit
      non-goals, social recovery, operator monitoring. Surfaces the
      v1 gap that an `ActorAttestationKeyRevocation` should close
      (Phase 2 §14 follow-on).
- [ ] Security review of the keystore (mode bits, atomic write semantics,
      passphrase encryption deferred-but-documented).

**Why it matters:** the MVP works but is unhardened. Polish here makes
every later Phase 2 / Phase 3 item cheaper and safer to land.

### 14. Cold-Storage Attestation Keys

`ACTORS.md` §5.5.2 declares a separate cold-storage authority surface
that signs only emergency key events. This closes the bricking risk in
§5.5.1 (lost active key is recoverable) and the lost-active-key
compromise scenario in §10 (compromised active key can be retired from
cold storage). Pre-deployment, so we land it as a v1 in-place edit of
`ActorGenesis` rather than a v2 schema bump.

**Spec slice (committed first):**

- [x] `ActorGenesis` v1 grows `attestation_keys: list<PublicKey>`
      (non-empty, sorted, deduplicated, disjoint from `initial_key`).
      In-place edit of the existing v1 schema; existing dev-only
      `~/.kairo` data becomes invalid (different `ActorId`s) and must
      be wiped.
- [x] `ActorEmergencyKeyRotation` statement type spec
      (`schemas/canonical/actor-emergency-key-rotation-v1.md`,
      `schemas/json/actor-emergency-key-rotation-v1.schema.json`,
      `STATEMENTS.md` §4.2h).
- [x] `ActorEmergencyKeyRevocation` statement type spec
      (`schemas/canonical/actor-emergency-key-revocation-v1.md`,
      `schemas/json/actor-emergency-key-revocation-v1.schema.json`,
      `STATEMENTS.md` §4.2i).
- [x] `ActorAttestationKeyAdd` statement type spec
      (`schemas/canonical/actor-attestation-key-add-v1.md`,
      `schemas/json/actor-attestation-key-add-v1.schema.json`,
      `STATEMENTS.md` §4.2j) — append-only growth of the attestation
      set, signed by an existing attestation key.
- [x] `ACTORS.md` §5.5.2 promoted from "future failsafe" to v1
      design. §5.1 documents the two disjoint key surfaces. §6.1
      signature rule extended with surface-dispatch by statement kind.

**Impl slice:**

- [x] `kairo-statement::ActorGenesisBody` grows `attestation_keys:
      Vec<PublicKey>` with canonical encoding (sorted-dedup) and
      JSON DTO. Body validator enforces non-empty, disjoint from
      `initial_key`. Every existing test fixture and example that
      creates an actor needs at least one attestation key — this is
      the bulk of the sweep work.
- [x] `kairo-statement::{ActorEmergencyKeyRotationBody,
      ActorEmergencyKeyRevocationBody, ActorAttestationKeyAddBody}`
      with canonical encoding + JSON DTOs. New `SigningSurface`
      enum (`Operational` / `Attestation`) tags each `StatementBody`
      via const default; the three new bodies override to
      `Attestation`.
- [x] Extend `kairo-identity::ActorResolver` with
      `attestation_keys_at(actor, at) -> BTreeMap<KeyId, PublicKey>`
      returning the §5.5.2 set (map shape so the verifier gets bytes,
      not just IDs, in one resolver call). New `KeySurface` field on
      `KeyRotationEntry`/`KeyRevocationEntry`; new
      `AttestationKeyAddEntry` type. `MemoryActorResolver` tracks a
      `Vec<AttestationKeyAddEntry>` per actor with
      `insert_attestation_add`.
- [x] Extend the per-actor key-event index in `kairo-store` (added
      in §10) with `attestation_adds` plus per-entry surface markers
      on rotations / revocations. Keep all key-set state in one file
      per actor. New trait methods:
      `put_actor_emergency_key_rotation`,
      `put_actor_emergency_key_revocation`,
      `put_actor_attestation_key_add`, plus matching `get_*` and a
      `decode_attestation_adds` that drives
      `FilesystemStore::ActorResolver::attestation_key_adds`.
- [x] Update `verify_envelope_statement` with surface dispatch:
      operational kinds use the existing active-key-at-T rule;
      emergency kinds use `attestation_keys_at`. New
      `SignatureStatus::NotInAttestationSet { signature_key_id }`
      variant for surface failures.
- [x] Active-key resolver walks the unified chain (rotation +
      revocation + emergency variants) — `active_key_at` already
      walks "key-event chain leaf"; the chain just gains new
      contributing kinds.

**CLI slice:**

- [x] `kairo actor create` grows `--attestation-key <hex-pubkey>`
      (repeatable, operator-presented) and `--generate-attestation-key`
      (repeatable; clap `ArgAction::Count` so `--generate-attestation-key`
      can be passed N times). Each generate produces a fresh
      keypair, prints `seed = <base64>  pubkey = <hex>
      attestation_key_id = <id>` to stdout (the returned String, so
      `kairo actor create > out.txt` captures all seeds at once),
      and emits a stderr "RECORD THIS, IT WILL NOT BE SAVED"
      warning. At least one attestation key is required across the
      union of both flags.
- [x] `kairo actor recover-key sign --actor <id>
      --attestation-key-seed <path>` — convenience: reads a
      base64-encoded attestation seed file, generates a fresh
      active signing key, signs and persists
      `ActorEmergencyKeyRotation`, and stores the new signing
      secret in the keystore (put-then-replace handles the
      lost-keystore recovery scenario). Seed is read once and
      never persisted by Kairo.
- [x] `kairo actor recover-key prepare --actor <id> --new-key <hex>
      --output <path>` and `kairo actor recover-key import
      --prepared <path> --signature <path>` — pure two-step path
      for HSM/YubiKey operators. `prepare` emits a partially-filled
      JSON envelope plus a sibling `<output>.payload` containing
      raw canonical bytes for external signing. `import` auto-
      detects which attestation key produced the signature by
      trying each one in the actor's attestation set. The new
      active signing key's secret stays operator-managed (not
      written to the keystore) — surfaced in the success message.
- [x] `kairo actor add-attestation-key sign --actor <id>
      --signing-attestation-key-seed <path>
      (--key <hex> | --generate)` plus matching `prepare`/`import`
      subcommands. Mirrors the `recover-key` shape. Body validator
      rejects duplicates and signing-surface collisions before
      persisting.
- [x] `kairo actor key-history` (already in §10) extended to
      surface the genesis-declared attestation set, every
      `ActorAttestationKeyAdd`, and per-entry `surface` markers on
      rotations + revocations. Both text and `--json` modes.

**Why it matters:** closes the bricking hole §10 explicitly left open,
so a lost or compromised active key is no longer the end of an actor's
identity. Without this, `ACTORS.md` §5.5.1 is the only failsafe — and
"publish a new genesis and re-establish trust socially" is a poor
operator story for any real-world deployment.

**Follow-on (design locked, not yet implemented):
`ActorAttestationKeyRevocation`.** The append-only attestation set is
the largest gap in the §14 threat model — a compromised attestation
key remains authoritative forever (`THREAT_MODEL.md` §5.11, §5.12,
§6.1). The design is locked:

- **Body shape:** `{ revoked_key: KeyId, reason: Option<String> }`.
  Single-key revocation, no batch.
- **Signing surface:** attestation. Signed by any current attestation
  key, including the key being revoked itself (the legitimate
  "I think this key is compromised, burn it" gesture).
- **Non-empty-set rule:** revocation is invalid if it would leave the
  attestation set empty. Operators with only one attestation key must
  `ActorAttestationKeyAdd` first. Symmetric with the §5.9 bricking
  guard at the operational surface.
- **No `retroactive` flag.** Asymmetric with `ActorKeyRevocation` by
  design: attestation keys never sign consequential statements
  directly — they only sign emergency events that introduce or modify
  operational keys. Cleanup of damage done with a compromised
  attestation key is therefore a routine
  `ActorKeyRevocation { retroactive: true }` against the malicious
  operational key the emergency event introduced. The attestation
  revocation only stops the bleeding (no further emergency events
  from that attestation key); historical damage gets unwound at the
  operational layer where it accrued.
- **Recovery-surface symmetry remains:** any power the attestation
  surface gives the operator, it gives an attacker who holds the key.
  A compromised attestation key can also revoke legitimate
  attestation keys (subject to the non-empty-set rule). The
  `ActorAttestationKeyRevocation` primitive does not close the
  "all attestation keys compromised" scenario — that remains social
  recovery (`THREAT_MODEL.md` §7).

**Spec slice:**

- [x] `ActorAttestationKeyRevocation` statement type spec
      (`schemas/canonical/actor-attestation-key-revocation-v1.md`,
      `schemas/json/actor-attestation-key-revocation-v1.schema.json`,
      `STATEMENTS.md` §4.2k).
- [x] `ACTORS.md` §5.5.2 promoted from append-only to non-empty
      mutable set; surface dispatch line in §6.1 extended with the
      fourth emergency kind; §5.1 description of the attestation
      surface updated.

**Impl slice:**

- [x] `kairo-statement::ActorAttestationKeyRevocationBody` with
      canonical encoding + JSON DTO. `SigningSurface = Attestation`.
      Body validator checks `revoked_key` shape only; the non-empty
      and "in current set" checks live at the resolver/store layer
      (the body alone cannot know the live set state).
- [x] Extend `kairo-identity::ActorResolver`: `attestation_keys_at`
      now composes `genesis ∪ adds − revocations`. New
      `AttestationKeyRevocationEntry` type; new
      `attestation_key_revocations(actor) -> Vec<…>` resolver method.
      `MemoryActorResolver` tracks revocations alongside adds.
- [x] Extend the per-actor key-event index in `kairo-store` with
      `attestation_revocations`. New trait method
      `put_actor_attestation_key_revocation` plus matching `get_*` and
      a `decode_attestation_revocations` that drives
      `FilesystemStore::ActorResolver::attestation_key_revocations`.
      The store's put-time validator refuses to persist a revocation
      that would empty the resulting attestation set (the symmetric
      bricking guard) and reports it via `StoreError::Rejected`.
- [x] `verify_envelope_statement` requires no new dispatch (the
      fourth emergency kind reuses the existing attestation-surface
      branch). Self-revocation (signing key == revoked key) succeeds
      because the signing key is still in the set at `created_at`.

**CLI slice:**

- [ ] `kairo actor revoke-attestation-key sign --actor <id>
      --signing-attestation-key-seed <path> --revoke-key <key-id>
      [--reason <text>]` — convenience flow mirroring
      `recover-key sign`. Refuses to construct a revocation that
      would empty the attestation set; suggests
      `add-attestation-key sign` first.
- [ ] `kairo actor revoke-attestation-key prepare --actor <id>
      --revoke-key <key-id> [--reason <text>] --output <path>` and
      `kairo actor revoke-attestation-key import --prepared <path>
      --signature <path>` — pure two-step path for HSM/YubiKey
      operators, mirroring `recover-key prepare`/`import` and
      `add-attestation-key prepare`/`import`. Auto-detects the
      signing attestation key from the signature.
- [ ] `kairo actor key-history` extended to list
      `ActorAttestationKeyRevocation` entries alongside adds, with
      surface markers and the resulting attestation set after each
      event. Both text and `--json` modes.

**Follow-on (design locked, not yet implemented):
M-of-N attestation key thresholds.** Single-key compromise of any
attestation key currently lets an attacker do everything the legitimate
operator can on the recovery surface. This is the largest residual gap
in the §14 threat model; `ActorAttestationKeyRevocation` stops the
bleeding once an operator notices, but does not raise the cost of the
initial compromise. Thresholds raise that cost by requiring multiple
distinct attestation keys to authorize any emergency event — TUF roots,
DNSSEC KSK ceremonies, Bitcoin multisig, and Casa custody all use the
same construction.

The design is locked:

- **Threshold field on `ActorGenesis`:** new required `attestation_threshold: u8`,
  with `1 ≤ attestation_threshold ≤ |attestation_keys|`. No default,
  no JSON sugar — always explicit. Pre-federation we update `v1`
  schemas in place rather than minting a `v2`. Existing local genesis
  events get rebuilt with explicit `threshold = 1` (preserves today's
  behavior). The threshold participates in canonical bytes and
  therefore in `ActorId` derivation, so a rebuild produces a new
  `ActorId` for any locally-stored actor — fine pre-federation,
  unacceptable after.
- **Multi-signature envelope on attestation-surface statements.** The
  five emergency kinds (`ActorEmergencyKeyRotation`,
  `ActorEmergencyKeyRevocation`, `ActorAttestationKeyAdd`,
  `ActorAttestationKeyRevocation`, and the new
  `ActorAttestationThresholdChange`) carry
  `signatures: Vec<Signature>` instead of a single `signature`.
  Operational kinds (`ObjectRevision`, `ActorKeyRotation`, etc.) keep
  the singular `signature` envelope — the asymmetry mirrors the
  existing surface dispatch. Signatures are excluded from
  `StatementId` canonical bytes (today's rule), so the unsigned
  body bytes are unchanged.
- **Verifier rule:** at the statement's `created_at`, resolve the
  attestation set and threshold; require ≥ threshold valid
  signatures, each from a distinct `key_id` in the set. Duplicate
  `key_id`s do not count as multiple signatures — only distinct
  attestation keys contribute. Sub-threshold count fails the entire
  statement (not "verify what we have and ignore the rest").
- **Generalized non-empty rule.** The §5.5.2 set-size guard becomes
  "resulting attestation set size ≥ resulting attestation threshold."
  The current non-empty rule is the threshold = 1 special case.
  Revocations and threshold lowers that would violate this guard are
  invalid; the store rejects them with `StoreError::Rejected`.
- **New emergency type: `ActorAttestationThresholdChange`.** Body
  `{ new_threshold: u8 }`. Authority rule is asymmetric to prevent
  an attacker who has just-barely reached threshold from quietly
  consolidating control:
  - **Raises** (`new_threshold > current_threshold`) require
    `max(current_threshold, new_threshold)` distinct attestation
    signatures.
  - **Lowers** (`new_threshold < current_threshold`) require
    `current_threshold` distinct attestation signatures.
  - No-op changes (`new_threshold == current_threshold`) are valid
    but redundant.
  Validation: `1 ≤ new_threshold ≤ |attestation_set at created_at|`.
- **Co-signing flow.** Multi-sig coordination uses a derived-ID
  exchange so cosigners cannot drift on the unsigned bytes (e.g., on
  `created_at`): `prepare` writes the complete unsigned statement
  (with `created_at` locked); each cosigner signs those exact
  canonical bytes; `co-sign` appends the new signature to the
  partial envelope; `submit` verifies the envelope meets threshold
  and persists. Each signature implicitly attests to the same
  `StatementId`.
- **Staging is independent, not atomic.** Adds, revocations, and
  threshold changes are independent statements ordered by
  `created_at`. Operators wanting to go from 1-of-1 to 3-of-3 stage
  two `ActorAttestationKeyAdd` statements first, then issue an
  `ActorAttestationThresholdChange` (signed by all three new keys
  per the raise rule). There is no atomic "add and bump" envelope.
- **Resilience hygiene:** with the asymmetric authority rule, an M-of-M
  configuration plus a single lost key bricks recovery (the lower-rule
  needs `current_threshold` sigs, but only `current_threshold − 1`
  remain). Operators MUST use M-of-N with `N > M` for resilience —
  e.g., 3-of-5, not 3-of-3. The CLI surfaces this in `add-attestation-key`
  and threshold-change flows.

**Why it matters:** moves the recovery surface from "one key, full
control" (PGP single-primary, today's Kairo) to "k of n, distributed
trust" (TUF root, DNSSEC KSK with multi-party ceremonies, modern
multisig). Single-key compromise of any one attestation key no longer
lets an attacker rotate, revoke, or add anything. The
`ActorAttestationKeyRevocation` follow-on still does the cleanup work
once compromise is detected; thresholds lower the probability that
detection comes too late.

**Spec slice:**

- [x] `ActorGenesis` schema gets required `attestation_threshold` field
      (canonical + JSON). Body validator enforces
      `1 ≤ threshold ≤ |attestation_keys|`. `actor-genesis-v1.md` and
      JSON schema updated; canonical pseudocode bumped.
- [x] Statement envelope schema gains a shared `signatures` $defs
      (array of `signature`, `minItems: 1`, distinct `key_id`,
      sorted by `key_id` ascending in canonical encoding). The four
      existing attestation-surface schemas
      (`actor-emergency-key-rotation-v1`,
      `actor-emergency-key-revocation-v1`,
      `actor-attestation-key-add-v1`,
      `actor-attestation-key-revocation-v1`) switch from
      `signature` to `signatures`.
- [x] New `actor-attestation-threshold-change-v1` schema (canonical
      + JSON). Body shape, asymmetric authority rule, validation
      bounds, examples for raise / lower / no-op.
- [x] `STATEMENTS.md` §4.2h–§4.2k updated for the multi-signature
      envelope; new §4.2l added for
      `ActorAttestationThresholdChange`. The signing-surface
      summary in §4.2h notes "≥ threshold distinct signatures
      from the attestation set" rather than "one signature."
- [x] `ACTORS.md` new §5.5.3 introducing the threshold concept,
      asymmetric authority rule, generalized set-size guard, and
      operator-hygiene callout (M-of-N with N > M). §5.5.2 updated
      so the bricking guard reads "resulting set size ≥ resulting
      threshold." §6.1 surface-dispatch line names the fifth
      emergency kind.
- [x] `THREAT_MODEL.md` §5.11 / §5.12 / §6.1 updated to reflect that
      single-key compromise no longer authorizes emergency events
      when threshold > 1; added Phase-2-follow-on bullets where
      thresholds change the residual risk.

**Impl slice:**

- [x] `kairo-statement` envelope refactor: each attestation-surface
      body now flows through `MultiSignedStatement<B>` with
      `signatures: Vec<Signature>` (sorted, distinct `key_id`).
      JSON DTOs updated to `signatures` arrays.
- [x] `kairo-statement::ActorAttestationThresholdChangeBody` with
      canonical encoding + JSON DTO. `SigningSurface = Attestation`.
- [x] `kairo-identity::ActorResolver` gains
      `attestation_threshold_at(actor, T)` that composes
      `ActorGenesis.attestation_threshold` with the chain of
      `ActorAttestationThresholdChange` statements ≤ T.
      `MemoryActorResolver` tracks threshold change entries.
- [x] `kairo-store` gains an `attestation_threshold_changes` index
      slot in the per-actor key-event index file, with
      `put_actor_attestation_threshold_change` /
      `get_actor_attestation_threshold_change` trait methods.
      Put-time validator enforces the asymmetric authority rule
      (raises require `max(current, new)` distinct attestation-set
      signatures; lowers require `current`) and refuses changes
      that would push threshold above the projected set size.
- [x] New `verify_envelope_multi_statement` checks (a) each
      signature in `signatures` against an attestation-set key,
      (b) distinct `key_id`s (via the constructor invariant),
      and (c) `signatures.len() >= attestation_threshold_at(...)`.
- [ ] CLI/keystore plumbing for the multi-sig "co-sign" flow at
      the protocol layer (decoding partial envelopes, appending
      signatures, distinctness check before submit).

**CLI slice:**

- [ ] `kairo actor co-sign --prepared <path> --keystore <path>`
      appends a single signature to a partially-signed attestation-
      surface envelope. Refuses to add a duplicate `key_id`.
      Reports current `(have, need)` count.
- [ ] `kairo actor submit --prepared <path>` finalizes a partial
      envelope: verifies threshold met + distinctness + per-signature
      validity, then dispatches to the appropriate `put_*` based on
      the statement type. Refuses sub-threshold envelopes.
- [ ] `prepare` subcommands of `recover-key`, `add-attestation-key`,
      `revoke-attestation-key`, and the new `change-attestation-threshold`
      emit a partial envelope (zero signatures) plus the canonical
      bytes payload for external signing. `import` accepts a
      *single* signature for backward compatibility with the existing
      1-of-1 flow when threshold = 1.
- [ ] New `kairo actor change-attestation-threshold sign|prepare|co-sign|submit`
      verb tree mirroring the others. `sign` is the convenience flow
      for the threshold = 1 case (single-key, single-keystore); above
      threshold = 1, the operator must use `prepare` + `co-sign` +
      `submit`.
- [ ] `kairo actor key-history` extended with the threshold trajectory
      (genesis threshold + every change event) and a per-event
      `(quorum_at_event)` annotation. Both text and `--json` modes.

## After Phase 2

Once Phase 2 picks and lands a focused slice from the catalog, the natural
follow-ups depend on which slice was chosen. Sketches:

- **If Phase 2 = Git mirror + daemon + capability model:** the path to
  Phase 3 is *federation made real* — sync between two daemons, opt-in
  trust propagation, distributed snapshot resolution. Web client and
  build/run execution become tractable on top.
- **If Phase 2 = build/run execution + provider objects + polish:** the
  path to Phase 3 is *reproducible scientific workflows* — execution
  records as first-class statements, archival of run inputs/outputs/logs,
  and the `kairo reproduce` flow per `specs/CLI.md` §19.
- **If Phase 2 = web client + daemon + search:** the path to Phase 3 is
  *the public-facing federation portal* — registries, archive-mirror
  discovery, multi-tenant nodes.

Recurring themes that will keep showing up in any Phase 3+ direction:

- **Capability model load-bearing.** Several Phase 2 items (federation
  policy, cross-actor branch/tag supersedes, build executor scoping) are
  blocked by it. The longer it's deferred, the more workarounds accumulate.
- **`~/.kairo/git/` managed mirror is a federation precondition.** Bundles
  that aren't self-contained limit federation to "you also need to share
  the Git URL" — workable for collaborators on one forge, weak for
  archival.
- **Multi-process safety becomes mandatory the moment a daemon ships.**
  File locks are not a "polish later" concern once §2 lands.
- **Integration test coverage is currently inline-per-test.** Adding any
  new crate (web client, daemon, federation) without a shared fixture
  layer will compound the duplication Phase 1 already has.
- **Statement-type evolution is expensive.** Every v2 schema bump means
  re-deriving every existing `StatementId` in the wild. Phase 2 §12 is the
  best window to design v2 shapes once, with full context.

The Phase 3 plan itself will live in `specs/PHASE_3.md` once Phase 2
selection is made.
