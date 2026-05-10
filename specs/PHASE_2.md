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

1. Closing the storage story (managed Git cache; multi-process safety).
2. Standing up the long-running services that downstream surfaces depend on
   (daemon).
3. Doing the spec-first design work for the items the long-term system needs
   but the MVP could legally defer (capability model, federation protocol).
4. Hardening what's already shipped (integration tests, property tests,
   release-engineering basics, threat model).

A focused Phase 2 might pick **one closing item + one foundational service +
one spec-first design pass** rather than attempting all twelve sections.

## Catalog

### 1. `~/.kairo/git/` Managed Git Cache

**Status: closed.** Bundles can now ship Git history end-to-end:
exporter packs from the cache via `kairo bundle export --include-git`,
recipient streams `git/<object-id>.pack` into their own cache via
`kairo bundle import`, and `kairo verify object` resolves
`git:sha256:` revisions through the cache without needing a working
tree. See `DECISIONS.md` §7–§9 for the locked design and `STORE.md`
§4 / `PACKAGE.md` §6.1 for the on-disk shape.

- [x] **Layout: per-Kairo-object bare repos sharded two levels deep,
      with a shared object pool referenced via
      `objects/info/alternates`.** See `DECISIONS.md` §7. Forks
      across Kairo objects dedupe at the Git-object layer (pool
      stores each blob/tree/commit once); per-object bare repos keep
      refs, locks, and CLI scoping per-Kairo-object. Fetches land
      objects in `pool/objects/` under namespaced refs
      (`refs/kairo/<object-id>/<branch>`) and mirror the resolved
      refs into the per-object repo. Pool is treated as a
      load-bearing cache — its loss is a cache miss, not authority
      loss. Single-bare and plain per-object-bare were rejected:
      single-bare conflates lock granularity and complicates
      per-object GC; plain per-object-bare duplicates Git objects
      across forks, which matters for federation/archival.
- [x] **Fetch transport: shell out to host `git` binary, structured
      behind a `GitCacheTransport` trait.** See `DECISIONS.md` §8.
      V1 invocation: `git -c protocol.version=2 fetch --no-tags
      --no-write-fetch-head <url> <refspec>:refs/kairo/<object-id>/<branch>`.
      `git ≥ 2.x` becomes a documented runtime dep for cache
      operations (not for `verify object` against a cwd repo, which
      keeps using `gix` reads). `gix-protocol` rejected for v1 on
      hosting-compatibility, dep-weight, and API-stability grounds;
      a future swap is a localized second `GitCacheTransport` impl.
- [x] **`kairo-git` writer module.** `GitCache` in
      `crates/kairo-git/src/cache.rs` provides `open`, `path_for`,
      `ensure_repo` (per-object bare init + alternates wiring),
      `has_commit`, `fetch` (orchestrates pool fetch under pool
      lock + ref-mirror under per-object lock via the
      `GitCacheTransport` shell-out), `ingest_pack_from(impl Read)`
      (streams to `git index-pack --stdin`), `pack_for_object_to(_,
      impl Write)` (streams from `git pack-objects --stdout`), and
      `set_ref`. Streaming primitives drain stderr in worker
      threads to avoid pipe-buffer deadlock; `Vec<u8>`-returning
      wrappers exist for tests and small callers.
- [x] **`verify object` cache integration.** `kairo verify object`
      now consults the managed cache as the first-tried source for
      `git:sha256:` revisions, with cwd-discovery as fallback. New
      flags `--no-cache` (skip cache, force cwd) and `--no-cwd-repo`
      (skip cwd discovery, cache-only — the federation/daemon
      mode). Output adds a `commit lookup: ...` diagnostic line
      so the operator can tell which source served the verify.
      `--json` shape unchanged in this slice.
- [x] **Bundle integration.** Export-side
      `kairo bundle export --include-git` packs commits from the
      cache via streaming and writes `<bundle>/git/<object-id>.pack`;
      manifest's `git_history.included` flips to `true`. Import-side
      detects shipped `git/` packs, streams each into the
      recipient's cache via `ingest_pack_from`, and pins every
      `expected_commits` OID as `refs/kairo/imported/<oid>` in the
      matching per-object repo. End-to-end roundtrip verified by
      `kairo_bundle_roundtrip_with_git_packs_verifies_without_cwd_repo`:
      a recipient with no working tree reaches `VALID` against
      a federated bundle. Default-on flip remains deferred future
      work.
