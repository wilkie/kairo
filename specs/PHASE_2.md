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

### 3. Capability / Delegation Model (spec-first)

The recurring "future work" theme. Phase 1 deliberately deferred cross-actor
authority claims: `ObjectVersionTag`'s cross-actor `supersedes` is recorded
but not honored; `ActorTrust` cross-actor `supersedes` is invalid; multi-
maintainer flows are unsupported. A capability model would unlock all of
these by giving actors a way to grant scoped authority to other actors.

- [ ] Draft `specs/CAPABILITIES.md` with the MVP capability statement type,
      scope vocabulary, and resolution rules. The path is now free — the
      pre-Phase-1 file at that name was the runtime sandbox spec, which has
      been renamed to `specs/SANDBOX.md`. Seed prose for the new doc lives
      in `ACTORS.md` §10–12 (Actor Capabilities / Grants / Revocation).
- [ ] Decide whether capability grants are first-person (like trust) or
      object-scoped (like branches/tags).
- [ ] Spec the interaction with `ObjectBranch v2` (cross-actor supersedes
      enabled by capability grants) and with version-tag cross-actor
      `supersedes`.
- [ ] Spec how key rotation interacts with capabilities (do grants follow
      the actor across rotations, or attach to a specific key?).
- [ ] Decide spec-first vs. implement-alongside; capability semantics that
      ship wrong are very expensive to walk back.

**Why it matters:** required precondition for federation policy
(`specs/POLICY.md`), multi-maintainer workflows, and any "co-owners" UX.

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

`specs/ACTORS.md` §5.4 describes key status (active / rotated / revoked).
Phase 1 only deals with each actor's *initial* key. Real-world security
needs rotation and revocation.

- [ ] Spec the `ActorKeyRotation` statement type (signed by current key,
      names the next key).
- [ ] Spec the `ActorKeyRevocation` statement type (urgent revocation,
      possibly by an attestation rather than the key itself).
- [ ] Update `ActorResolver` to return active-keys-at-causal-position
      instead of just the initial key.
- [ ] Update verification to check the key was active at the statement's
      causal position (already noted in `specs/ACTORS.md` §6.1 as an MVP
      gap).
- [ ] Add CLI: `kairo actor rotate-key`, `kairo actor revoke-key`.

**Why it matters:** baseline security hygiene; required for any real-world
multi-year actor identity.

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

- [ ] `ObjectBranch v2` with `supersedes` chain (`specs/STATEMENTS.md`
      §4.2a "Future" subsection). Required when same-second branch
      collisions cause user-visible damage **or** when §3 capability model
      lands and cross-actor branch supersession needs a chain edge.
- [ ] `ObjectVersionTag` cross-actor `supersedes` honored by the resolver
      (today recorded but not load-bearing; flip when §3 lands).
- [ ] `ActorTrust` `forget` operation (federation concern — flush peer
      opinions from the local node without re-publishing a withdrawal).
- [ ] Schema bump migration tooling for the v1 → v2 transitions.

**Why it matters:** statement schemas are content-addressed, so v2
migrations are real work — better designed once with capability model
context than piecemeal.

**Depends on:** §3 (capability model) for the cross-actor cases.

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
- [ ] Threat model document (`specs/THREAT_MODEL.md`?). What does Kairo
      defend against (forgery, tampering, equivocation, denial-of-truth,
      key compromise)? What does it not (denial-of-service, traffic
      analysis, side-channels)?
- [ ] Security review of the keystore (mode bits, atomic write semantics,
      passphrase encryption deferred-but-documented).

**Why it matters:** the MVP works but is unhardened. Polish here makes
every later Phase 2 / Phase 3 item cheaper and safer to land.

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