- [x] **CLI shape: `kairo git ...` verb tree.** See `DECISIONS.md` §9.
      Shipped: `kairo git fetch --object <id> --remote <url>
      [--branch <name>]` and `kairo git cache status`. Later:
      `kairo git cache verify` (pool integrity probe) and
      `kairo git cache gc [--object <id>]` (paired with `STORE.md`
      §12 GC work). `kairo cache ...` and folding into
      `kairo verify object --fetch` were both rejected to preserve
      naming clarity and the read-only contract of `verify`.
- [x] **Spec sweeps.** `STORE.md` §4 documents the `git/` layout
      (pool + sharded per-object repos with alternates, pool/per-
      object lock files, cache-vs-authoritative semantics).
      `PACKAGE.md` §6.1 documents the bundle git-history flow
      (export packs, import streams, ref-pinning, streaming
      end-to-end). `THREAT_MODEL.md` §5.16 covers cache tampering
      (Git's content-addressing inherits all the fixity defenses;
      pool loss is a cache miss, not authority loss).

**Why it matters (now realized):** self-contained bundles ship and
import end-to-end; `kairo verify object` no longer requires a cwd
repo when the cache or a federated bundle has the commits. Future
federation (§4) builds on these primitives without re-deriving
them.

### 2. Daemon

Long-running local service that coordinates store access, federation,
policy, scheduling, and (eventually) build/run execution.
`specs/DAEMON.md` and `specs/API.md` describe the long-term shape; the
v1 daemon is a deliberate sliver — see `DECISIONS.md` §10 for the
locked MVP scope (foreground process, Unix-socket-only, read-only
HTTP+JSON, ~11 endpoints, stateful daemon with stateless requests,
OpenAPI deferred to §5).

- [x] **Slice plan.** See `specs/PHASE_2_DAEMON.md` — eight
      sliced commits decomposing the bullets below. Each slice
      is reviewable independently.
- [x] **MVP scope locked.** See `DECISIONS.md` §10. Eight
      sub-decisions: foreground-only process model; Unix-socket
      transport (no TCP/TLS in v1); OpenAPI deferred until §5;
      ~11 read-only endpoints under `/api/v1/`; stateful daemon
      with stateless requests; CLI probe-and-fall-back dispatch
      with explicit `--daemon`/`--direct` overrides; structured-
      text logs via `tracing` (JSON + rolling files deferred);
      HTTP framework pending separate decision.
- [x] **Multi-process safety.** Done in §6 (`kairo-store` /
      `kairo-keystore` advisory locks). The precondition that
      makes a stateful daemon + concurrent CLI direct mode safe.
- [x] **Two-process architecture and HTTP framework.** See
      `DECISIONS.md` §11. Daemon and (future) web-server are
      separate processes in separate crates with distinct trust
      models — daemon is Unix-socket-only and trusted; web-server
      (Phase 2 §5) is the TCP / browser-facing demilitarized zone.
      Bridged by a small `kairo-daemon-client` Rust crate that
      `kairo-cli` (and a future Rust web-server impl) consume.
      Daemon framework is `axum` + `tokio`; blocking store calls
      go through `tokio::task::spawn_blocking`. Other frameworks
      (raw `hyper`, sync `tiny_http`-style) rejected — see §11.
- [x] Spec reconciliation pass: `DAEMON.md` §5.1 / §6.1 trimmed
      to v1 components; §13 transport list narrowed to Unix
      socket. `API.md` §3 contract-strategy section updated to
      note OpenAPI deferral and the two-process split; §9 auth
      section describes daemon (Unix-socket-perms) vs web-server
      (TCP+auth). `CLI.md` §3.3 clarified that v1 daemon serves
      reads only; write commands stay direct. Both
      `kairo daemon start|status|stop` and (deferred)
      `kairo web start|status|stop` documented.
- [x] Stand up `crates/kairo-daemon` (slices 1–2): foreground
      binary that opens `FilesystemStore` once, binds a listening
      Unix socket at `<store>/daemon.sock` (mode 0600), writes a
      PID file at `<store>/daemon.pid`, runs the axum app
      (driven by a hyper-util accept loop because axum 0.7's
      `serve` is TCP-only), and shuts down gracefully on
      `SIGTERM`/`SIGINT` with a 10s drain cap. Blocking store
      calls run on `tokio::task::spawn_blocking`. Double-start
      is refused via a connect-probe + stale-socket unlink.
- [x] Implement the read-only endpoints listed in `DECISIONS.md`
      §10.4 (slices 5–7). `GET /api/v1/blobs/{id}` streams
      response bodies through `tokio_util::io::ReaderStream`
      (64 KiB chunks) — no full materialization. The version-
      tag endpoint honors cross-actor `supersedes` via the
      capability evaluator already shipped in §3.
- [x] Stand up `crates/kairo-daemon-client` (slices 3 / 5–7):
      Rust crate wrapping HTTP-over-Unix-socket calls against
      `/api/v1/...`. Hand-rolled `hyper::client::conn::http1`
      handshake per request (no pool yet — added if it ever
      matters). Per-endpoint typed methods plus `BlobReader`
      (`AsyncRead` over the streaming blob body) and a typed
      `ClientError` enum. Used by `kairo-cli`'s dispatch in v1;
      future Rust web-server impl uses the same crate.
- [x] Add `kairo daemon start | status | stop` to the CLI
      (slice 4). `start` runs the daemon in the foreground;
      `status` probes the socket and prints PID/store/schema/
      version, exiting 9 (`daemon_unavailable`) under `--daemon`
      when absent; `stop` reads the PID and sends `SIGTERM`
      via `nix::sys::signal::kill` (workspace forbids
      `unsafe_code`), with `--wait` polling until the socket
      disappears.
- [x] Implement CLI daemon-mode dispatch (`CLI.md` §3.3) using
      `kairo-daemon-client` (slice 8). The `cli::dispatch`
      module probes the socket and returns `Mode::Daemon` /
      `Mode::Direct`; `--daemon` requires reachable, `--direct`
      / `--offline` force direct. `kairo branch show`,
      `kairo tag show`, and `kairo trust show` route through
      dispatch end-to-end; remaining read commands are
      one-line follow-ups (tracked as polish, not §2
      blockers). Write commands stay direct regardless of mode.

**Status: shipped.** All eight slices in `PHASE_2_DAEMON.md`
landed. The new crates are `crates/kairo-daemon` (lib + bin)
and `crates/kairo-daemon-client` (lib). `kairo-cli` gained
the `daemon` verb tree, the `--daemon` / `--direct` /
`--offline` global flags, and the `cli::dispatch` module.
Mutation, execution, federation, policy, GC, and OpenAPI are
explicitly out of v1 scope and gated on later phases (§4 / §5
/ §7) or their own `DECISIONS.md` follow-ups.

**Why it matters:** every downstream surface (web client,
federation-as-service, daemon-mode CLI, multi-actor workflows)
depends on the daemon existing as the local coordination layer.
§4 federation and §5 web client can now start.

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

**Depends on:** §1 (Git cache) for serving Git data, §2 (daemon) for
running federation as a service, possibly §3 (capability model) for
cross-actor authority during sync.

### 5. Web Client

`specs/WEB_CLIENT.md` describes the long-term TypeScript/React surface.
Phase 2 stands up a **read-only inspector** that drives every shipped
daemon read endpoint end-to-end. Shape is locked in `DECISIONS.md` §12;
slice plan in `specs/PHASE_2_WEB_CLIENT.md`.

Bullets, by slice (see `PHASE_2_WEB_CLIENT.md` §3):

- [x] Slice 1 — `utoipa` annotations on the daemon; `GET /api/v1/openapi.json`;
      `kairo-daemon dump-openapi`; checked-in `openapi/kairo-daemon.json`.
- [x] Slice 2 — `GET /api/v1/verify-object/:id` daemon endpoint
      returning `ValidationResult`. (12th read endpoint.)
- [x] Slice 3 — `crates/kairo-web` Rust crate: axum proxy + `ServeDir`
      for the SPA bundle, loopback-only TCP, daemon-client passthrough.
- [x] Slice 4 — CLI verbs `kairo web start | status | stop`.
- [x] Slice 5 — `frontend/` monorepo scaffold (pnpm + Turborepo + Vite);
      five package shells + `pnpm generate:api` pipeline.
- [x] Slice 6 — `api-client` + `object-model` packages; TanStack Query
      hooks for all 12 endpoints; Zod runtime envelopes.
- [x] Slice 7 — App shell, TanStack Router routes, `ui` package primitives,
      dashboard backed by `useDaemonStatus`.
- [x] Slice 8 — Object browser: genesis + branches + tags + revisions +
      capability heads + trust panel; actor and statement detail pages.
      Pulled in 4 follow-up daemon endpoints (`/version-tags/:object`,
      `/revisions/:object`, `/trust/about/:of`,
      `/capabilities/for-object/:object`) so the inspector could compose
      every panel from real reads.
- [x] Slice 9 — `validation-viewer` package; `verify-object` integration
      with status badges per `WEB_CLIENT.md` §10. `StatementGraphView` /
      `AuthorityChainView` deferred (see `PHASE_2_WEB_CLIENT.md` slice 9
      "Deferred"); the badge + issue list satisfy the slice exit criteria.
- [x] Slice 10 — `artifact-viewers` (text/JSON/binary, content-sniff
      registry); `/blobs/$id` route; Playwright e2e suite (5 critical
      workflows from `WEB_CLIENT.md` §22); GitHub Actions CI
      (`.github/workflows/ci.yml`); close-out.

**What shipped (Phase 2 §5 outcome).** A read-only inspector that
exercises every v1 daemon read endpoint end-to-end:

- **Daemon surface** (`crates/kairo-daemon`): `utoipa`-annotated
  HTTP+JSON API with 16 v1 endpoints under `/api/v1/`, an `openapi.json`
  served live + checked in at `openapi/kairo-daemon.json`, and a
  drift-detection test (`openapi_drift.rs`) that fails CI if the live
  schema and the checked-in copy diverge.
- **Daemon-side proxy** (`crates/kairo-web`): loopback-only axum +
  `ServeDir` that bridges browser → daemon Unix socket. Optional
  `--spa-dir` so the proxy can run API-only against the dev server.
- **CLI**: `kairo web start | status | stop` with PID-file lifecycle.
- **Frontend monorepo** (`frontend/`): pnpm + Turborepo, Vite + React
  19 + MUI 6, TanStack Query/Router, ky transport, MSW for browser
  mocks, Vitest for unit tests, Playwright for e2e. Five published
  packages — `@kairo/api-client`, `@kairo/object-model`, `@kairo/ui`,
  `@kairo/validation-viewer`, `@kairo/artifact-viewers` — and the
  `@kairo/web-client` app.
- **Inspector pages**: dashboard (daemon status), object detail
  (genesis + branches + tags + revisions + capability heads + trust
  opinions + validation panel), actor detail, statement detail
  (typed summary + raw envelope, marked `Unverified` per §10), blob
  preview (content-sniff-driven viewer registry — text / JSON /
  binary). Locality badges on every panel per §15.
- **Mock surface**: typed `mockRegistry` with two seeded objects
  (rich Alpha + minimal Beta), plus Gamma (invalid) and Delta
  (conflicted) for validation-state coverage; three actors
  (Alice / Bob / Carol). Unknown ids return a daemon-style 404.
- **Brand integration**: Kairo logo + favicon set, theme colors
  derived from the icon SVG (wordmark purple `#780078` for primary,
  hexagon teal `#007373` for secondary).
- **CI** (`.github/workflows/ci.yml`): three parallel jobs — Rust
  workspace tests, frontend pipeline (`typecheck/lint/test/build`),
  Playwright e2e against MSW. Concurrency cancels superseded runs.

**Deliberately deferred** (each a clean follow-up, not a v1 gap):
`StatementGraphView` / `AuthorityChainView`; per-actor statement
listings (currently full-scan; surfaces a placeholder until a
proper index lands); image / audio / video / markdown / CSV
viewers; sandboxed and runtime-required artifact viewers;
federation-preview labels. Documented in
`PHASE_2_WEB_CLIENT.md` §3 (slice-by-slice "Deferred" notes) and
§4 (deliberate gaps).

**Why it matters:** primary user-facing surface for the federation /
archival use case, and the daemon's first broad consumer — every shipped
read endpoint gets exercised end-to-end through this client.

**Depends on:** §2 (daemon). Pulls slices 1–2 of the web-client plan
forward as additional daemon work (OpenAPI annotations + verify-object
endpoint) before any TypeScript code is written.

### 6. Multi-Process Safety / File Locks

`FilesystemStore` and `FilesystemKeystore` now serialize concurrent
read-modify-write on every materialized index and per-actor key file
through per-record advisory locks. Status: **closed.**

- [x] **Locking strategy: per-record advisory locks via sidecar
      `.lock` files.** One `.lock` file per materialized-index file
      (branches, version tags, trust, capability head, capability
      reverse index, actor key-event index, plus per-actor keystore
      entries). The `.lock` file is a zero-byte sentinel; its only
      job is to be the `flock(2)` / `LockFileEx` subject. Reads don't
      take the lock — `atomic_write` + `fs::rename` already gives
      readers a consistent snapshot. Two unrelated writes (different
      actors, different objects) don't block each other. Per-shard
      and store-wide alternatives were rejected: per-shard adds
      complexity for no real gain, store-wide would serialize the
      whole daemon.
- [x] **`fs2` for cross-platform flock.** Adds `fs2 = "0.4"` to
      `kairo-store` and `kairo-keystore`. POSIX `flock(2)` and
      Windows `LockFileEx` both release on fd close, so a crashed
      writer doesn't leave a stuck lock. Helpers live in
      `kairo-store/src/lock.rs` (`with_index_lock`) and
      `kairo-keystore/src/lock.rs` (`with_key_lock`).
- [x] Concurrent put/get tests as inline unit tests in
      `kairo-store/src/lib.rs` (`concurrent_branch_writes_do_not_lose_updates`)
      and `kairo-keystore/src/lib.rs`
      (`concurrent_put_for_same_actor_admits_exactly_one`,
      `concurrent_put_for_distinct_actors_all_succeed`). They spawn
      N threads doing concurrent writes against the same store / key
      set and assert no corruption, no lost updates, and that the
      refuse-overwrite contract holds under contention. Lock-level
      serialization is also covered by `lock::tests` in each crate.
- [x] **Failure mode: bounded retry, then `LockTimeout` error.**
      `try_lock_exclusive` in a loop with 50ms sleep and a 2s
      deadline; expired contention surfaces as `StoreError::LockTimeout`
      / `KeystoreError::LockTimeout` rather than blocking forever.
      Hard error surfaces deadlocks fast and lets tests assert the
      contention path.

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
      the §1 managed cache (paired work).
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

- [x] Programmatic integration test that runs `examples/README.md`
      end-to-end (so the walkthrough doesn't bit-rot when commands change).
      `examples_readme_walkthrough_round_trip` in `kairo-cli/src/tests.rs`
      mirrors each step: actor create → object create → revision create →
      manifest inspect → branch set/show/list → tag bind/show/list →
      snapshot compute → verify object (asserts VALID) → trust grant →
      re-verify (asserts trust=trusted) → per-record import to fresh store
      → bundle export/import to another fresh store. Skips on hosts
      missing `git`.
- [x] Property tests for canonical-encoding determinism (`proptest` over
      every body type's `CanonicalEncode` impl: parse → re-encode →
      byte-equal). `kairo-statement/tests/property_tests.rs` covers all
      14 statement body types plus `Capability`; `kairo-identity/tests/
      property_tests.rs` covers `ActorGenesisBody`. Each round-trip
      test asserts `canonical_bytes(body) == canonical_bytes(body')`,
      and explicit determinism tests assert encoding the same body
      twice yields identical output.
- [x] Property tests for JSON DTO round-tripping (random body → JSON →
      back → equal). Same files as above; the round-trip macro asserts
      `body == body'` after `BodyJson::from_body → serde_json → BodyJson
      → to_body`. Catches drift between `CanonicalEncode` and the
      JSON serialization pair on every supported body type.
- [x] **Statement-type indexing — by-actor dimension landed
      alongside §5 web client.** Carry-over from Phase 1 §2 ("index
      statements by object, actor, and statement type"). The
      by-actor cut shipped first because the §5 inspector's
      `/actors/$id` page needed it: `kairo-store` now maintains a
      per-actor materialized index at
      `<store>/statements_by_actor/<XX>/<YY>/<actor-id>.json`, every
      `put_*` for a signed envelope appends to it, and
      `StatementByActorResolver::list_statements_by_actor` is wired
      through `GET /api/v1/actors/{id}/statements`
      (`StatementByActorDto`). `ObjectGenesis` is intentionally
      excluded — it carries `created_by` rather than the envelope
      `actor` field every other statement type uses; the
      owned-objects view is a separate cut. The
      `statements-of-kind-T` dimension is still deferred until §4
      federation (replicate-by-kind) needs it, matching the
      no-index-without-a-consumer rule.
- [x] **Store fixtures crate.** Carry-over from Phase 1 §2.
      `kairo-test-support` collects shared test setup: git-repo
      fixtures (`init_source_repo`, `build_pack_from`,
      `skip_if_no_git`), and `StoreFixture` for actor/object/
      revision/branch chains driven through library APIs. Used as
      a `dev-dependency` from `kairo-git`, `kairo-cli`, and
      `kairo-bundle`; replaces ~200 LOC of duplicated inline test
      helpers across those crates.
- [x] Versioning policy: workspace-uniform pre-1.0 semver per
      `VERSIONING.md`; bump `0.x.0` for any breaking change in either
      Rust API or wire format, `0.x.y` for additions and fixes. MSRV
      pinned at Rust 1.95 (`workspace.package.rust-version`); MSRV
      bumps are themselves treated as breaking. Wire-format changes
      (anything that alters canonical bytes / `*Id` derivation) get
      a distinct `Wire format` section in `CHANGELOG.md` so they're
      impossible to miss in review.
- [x] Threat model document — `specs/THREAT_MODEL.md`. Drafted as a
      consolidation of the security argument scattered across the
      spec set: assets, adversaries, defended-against attacks (with
      mechanism cross-references and residual risk), explicit
      non-goals, social recovery, operator monitoring. Surfaces the
      v1 gap that an `ActorAttestationKeyRevocation` should close
      (Phase 2 §14 follow-on).
- [x] Security review of the keystore — `specs/KEYSTORE_REVIEW.md`.
      Ten findings categorized Medium / Low / Info. The two worth
      fixing soon are §3.1 (mode-bits race window between
      atomic-rename and chmod) and §3.5 (no zeroize-on-drop for
      `SecretSigningKey`). Multi-process TOCTOU findings (§3.2,
      §3.3) are tracked under PHASE_2 §6. Plaintext-at-rest is
      explicitly out-of-MVP-scope; passphrase encryption is the
      documented post-MVP work.

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

- [x] `kairo actor revoke-attestation-key sign --actor <id>
      --signing-attestation-key-seed <path> --revoke-key <key-id>
      [--reason <text>]` — convenience flow mirroring
      `recover-key sign`. The store-layer non-empty-set guard
      refuses revocations that would empty the attestation set,
      pointing operators at `add-attestation-key sign` first.
- [x] `kairo actor revoke-attestation-key prepare --actor <id>
      --revoke-key <key-id> [--reason <text>] --output <path>` and
      `kairo actor revoke-attestation-key submit --prepared <path>
      [--signature <path>]` — pure two-step path for HSM/YubiKey
      operators, mirroring `recover-key prepare`/`submit` and
      `add-attestation-key prepare`/`submit`. Auto-detects the
      signing attestation key from `--signature`; multi-signer
      flows go through `kairo actor co-sign`.
- [x] `kairo actor key-history` extended to list
      `ActorAttestationKeyRevocation` entries alongside adds and
      threshold changes. Both text and `--json` modes.

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
- [x] CLI plumbing for the multi-sig "co-sign" flow at the
      protocol layer: `kairo actor co-sign` mutates the JSON
      `signatures` array in-place, refuses duplicate `key_id`s,
      and reports the running `(have, need)` count. Submit
      validates distinctness + threshold + per-signature validity
      before persisting.

**CLI slice:**

- [x] `kairo actor co-sign --prepared <path> --actor <id>
      --attestation-key-seed <path>` appends a single signature to
      a partially-signed attestation-surface envelope. Refuses to
      add a duplicate `key_id`. Reports current `(have, need)`
      count.
- [x] `kairo actor <verb> submit --prepared <path>` finalizes a
      partial envelope: verifies threshold met + distinctness +
      per-signature validity, then dispatches to the appropriate
      `put_*`. Refuses sub-threshold envelopes. Renamed from
      `import` for symmetry with `prepare`/`co-sign`.
- [x] `prepare` subcommands of `recover-key`, `add-attestation-key`,
      and the new `change-attestation-threshold` emit a partial
      envelope (zero signatures) plus the canonical bytes payload
      for external signing. `submit --signature <path>` accepts a
      single signature for the 1-of-1 backward-compat flow. (A
      `revoke-attestation-key` CLI is deferred to a follow-on
      slice; the store layer already supports it.)
- [x] New `kairo actor change-attestation-threshold sign|prepare|submit`
      verb tree. `sign` is the single-signer convenience flow,
      restricted to cases where the asymmetric authority rule
      needs exactly one signature (effectively no-ops at
      threshold = 1); raises and lowers must use
      `prepare` + `co-sign` + `submit`.
- [x] `kairo actor key-history` extended with the threshold
      trajectory (genesis threshold + every change event) and a
      per-event `quorum_at_event` annotation. Both text and
      `--json` modes.

## After Phase 2

Once Phase 2 picks and lands a focused slice from the catalog, the natural
follow-ups depend on which slice was chosen. Sketches:

- **If Phase 2 = Git cache + daemon + capability model:** the path to
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
- **`~/.kairo/git/` managed cache is a federation precondition.** Bundles
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
