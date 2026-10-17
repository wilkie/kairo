# DECISIONS.md

## Status

Draft decision log for reconciling the current specifications.

This file records choices made where the specs previously described the same
concept differently. Individual specs should follow these decisions unless a
later dated decision explicitly replaces them.

---

## 1. CLI Name

Decision: the CLI binary is `kairo`.

Rationale: `CLI.md`, `WORKSPACE.md`, `PACKAGE.md`, and command examples use
`kairo`. The shorter `kai` spelling is not supported.

Affected specs:

- `OVERVIEW.md`
- `CLI.md`
- `PROJECT_LAYOUT.md`

---

## 2. Object Identity vs Snapshot Identity

Decision: an Object is a stable logical lineage; a Snapshot is a selected Object
state.

The following are distinct:

- `ObjectId`: stable identity of the Object lineage, derived from or bound to an
  Object genesis statement.
- `RevisionId`: content storage revision, such as a Git commit ID.
- `SnapshotId`: deterministic identity of an Object state selected by a
  statement frontier.
- `BlobId`: content-addressed identity for external bytes.

Rationale: federation, ownership transfer, forks, signed version tags, and
append-only statement history all require stable Object identity that does not
change when content changes. Content-addressed snapshots still provide immutable
state identity.

Affected specs:

- `OBJECT.md`
- `OBJECT_STORE.md`
- `CORE_LIBRARY.md`
- `IDENTIFIERS.md`

---

## 3. Git Is a Storage Backend, Not Semantic History

Decision: Git may back source revisions and deduplicate content, but Kairo
semantic history is made of signed statements.

Rationale: imported legacy repositories may preserve Git history, but Kairo must
not trust branches, tags, commits, or Git signatures as Kairo authority unless
signed Kairo statements bind them into the Object history.

Affected specs:

- `OBJECT_STORE.md`
- `WORKSPACE.md`
- `CORE_LIBRARY.md`
- `OVERVIEW.md`

---

## 4. Identifier and Reference Spelling

Decision: Kairo uses bare ID payloads in typed fields, typed references where the
field does not already provide type context, and `kairo:` references for external
interchange.

The canonical forms are:

- Bare ID payload: `<id>`
- Internal typed Object reference: `object:<id>`
- Internal typed Snapshot reference: `object:<id>:snapshot:<snapshot-id>`
- External Object reference: `kairo:object:<id>`
- External Snapshot reference: `kairo:object:<id>:snapshot:<snapshot-id>`
- Other typed references use the same pattern: `actor:<id>`,
  `statement:<id>`, `blob:<id>`, or external `kairo:actor:<id>`,
  `kairo:statement:<id>`, `kairo:blob:<id>`.

Typed manifest fields should use bare IDs:

```toml
object = "<id>"
snapshot = "<snapshot-id>"
```

Untyped CLI arguments, federation tokens, package references, logs, and
cross-system links should use typed references.

Rationale: field names such as `object`, `snapshot`, and `actor` already provide
type context, so the ID payload should not repeat the type. URI-style external
references provide clear archival and federation semantics.

Affected specs:

- `IDENTIFIERS.md`
- `API.md`
- `FEDERATION.md`
- `OBJECT_STORE.md`

---

## 5. Provider Terminology

Decision: use specific terms when possible:

- **Core provider trait**: a Rust trait that supplies Objects, Statements, Blobs,
  or Snapshots to the core library.
- **Provider Object**: an Object that declares it can provide a tool, library,
  runtime, environment, emulator, or capability.
- **Federation holder/indexer/advertiser**: a node role in federation protocols.

Rationale: multiple specs used "provider" for all three ideas. The more specific
terms reduce ambiguity without changing the model.

Affected specs:

- `GLOSSARY.md`
- `CORE_LIBRARY.md`
- `FEDERATION.md`
- `PLANNER.md`
- `STORE.md`

---

## 6. Source of Validation Truth

Decision: `CORE_LIBRARY.md` is the canonical validation/planning spec. The older
short `CORE_LIBRARY_SPEC.md` is retained only as a historical summary and must
not override `CORE_LIBRARY.md`.

Rationale: `CORE_LIBRARY.md` has the detailed validation statuses, closure
semantics, authority model, and dependent-spec requirements used by the daemon,
CLI, API, package, and web-client specs.

Affected specs:

- `CORE_LIBRARY.md`
- `CORE_LIBRARY_SPEC.md`

---

## 7. Managed Git Cache Layout

Decision: the `~/.kairo/git/` managed cache uses **per-Kairo-object bare
repositories sharded two levels deep, with a shared object pool referenced
via `objects/info/alternates`**. Layout:

```
~/.kairo/git/
  pool/                                # single shared bare repo
    objects/                           # all Git objects/packs live here
  <XX>/<YY>/<object-id>/               # per-Kairo-object bare repo
    objects/info/alternates            # → ../../../pool/objects
    refs/heads/...                     # this object's refs only
```

Fetches land objects in the pool (under namespaced refs like
`refs/kairo/<object-id>/<branch>`) and mirror the resolved refs into the
per-object repo.

Rationale: the three layouts considered were single-bare, per-object-bare,
and per-object-bare-plus-alternates-pool. Single-bare gives free
deduplication but conflates per-object lock granularity and complicates
per-object GC. Plain per-object-bare gives clean per-object blast radius
(locks, GC, CLI scoping) but duplicates Git objects across forked Kairo
objects — and Kairo forks are common-enough-to-matter (federation,
archival of derived datasets). Per-object-bare with alternates keeps the
per-object refs/locks/CLI surface and reuses Git's standard `alternates`
plumbing for cross-object dedup. Forking object A into object B is `git
init --bare` plus an alternates line plus copying refs — effectively free
on disk.

The cost is a single point of failure: deleting `pool/` breaks every
cached repo at once. We accept that and surface it as "treat the pool
as a load-bearing cache"; `kairo git cache verify` can detect breakage. The
fetch dance (objects to pool, refs to per-object) is standard Git
plumbing and is what `git clone --reference` and Forgejo/Gitea object
storage already do.

Affected specs:

- `STORE.md` — adds the `git/` slot to §4 layout, documents pool
  ownership semantics.
- `PACKAGE.md` — bundle import/export ingest into / pack from the
  cache; `BundleGitHistory.included = true` becomes meaningful.
- `PHASE_2.md` §1 — layout bullet marked closed against this decision.
- `THREAT_MODEL.md` — cache tampering detectable via Git OIDs;
  pool-loss surfaces as cache miss, not authority loss.

---

## 8. Managed Git Cache Fetch Transport

Decision: the managed cache fetches commits by **shelling out to the
host's `git` binary**, structured behind a `GitCacheTransport` trait so a
future `gix-protocol` implementation is a localized swap.

V1 invocation pattern (canonical form, normalized to bypass user
gitconfig surprises):

```text
git -c protocol.version=2 fetch --no-tags --no-write-fetch-head \
    <url> <remote-refspec>:refs/kairo/<object-id>/<remote-branch>
```

Fetches land in `~/.kairo/git/pool/objects/` (the §7 alternates pool);
resolved refs are then mirrored into the per-object bare repo.

`git ≥ 2.x` becomes a documented runtime dependency. The CLI probes
`git --version` on first cache operation and emits a clear error with
installation pointers if absent. `git` is already a test-time dependency
across `kairo-git`, `kairo-cli`, and `kairo-bundle` test suites; this
change makes it a runtime dep for the cache surface (not for `verify
object` against a cwd-discovered repo, which uses `gix` reads only).

Rationale: the two options were `gix-protocol` (enable
`blocking-network-client` + `blocking-http-transport-reqwest-rust-tls`
on the existing `gix` dep, pulling reqwest+hyper+rustls+ring through
the workspace) and shelling out to `git fetch`. Both are forced into
the same fetch model — fetch by refspec then verify reachable OIDs —
because Git's smart protocol requires server-side opt-in
(`uploadpack.allowReachableSHA1InWant`) for fetch-by-OID, which most
forges don't enable. So the differentiators are not capability but
compatibility, dep weight, and embedding cost.

Shell-out wins on:

- **Hosting compatibility.** `~/.gitconfig`, credential helpers, SSH
  agent, custom CAs, FIDO keys, ProxyCommand, GitHub LFS, GitLab
  custom transports, etc. all just work because the user's `git`
  already supports them. Replicating that surface through
  `gix-protocol` + `gix-credentials` is a long tail of bugs we'd
  find one forge at a time.
- **Dep weight.** The MVP avoids pulling reqwest+rustls+ring into
  the workspace until we actually need them.
- **API stability.** `git fetch` CLI is effectively frozen;
  `gix-protocol` is pre-1.0 and its fetch API has churned
  release-to-release.

Costs accepted, with mitigations:

- **Process management in daemon.** Standard
  `tokio::process::Command` with timeouts; SIGTERM on cancel; reap
  on drop. Well-trodden territory.
- **Output parsing.** No progress UI in v1 — stream stderr to the
  user's terminal and capture exit code. Classify coarsely: `0`
  success, `128` git-detected error, other; surface stderr verbatim
  in the error.
- **Cross-platform process semantics.** Use `std::process::Command`'s
  cross-platform abstractions; document Windows quoting/env quirks
  in `kairo-git` rustdoc as we hit them.

Future swap path: when `gix-protocol` stabilizes past 1.0 and the
auth-helper story closes (or a long-running daemon makes fork-exec
costs visible), add a second `GitCacheTransport` impl and gate
selection on a feature flag. The `GitCacheTransport` trait surface is
intentionally tiny — open URL, request refspec, stream pack into
pool, return resolved OIDs — so the swap is localized.

Affected specs:

- `PHASE_2.md` §1 — fetch transport bullet marked closed against
  this decision; writer-module bullet now references the trait
  shape.
- `STORE.md` — `git/` runtime-dep note (`git ≥ 2.x` required for
  cache operations; not required for cwd-only `verify object`).

---

## 9. Managed Git Cache CLI and Bundle Surface

Decision: managed-cache CLI commands live under the **`kairo git ...`**
verb tree, and bundles are **opt-in** to shipping Git packs via
`kairo bundle export --include-git`.

CLI shape:

```text
kairo git fetch --object <id> --remote <url> [--refspec <ref>]
kairo git cache status
kairo git cache verify          # pool integrity probe (later)
kairo git cache gc [--object <id>]   # paired with STORE.md §12 (later)
```

Bundle export:

```text
kairo bundle export <object-id> --output <path>
                                   # BundleGitHistory.included = false
kairo bundle export <object-id> --output <path> --include-git
                                   # BundleGitHistory.included = true,
                                   # ships git/<object-id>.pack from
                                   # the §7 cache pool
```

Bundle import side always ingests a shipped `git/` subdirectory into
the managed cache — there is no `--ignore-git` flag; if the bundle
ships pack data, it lands in the pool.

Rationale:

- **`kairo git ...` matches the surface naming.** The verb tree is for
  managing the local Git cache that Kairo owns. `kairo cache ...`
  was considered but loses information (cache of *what*?).
  Folding fetch into `kairo verify object --fetch` was considered
  but conflates "verify" (read-only, idempotent) with "fetch"
  (network I/O, mutates the cache); keeping them as distinct verbs
  preserves the read-only contract of `verify`.
- **Opt-in `--include-git` for bundle export.** Default-off keeps
  bundle exports cheap and predictable: today's exports stay the
  same shape and size; users opt in only when they need
  self-contained federation. Default-on later — once the import side
  is ubiquitous and bundle consumers can rely on Git data being
  present — is a separate decision recorded as a future flip in
  `PACKAGE.md`. Default-on now would silently bloat every export
  the day the flag lands.
- **Asymmetric import.** Bundle import always ingests git data when
  present because (a) ingest is cheap (a copy + `git index-pack`
  into the pool that already exists), (b) ignoring shipped git data
  would create the bug where the same bundle yields a different
  cache state on different recipients, and (c) a recipient who
  truly doesn't want git data should not be importing the bundle in
  the first place.

Affected specs:

- `PHASE_2.md` §1 — CLI bullet marked closed against this decision;
  bundle-flip bullet now describes opt-in flag, default-on as
  future flip.
- `PACKAGE.md` — bundle export grows `--include-git` flag;
  `BundleGitHistory.included = true` becomes meaningful;
  default-flip path documented as future work.
- `CLI.md` — `kairo git ...` verb tree documented.

---

## 10. Daemon MVP Shape

Decision: the Phase 2 §2 daemon ships as a **deliberate sliver** of
the long-term `DAEMON.md` shape — a stateful, foreground-only,
Unix-socket-bound, read-only HTTP+JSON facade in front of
`FilesystemStore`. Federation, executors, policy, and tasks are
post-MVP and left as later phases.

Eight sub-decisions, all locked together because they're tightly
coupled MVP scope:

1. **Process model: foreground only.** `kairo daemon start` blocks
   in the foreground; users supervise it with systemd, launchd,
   tmux, `nohup`, or a plain `&`. No double-fork. Shutdown on
   `SIGTERM`/`SIGINT`. Adds zero supervision complexity to our
   code; modern process managers are the right place for that
   responsibility.

2. **Transport: Unix domain socket only.** Socket at
   `<store>/daemon.sock`, owned by user, mode `0600`. Filesystem
   permissions are the only authn/authz surface in v1 — no TCP, no
   TLS, no bearer tokens. Network exposure is a §5 web-client
   concern.

3. **OpenAPI: deferred to §5 web client work.** Likely needed
   eventually, but the v1 daemon's only client is `kairo-cli` and
   both sides are Rust. Hand-coded handlers for v1; pick `utoipa`
   (or alternative) when the web-client contract crystallizes
   the external shape.

4. **API surface: ~11 read-only endpoints under `/api/v1/`.**
   `GET /version`, `GET /status`, `GET /actors/{id}`,
   `GET /objects/{id}`, `GET /statements/{id}`,
   `GET /branches/{object}`, `GET /branches/{object}/{name}/latest`,
   `GET /version-tags/{object}/{version}`,
   `GET /trust/{by}/{of}`, `GET /capabilities/{grantor}`,
   `GET /blobs/{id}` (streaming). No write paths in v1; CLI
   write commands continue running direct against the store.

5. **State model: stateful daemon, stateless requests.** The
   daemon process holds a single `Arc<FilesystemStore>` opened at
   startup and a listening socket. Each request is self-contained
   — no per-client session state, no cursors, no long-running
   task references. Statelessness at the request level keeps
   crash recovery trivial (restart re-opens the store and re-binds
   the socket); statefulness at the process level avoids the
   cost of re-opening the store per request.

6. **CLI dispatch: probe-and-fall-back.** Default behavior:
   probe `<store>/daemon.sock` existence. If present and
   `GET /api/v1/status` responds, route reads through the daemon.
   Otherwise direct. Explicit overrides: `--daemon` requires
   daemon mode (errors if unreachable); `--direct` / `--offline`
   forces direct. Write commands stay direct regardless of mode.

7. **Logging: stderr, human-readable, structured-text for v1.**
   Format: `time level component msg`. JSON log format and
   configurable rolling log files are known future work — the
   shape (typed events through `tracing`) leaves room to swap
   in a JSON subscriber later without touching call sites.

8. **HTTP framework: axum + tokio.** See §11 for the two-process
   architecture and full rationale. Daemon handlers are async;
   blocking `FilesystemStore` calls are wrapped in
   `tokio::task::spawn_blocking`.

Rationale: every sub-decision narrows scope toward "this is the
foundation §4/§5/§7 build on." The §6 multi-process safety work
is the precondition that makes a stateful daemon + concurrent CLI
direct mode safe; without it, this whole shape would race.

Affected specs:

- `PHASE_2.md` §2 — bullets reframed against this decision.
- `DAEMON.md` — §5.1/§6.1 reconciled to v1 (post-MVP components
  marked deferred); §13 transport list narrowed to Unix socket.
- `API.md` — §3 contract-strategy section updated to note
  OpenAPI deferral; §10–§22 endpoint groups tagged for v1
  inclusion vs deferral.
- `CLI.md` §3.3 — clarified that v1 daemon serves reads only;
  write commands stay direct regardless.

---

## 11. Two-Process Architecture: `kairo-daemon` and `kairo-web`

Decision: the daemon and the web-server are **separate processes
in separate crates with distinct trust models**, bridged by a
small `kairo-daemon-client` Rust crate that speaks HTTP over the
daemon's Unix socket. The daemon framework is **`axum` + `tokio`**.

### Layout

```text
kairo-daemon process                 kairo-web process
  ├── crates/kairo-daemon              ├── crates/kairo-web (Phase 5)
  ├── Unix socket only                 ├── TCP (browser-facing)
  ├── /api/v1/... (axum + tokio)       ├── auth, CORS, static SPA
  └── trusted: filesystem perms        └── translates → daemon-client
        ↑                                    │ HTTP-over-Unix-socket
        │                                    │
        └────── crates/kairo-daemon-client ──┘
                  (consumed by kairo-cli too)
```

### Trust model

The daemon's trust contract is "you connected via the Unix socket,
you're trusted." Filesystem permissions on `<store>/daemon.sock`
(mode 0600) are the only authn/authz. Anything reaching the daemon
has been vetted; it never sees a browser request directly.

The web-server is the **public trust boundary**. It terminates TCP
(and TLS, eventually), serves the SPA bundle, handles cookies /
bearer tokens / OAuth, applies CORS and rate limits, validates
browser-shaped input, and translates approved requests into
daemon-client calls. A bug in the web-server's untrusted-input
handling cannot compromise the daemon's address space.

This is the standard pattern for systems that pair a privileged
daemon with a public web UI: PostgreSQL + a web frontend (PostgREST
or app server), `systemd-journald` + `systemd-journal-gatewayd`,
many database-admin tools. The privilege separation is genuine —
the web-server can run as a different user, in a sandbox, in a
container, with no filesystem access except its static-asset
directory and the daemon socket.

### Crate split

- **`crates/kairo-daemon`** (Phase 2 §2): the daemon binary.
  Owns the store, listens on Unix socket, implements the §10.4
  read endpoints. Async via tokio; HTTP via axum.
- **`crates/kairo-daemon-client`** (Phase 2 §2): tiny Rust crate
  that wraps HTTP-over-Unix-socket calls. Used by `kairo-cli`'s
  daemon-mode dispatch. The same crate becomes the path Rust
  callers (including a Rust web-server impl, if §5 picks Rust)
  use to reach the daemon.
- **`crates/kairo-web`** (Phase 2 §5, deferred): the web-server
  binary. Framework choice is **separate and deferred** until
  §5 — could be axum (sharing types with the daemon's responses),
  could be Node, could be anything. Doesn't affect §2.

### Daemon framework: axum + tokio

`axum` + `tokio` is the daemon's HTTP/runtime stack. Rejected
alternatives:

- **Raw `hyper`** without a framework: still pulls in tokio (hyper
  requires it), so we don't avoid the runtime; we just hand-roll
  routing/middleware. Costs us code, gains nothing.
- **Sync mini-frameworks** (`tiny_http`, `rouille`): tiny dep
  footprint and matches the rest of the workspace's sync style,
  but it's an ecosystem dead-end. Federation (§4) needs async
  HTTP clients to peers; executor supervision (§7) wants async
  process management; streaming responses want `AsyncRead`/
  `AsyncWrite`. Adopting tokio later means rewriting handlers.

Async/sync split is contained: only `kairo-daemon` and
`kairo-daemon-client` are async. `kairo-cli`, `kairo-store`,
`kairo-keystore`, etc. stay sync. Blocking store calls inside the
daemon are wrapped in `tokio::task::spawn_blocking` so they don't
stall the runtime.

### CLI surface

```text
kairo daemon start | status | stop      # Phase 2 §2
kairo web    start | status | stop      # Phase 2 §5 (deferred)
```

Symmetric verb shape; obvious which binary each manages. Each can
be invoked in the foreground; supervisors (systemd, launchd, tmux,
`nohup`, `&`) handle backgrounding. Future convenience verb like
`kairo daemon start --serve-web` could in-process both for casual
deployments — explicitly out of scope for v1.

### Rationale

- **Trust isolation.** Daemon never sees untrusted input. Security
  review reduces to two narrower problems instead of one bigger
  one. The daemon owns signing keys; the network-facing process
  doesn't.
- **Performance is fine.** Per-request overhead of the extra hop
  is ~10–100µs over the Unix socket; streaming throughput cost is
  ~20–30% but absolute speed is bounded by disk/network. Browser-
  driven traffic is interactive, so invisible. Federation,
  executors, and CLI all bypass the web-server entirely.
- **Tech-stack independence for the web-server.** The web-server
  doesn't have to be Rust. If §5 chooses Node (for SPA toolchain
  affinity), the daemon stays Rust. The HTTP+JSON contract is the
  only boundary.
- **Async adoption is contained.** Tokio in the daemon now buys
  us federation, executors, and streaming for free; rest of
  workspace stays sync.

### Reservations accepted

- Two binaries to manage. Mitigated by symmetric `start|status|stop`
  verbs and future supervisor-friendly conveniences.
- API contract drift between daemon and web-server. Mitigated by
  one OpenAPI doc describing both surfaces (when §5 lands); the
  web-server is mostly a passthrough.
- No "single binary, just run it" deployment story for v1. Could
  be added later as `kairo daemon start --serve-web` without
  changing the architecture.

Affected specs:

- `PHASE_2.md` §2 — bullets reframed; new bullets for
  `kairo-daemon-client`, `kairo-web` deferral.
- `PHASE_2.md` §5 — web-client section will reference this
  decision when scope-locked.
- `DAEMON.md` — §5.7 ApiService text aligned with Unix-socket-only
  + axum.
- `API.md` — §3 contract-strategy notes the two-process model;
  §9 authentication describes the daemon (Unix-socket-perms) vs
  web-server (TCP+auth) split.
- `CLI.md` — `kairo daemon` and `kairo web` verb trees both
  documented.

---

## 12. Web Client v1 Shape

Decision: the Phase 2 §5 web client ships as a **read-only
inspector** that exercises the daemon's existing read endpoints
end-to-end. Like §10's daemon sliver, it is a deliberately narrow
slice of the long-term `WEB_CLIENT.md` shape — no build, run,
runtime sessions, federation, policy, tasks, or write paths in v1.
Every visitor is treated as an **anonymous public viewer**; real
actor auth is post-v1.

Nine sub-decisions, locked together because they're tightly
coupled MVP scope:

1. **Surface: read-only inspector over the 11 daemon endpoints.**
   v1 covers object/genesis/branch/tag/trust/capability views and
   raw blob preview. No build/run/policy/federation/tasks UI; each
   waits for its own daemon endpoint group to ship. The web client
   is the daemon's broad consumer — its job is to drive every
   shipped read endpoint into a user-visible surface.

2. **Auth: anonymous public viewer, no login in v1.** Browser
   visitors are treated as a non-actor "public" persona that may
   perform any read-only action. No login UI, no signing keys in
   the browser, no token storage. Deferred to post-v1: a bearer-
   token capability flow tied to actor signing keys (see the
   long-term `project_daemon_auth_direction` direction — the v1
   anonymous path leaves that ground untouched).

3. **Network surface: localhost-only TCP.** `kairo-web` binds
   `127.0.0.1:<port>` (default port chosen at slice time). No
   remote exposure in v1 — the trust model is "you can reach the
   loopback port = you are the local user," analogous to §10's
   "you can reach the Unix socket." Multi-user-machine hardening
   and TLS land with the bearer-token work.

4. **Tech stack: pnpm + Turborepo + Vite + React + TypeScript,
   TanStack Router + TanStack Query.** Per `WEB_CLIENT.md` §3.
   No deviation; the prescribed stack is well-suited to the
   inspector shape and lines up with the OpenAPI-driven type
   pipeline.

5. **Repo layout: full pnpm/Turborepo monorepo at `frontend/`.**
   `frontend/apps/web-client` plus the five prescribed packages
   (`api-client`, `ui`, `object-model`, `validation-viewer`,
   `artifact-viewers`) per `WEB_CLIENT.md` §4. Packages start as
   thin shells and grow as feature slices land; the layout is
   chosen now so cross-package imports never need to be retrofitted.

6. **API contract: OpenAPI now, generated TypeScript types.**
   Closes the `§10.3` deferral. `utoipa` annotations on daemon
   handlers + DTOs; daemon serves `GET /api/v1/openapi.json` and
   the schema is checked into the repo at
   `openapi/kairo-daemon.json`. Frontend generates TS types via
   `openapi-typescript`. `Zod` schemas validate API envelopes
   and errors at runtime per `WEB_CLIENT.md` §5.2. Generator
   choice: **`utoipa`** — most popular axum-compatible OpenAPI
   crate, derive macros on existing handlers and DTOs, minimal
   blast radius on the §2 daemon code.

7. **`kairo-web` Rust process: axum + tokio, daemon-client over
   Unix socket.** Mirrors the §10/§11 daemon stack. Holds a
   long-lived `kairo_daemon_client::Client`. Browser HTTP
   requests are translated to daemon-client calls; the daemon
   socket is never visible to the browser. Foreground-only,
   `kairo web start | status | stop` verbs symmetric with
   `kairo daemon`.

8. **SPA bundle served from disk.** `kairo-web --spa-dir <path>`
   serves the built Vite bundle from a configurable directory.
   `tower_http::services::ServeDir` for the static surface.
   No `include_bytes!` embedding in v1 — the binary stays small
   and bundles can be iterated on without rebuilding `kairo-web`.
   Embedded-bundle deploy mode is a clean follow-up if it's ever
   needed.

9. **New daemon endpoint: `GET /api/v1/verify-object/:id`.**
   Lands as a daemon slice that precedes the web client itself.
   Wraps the existing `kairo-core` verify pipeline, returns a
   `ValidationResult` DTO synchronously (no task envelope), so
   the inspector can show real validation status instead of
   "unverified" placeholders. Brings the daemon to **12 read
   endpoints** under `/api/v1/`. Async/streaming validation is
   post-v1.

Rationale: the v1 web client's job is to prove the daemon's
read API is good — not to ship the long-term web surface. Every
sub-decision narrows scope toward that proof. The OpenAPI work is
deliberately pulled forward of the TS code (§6) because typed
contracts are far cheaper to introduce before the consumer exists
than after. The verify endpoint is pulled in as a single extra
daemon slice (§9) because shipping an inspector that lies about
validation would set the wrong tone for every later consumer.

Affected specs:

- `PHASE_2.md` §5 — bullets reframed against this decision.
- `WEB_CLIENT.md` — Phase 1 vs long-term scope tagged inline; the
  five packages stay; deferred features (build/run/policy/
  federation/tasks/runtime) marked post-v1.
- `API.md` — §3 contract-strategy and §9 authentication updated;
  new §-block for `/verify-object/:id` if it isn't already.
- `DAEMON.md` — note the `verify-object` addition; web-server
  trust model carries forward from §11.
- `CLI.md` §3 / §10 — `kairo web` verbs documented in detail.

---

## 13. Daemon Write-Path MVP Shape

Decision: the daemon's write-path lands in **two phases**. Phase A
adds write endpoints over the existing Unix socket under the
**filesystem-perms-only trust model** that v1 reads already use.
Phase B adds the locked-direction **actor signing-key request auth
+ capability statements** (`DAEMON.md` §18.1, `API.md` §9.4) and
becomes the prerequisite for any TCP/browser exposure of writes.
Phase A is the focus of this decision; Phase B's wire format is
left to its own follow-up decision when that work begins.

Eight sub-decisions for Phase A, locked together:

1. **Two-phase split.** Phase A is "writes over the Unix socket
   with the v1 read trust model" — daemon trusts every
   `connect(2)` caller, same as today. Phase B layers signed-
   request auth and capability enforcement on top, and is what
   unblocks the `kairo-web` POST-pass-through. No write endpoint
   becomes reachable from outside the daemon process's socket
   permissions during Phase A.

2. **Endpoint shape: single generic `POST /api/v1/statements`.**
   The request body is a `SignedStatement<*>` JSON envelope — the
   same polymorphic shape statements carry on disk. The daemon
   dispatches on `body.type` to the matching `kairo-store::Store`
   `put_*` method (`put_actor_trust`, `put_object_revision`,
   `put_object_branch`, `put_object_version_tag`,
   `put_object_genesis`, plus the two capability statement types).
   Per-kind endpoints (`POST /api/v1/trust`, `POST
   /api/v1/branches/...`) were rejected: writes are uniform at the
   data-model layer (every write is "persist a signed statement"),
   and a generic surface absorbs new statement kinds without new
   routes.

3. **Signing stays client-side.** The daemon never holds private
   keys. `kairo-cli` (and any future Rust web-server impl) reads
   the local `kairo-keystore`, builds the `UnsignedStatement`,
   signs the canonical bytes, and ships the resulting
   `SignedStatement<*>` to the daemon. The daemon re-verifies the
   signature on receipt against the actor's pinned attestation
   key — the same path `kairo-statement::verify` uses — and
   refuses the write if verification fails. Keeps the daemon
   stateless w.r.t. key material; mirrors how federation peers
   will sign in Phase B. *(Superseded for session-cookie writes
   by §15 sub-decision 1; still holds for signed-envelope
   writes.)*

4. **Validation: re-run the data-model invariants the store
   already enforces.** Phase A's write handler runs the same
   verification the direct-mode CLI runs: signature validity,
   body shape, supersedes/parent invariants, head-line
   consistency. No new mutation policy in Phase A — the daemon
   gates writes on data-model correctness only. Cross-actor
   capability enforcement on writes (the authorization half) is
   Phase B work because authn must land first.

5. **`kairo-web` POST-block during Phase A.** `kairo-web`'s
   `/api/v1/*` proxy explicitly rejects `POST`, `PUT`, `PATCH`,
   and `DELETE` with `405 Method Not Allowed` for the duration of
   Phase A. Lifts in Phase B when the signed-request envelope is
   in place. Without this guard, a browser-side XSS or DNS-rebind
   could reach the daemon's write surface through the loopback
   TCP proxy. *(Lifted for an exact route allowlist by §15.)*

6. **CLI dispatch: writes follow the same probe-and-fall-back
   rules as reads.** Once a write endpoint exists for a given
   command, that command's default behavior becomes "probe the
   socket; route through the daemon if reachable, else direct."
   `--daemon` requires the daemon; `--direct` / `--offline` force
   direct. The v1 special case ("writes always direct") goes
   away one command at a time as slices land — commands without
   daemon-side support continue to ignore the socket.

7. **Wire format for Phase B is forward-pointer only.** The
   request-signing envelope (header names, hash algorithm, nonce
   semantics, timestamp window) is locked in a separate decision
   when Phase B begins. Phase A code must not pre-commit to a
   specific header layout, route prefix, or middleware shape that
   would constrain Phase B's design — the simplest way to preserve
   that freedom is to put zero auth concerns in Phase A's
   handler. Filesystem-perms is the only trust surface.

8. **Out of scope for Phase A.** Mutation policy (`DAEMON.md`
   §12), task framework, federation push/pull, GC, attestation-
   key rotation flows, batch writes, transactional multi-statement
   submission. Each is gated on later phases or its own follow-up.

Rationale: this is the same pattern §10 used for daemon reads —
narrow the v1 sliver, then layer trust on after the data-plane
sliver is proven. The Unix socket already inherits the filesystem-
perms trust boundary; writes-over-trusted-socket have exactly the
same trust model as today's reads. The two-phase split lets the
signed-envelope decision crystallize against working endpoints
instead of in the abstract. The §11 two-process split protects us
on the public-facing side: the `kairo-web` POST-block makes the
"writes have no auth yet" window invisible to anything reachable
from a browser.

Affected specs:

- `PHASE_2.md` — new §15 (this work) added after the existing
  §14; §2 cross-links to §15 in its "deferred mutation" note.
- `PHASE_2_DAEMON_WRITES.md` — new slice-plan doc paralleling
  `PHASE_2_DAEMON.md` (`DECISIONS.md` §13 is the locked shape;
  the slice plan is the build decomposition).
- `API.md` — §10's endpoint-group table flips `/objects`,
  `/branches`, `/version-tags`, `/trust`, `/capabilities`,
  `/statements` to "write paths land in Phase A" with a §15
  cross-reference; §9.3 clarifies that Phase A writes have no
  per-request auth.
- `DAEMON.md` — §13's "[v1] read-only" tag becomes
  "[v1 reads + Phase A writes]"; §18 retains its post-v1
  signed-request direction unchanged.
- `CLI.md` §3.3 — drop the "writes always direct" line; replace
  with "writes follow probe-and-fall-back once a daemon endpoint
  exists for the command."
- `WEB_CLIENT.md` — the read-only-inspector framing stays; the
  POST-block in `kairo-web` is documented as a Phase A invariant.

---

## 14. Daemon Write-Path Phase B Auth Shape

Decision: Phase B of the daemon write-path (`DAEMON.md §18.1`,
`API.md §9.4`) layers two mechanisms on top of Phase A's
existing `POST /api/v1/statements` surface:

1. **Authentication** — every write request carries a signed
   envelope; the daemon resolves the actor and verifies the
   signature against the actor's active signing key, the same
   identity primitive `kairo-statement::verify` already uses.
2. **Authorization** — for kinds whose semantics require
   cross-actor authority (`ObjectRevision`, `ObjectBranch`,
   `ObjectVersionTag`, `ActorCapabilityGrant`), the daemon runs
   `evaluate_capability` at request time to confirm the signer
   either is the object's root authority or holds a covering
   grant. Kinds whose authority is inherent in the signer
   identity (`ActorTrust`, `ObjectGenesis`,
   `ActorCapabilityRevocation`) skip the capability lookup but
   still enforce their built-in identity invariants.

Phase A's filesystem-perms-only trust model goes away on the
write surface once Phase B ships. The read surface stays
unauthenticated — its trust model is unchanged (loopback Unix
socket + filesystem permissions).

Eight sub-decisions for Phase B, locked together:

1. **Two-mechanism split (authn + authz).** Authentication is
   ed25519-signed-request envelope verification; authorization
   is capability-statement lookup. Both run server-side in the
   write dispatcher *before* the existing `verify_envelope_statement`
   + `put_*` pipeline. Failure on either layer returns a typed
   error envelope (`401 unauthorized` for authn, `403 forbidden`
   for authz) — the write never reaches the store.

2. **Wire format: `Authorization: KairoActor1` header with
   structured params.** The CLI emits:

   ```http
   Authorization: KairoActor1 actor=<actor-id>, key_id=<key-id>,
     timestamp=<i64>, nonce=<32-hex>, signature=<base64>
   ```

   `actor` and `key_id` are wire-stable identifiers; `timestamp`
   is i64 epoch seconds (matches `ActorTrust.created_at` and the
   rest of the protocol's binary canonical form); `nonce` is 16
   random bytes hex-encoded; `signature` is the raw 64-byte
   ed25519 signature, base64-encoded. The scheme name carries a
   version tag (`KairoActor1`) so the wire format can evolve
   without colliding with the post-Phase B successor.

   Per-field separate headers (`X-Kairo-Actor`, etc.) and
   single-base64-envelope alternatives were rejected: the
   `Authorization` form is industry-standard and works with off-
   the-shelf HTTP debugging tools; the structured-params layout
   is straightforward to parse and human-readable.

3. **Signing string: domain-prefixed canonical concatenation.**
   The client constructs:

   ```text
   kairo-daemon-write-v1\n
   <method>\n
   <path-and-query>\n
   <body-hash-hex>\n
   <timestamp>\n
   <nonce>\n
   ```

   The domain prefix (`kairo-daemon-write-v1`) prevents a
   signature captured from this protocol from being replayed in
   any future Kairo signing context. The trailing newline on
   every field eliminates concatenation ambiguity. The
   `path-and-query` is the request's `:path` exactly as it
   appears on the wire (no normalization), so percent-encoding
   and query-string ordering are pinned client-side.

4. **Body hash: SHA-256 of the raw request body, hex-encoded.**
   Always present in the signing string, including for empty
   bodies (the SHA-256 of zero bytes — a fixed constant). No
   per-method special-casing. Hex output matches the
   canonical-bytes "hash of N bytes" rendering used elsewhere
   in the protocol.

5. **Replay defense: timestamp-only window of ±60 seconds.**
   The daemon rejects requests whose `timestamp` is more than
   60 seconds away from `now()`. No server-side nonce cache in
   Phase B — the nonce is in the signed input so a stateful
   replay cache can be added later without a wire-format
   change. Phase B's threat model is the same loopback Unix
   socket the v1 reads already trust; a 60s capture-and-replay
   window is acceptable. A stateful cache is forward-compatible
   future work.

6. **Authorization scope.** Per-kind enforcement, summarized:

   | Kind                          | Server-side check                                                         |
   |-------------------------------|---------------------------------------------------------------------------|
   | `ActorTrust`                  | signer == `body.actor` (trust is first-person)                            |
   | `ObjectGenesis`               | signer == `body.created_by` (genesis = self-attesting authority)          |
   | `ObjectRevision`              | `evaluate_capability(grantee=signer, scope=Object(body.object), kind=ObjectRevision, at=now)` returns `Held`, *or* signer is the object's `created_by` root authority |
   | `ObjectBranch`                | as `ObjectRevision`, with `kind=ObjectBranch`                             |
   | `ObjectVersionTag`            | as `ObjectRevision`, with `kind=ObjectVersionTag`                         |
   | `ActorCapabilityGrant`        | recursive: signer (the grantor) must hold the granted capability *with* `delegable=true`, *or* be the object's root authority |
   | `ActorCapabilityRevocation`   | signer == grantor of the original grant (already enforced by the direct-mode CLI; the daemon mirrors it) |

   Root-authority match short-circuits the capability lookup —
   the same-actor-write case is the common path and shouldn't
   need to walk the chain.

7. **`kairo-web` POST-block stays.** Phase B does not lift the
   proxy's method filter. The web inspector is anonymous-public-
   viewer per `DECISIONS.md §12` sub-decision 2 — there is no
   UI for browser writes, and designing web-server identity
   (operator-configured actor key, delegated capability,
   forwarded user signatures) is its own design-and-slice unit.
   Future "web inspector login + browser writes" lifts the
   POST-block as part of that work, not as a side effect of
   Phase B. This decision keeps Phase B's blast radius to one
   crate boundary (the CLI ↔ daemon channel). *(That work is
   §15.)*

8. **Out of scope for Phase B.** Web auth (per (7)); federation
   peer auth (Phase 2 §4 work; uses the same actor-signing
   primitive but the framing is federation-shaped); mutual TLS;
   per-actor rate limiting; nonce cache (deferred future work
   per (5)); attestation-surface request signing (the four
   emergency body kinds stay direct-only in Phase A and Phase
   B — operator-presented keys per `ACTORS.md §5.5.2`); browser-
   side keystore; signing-key delegation to short-lived session
   tokens.

Rationale: same shape as §13 — narrow Phase B to "make the write
surface auth-protected" without entangling other identity surfaces.
Sub-decisions (2)–(5) lock the wire format precisely so the slice
plan can build against concrete bytes; sub-decisions (6)–(8)
fence the work so Phase B doesn't grow tendrils into web auth,
federation, or attestation flows. Forward-compatibility comes
from the `KairoActor1` scheme tag, the in-signing-string nonce
(for future stateful caches), and the per-kind enforcement
matrix (new kinds slot in without changing the wire format).

Affected specs:

- `PHASE_2.md` §15 — Phase B sub-section added under the
  existing Phase A bullets.
- `PHASE_2_DAEMON_AUTH.md` — new slice-plan doc paralleling
  `PHASE_2_DAEMON_WRITES.md`.
- `API.md` — §9.4 ("Post-v1 authentication direction") flips
  from forward-pointer to current-implementation; §10's
  endpoint-group table notes that `/statements` POST requires
  the `Authorization: KairoActor1` envelope.
- `DAEMON.md` §18.1 — the "[post-v1]" tag on signed-request
  auth becomes "[Phase B]", with the wire format pinned to §14.
- `CLI.md` §3.3 — daemon-mode write commands document that they
  build the signed envelope from the local keystore; nothing
  about the user-facing command shape changes.
- `WEB_CLIENT.md` — the POST-block remains; cross-link to the
  future "web inspector login" decision.

---

## 15. Browser Write Path via `kairo-web`

Decision: the web inspector writes statements through the daemon
using **daemon-side session signing**, and `kairo-web` lifts its
POST-block for an **exact route allowlist** guarded by a
**loopback-`Host` check** and a **same-origin `Origin` check**.
This is the "web inspector login + browser writes" decision that
§13 sub-decision 5 and §14 sub-decision 7 deferred to. Session
machinery itself (in-memory daemon sessions, `HttpOnly;
SameSite=Strict` cookie, actor picker) is specified in
`PHASE_2_WEB_CLIENT_AUTH.md`; the slice plan for this decision is
`PHASE_2_WEB_WRITES.md`.

Seven sub-decisions, locked together:

1. **Daemon signs for session-mode writes.** `POST
   /api/v1/statements` accepts two payload shapes
   (`PHASE_2_WEB_CLIENT_AUTH.md` §2.3): a client-signed envelope
   under `Authorization: KairoActor1` (Phase B, unchanged), or an
   unsigned `{ body_kind, body }` under a `kairo_session` cookie.
   In cookie mode the daemon loads the session actor's signing
   key from the local keystore, stamps `created_at`, signs, and
   dispatches through the same verification, capability check,
   and `put_*` path as signed mode. Both credentials on one
   request is `400 ambiguous_auth`; neither is `401`. This
   **replaces §13 sub-decision 3 ("the daemon never holds private
   keys") for cookie mode only**: the daemon reads the same
   `~/.kairo/keys/` the CLI on that host already reads, so the
   set of principals that can sign as a local actor does not
   grow. Signed mode keeps sub-decision 3 as written.
   `ObjectGenesis` has no envelope (its signature covers the body),
   so in cookie mode the client sends only `{ object_kind,
   initial_revision? }` and the daemon fills `created_by` (the
   session actor), `created_at`, and a fresh 32-byte nonce before
   signing the body. The four attestation-surface kinds stay
   CLI-only (§14 sub-decision 8).

2. **Proxy write allowlist is exact `(method, path)` pairs.**
   `kairo-web` forwards `GET` and `HEAD` on every `/api/v1/*`
   path, plus exactly:

   | Method   | Path                        |
   |----------|-----------------------------|
   | `POST`   | `/api/v1/sessions`          |
   | `DELETE` | `/api/v1/sessions/current`  |
   | `POST`   | `/api/v1/statements`        |

   Every other method/path is `405 web_method_not_allowed` with
   an `Allow` header listing `GET, HEAD` plus any allowlisted
   method on that path. No prefix matching, no trailing-slash
   tolerance. A new browser write route is a change to this
   table.

3. **Loopback `Host` on every request.** Before the method check,
   the proxy requires the request's `Host` (or HTTP/2
   `:authority`) to name `localhost`, an address in
   `127.0.0.0/8`, or `[::1]`, on any port; otherwise `403
   web_forbidden_host`. This is the DNS-rebind defense: a
   rebound page carries its own hostname in `Host`. It is
   load-bearing for writes because `POST /api/v1/sessions` takes
   no prior credential — without it a rebound page could mint a
   session cookie scoped to its own origin and write as any
   keystore actor. Applying it to reads also stops rebound pages
   from reading the keystore actor list. Any port is accepted so
   the Vite dev proxy (which forwards its own `Host`) keeps
   working.

4. **Same-origin `Origin` on writes.** An allowlisted write must
   carry `Origin` equal to `http://<Host>` (ASCII
   case-insensitive); missing, `null`, `https://`, or mismatched
   origins are `403 web_cross_origin`. `SameSite=Strict` does not
   cover this case: every `localhost` port is the same *site*, so
   another local dev server could otherwise ride the cookie or
   mint a session with a plain form post. Reads need no `Origin`.

5. **The proxy stays auth-agnostic.** `kairo-web` does not read
   the session cookie or `Authorization` header; both pass
   through and the daemon is the only authentication and
   authorization authority. The proxy's guards are request-shape
   checks, not identity checks. This keeps the §11 two-process
   split: no keystore or session state in `kairo-web`.

6. **No CORS.** The proxy emits no `Access-Control-*` headers and
   `OPTIONS` stays `405`, so cross-origin preflights fail closed.
   The only supported browser origin is the one `kairo-web` (or
   the Vite dev server in front of it) serves.

7. **Out of scope.** TLS and non-loopback binds (the loopback
   invariant is what makes 3 and 4 sufficient — revisit both if
   it changes), CSRF tokens (redundant with 4 under the loopback
   model), browser-held keys, passphrase-protected sign-in,
   persisted sessions, an `ObjectRevision` web UI (revisions need
   a design for naming a Git tree the browser cannot build), and
   federation peer auth.

Rationale: the cookie path is the smallest change that gives the
inspector real writes without inventing a second auth stack —
sessions produce the same `actor_id` the signed path does, and
capability enforcement is shared. Lifting the POST-block wholesale
would re-open the exact XSS/rebind tunnel §13 closed; the exact
allowlist plus `Host`/`Origin` checks close it again for the three
routes that need to be open, without a token protocol.

Affected specs:

- `PHASE_2_WEB_WRITES.md` — new slice plan.
- `PHASE_2.md` — new §16 tracking the slices; §15.1's "POST-block
  stays" note points here.
- `PHASE_2_WEB_CLIENT_AUTH.md` §4 — the proxy section now cites
  this decision for the write allowlist and guards.
- `DAEMON.md` §18 — "Post-Phase B" paragraph no longer says the
  POST-block stays or that browser auth terminates in
  `kairo-web`; sessions are daemon-side per
  `PHASE_2_WEB_CLIENT_AUTH.md` §2.2.
- §13 sub-decision 3 and 5, §14 sub-decision 7 — annotated as
  superseded by this section where they conflict.

---

## 16. Federation MVP: Object Pull Between Nodes

Decision: the first federation milestone is **pulling a published
object's signed history from one node into another**, over
**per-record, content-addressed HTTP endpoints** served by a
**separate, read-only `kairo-federation-http` process**. A node
is an **actor**; serving is **explicit per-object opt-in**; TLS
is terminated by a **reverse proxy**; and the pulling node keeps
a **local provenance record naming the immediate peer**. The
discovery half of `FEDERATION.md` (token advertisements, token
indexes, provider and resolution discovery, DHT, gossip) waits
for the statement types it depends on. The slice plan is
`PHASE_2_FEDERATION.md`.

Ten sub-decisions, locked together:

1. **A node is an actor.** A node signs, and is named, as an
   ordinary actor (`kairo:actor:<id>` externally) created with
   `actor_kind = "node"`. Keys, rotation, emergency attestation,
   trust, and `KairoActor1` request signing apply unchanged.
   There is no `NodeId` type and no `kairo:node:` reference
   form. `actor_kind` is self-declared, so it is a display hint
   and a lint target (e.g. warn when granting a capability to a
   node actor), never an authority input: what makes an actor a
   peer is local federation policy naming that actor id. By
   convention a node actor holds no capabilities and never signs
   as a maintainer; its operator's personal actor stays
   separate, so a compromised server key cannot sign object
   history.

2. **MVP scope is object pull.** `kairo fetch --object <id>
   --from <url>` (`CLI.md` §15, as a direct federation client
   per `CLI.md` §25) fetches a published object's closure
   (sub-decision 4), verifies it, and stores it. Push, trust
   transport, and the discovery half of `FEDERATION.md` come
   later: discovery needs `ProvidesCapability`,
   `ResolutionSucceeded`, and `TokenAdvertisement`, which arrive
   with `PHASE_2.md` §7 and §8. Push is last because it is the
   only part that lets a remote party write into a store.

3. **Serving is explicit per-object opt-in.** A node serves
   nothing until its operator runs `kairo federation
   publish <object-ref>` (`CLI.md` §25). The publish list is local operator state at
   `<store>/federation/published.json`, not a statement: it is
   policy about this node, not a claim about the object.
   `kairo federation unpublish` stops serving but cannot recall
   copies peers already hold, and the CLI says so. Requests for
   unpublished and nonexistent records get the same `404
   fed_not_found`, so a peer cannot probe for private objects
   (`FEDERATION.md` "Privacy and Access Control").

4. **The published closure of an object** is everything a
   pulling node needs to verify it, and nothing that would read
   as authority it shouldn't:
   - its `ObjectGenesis`, plus every `ObjectRevision`,
     `ObjectBranch`, and `ObjectVersionTag` whose body names it;
   - **authority evidence**: every `ActorCapabilityGrant` scoped
     to the object, and every `ActorCapabilityRevocation` of
     such a grant;
   - the id of every actor that signed any of the above.
     Actor data is not part of the object. The client fetches
     each actor's `ActorGenesis` and current key-event history
     (rotations, revocations, emergency and attestation kinds)
     from `/federation/v1/actors/{id}`, and skips any it already
     holds up to date. A node may hold many objects by one actor,
     and that actor's keys change independently of any of them.
     The server serves an actor's record when the actor appears
     in some published closure;
   - every blob those statements reference;
   - the object's Git pack, when the managed cache has one.

   `ActorTrust` is never served in the MVP. Shipping grants
   departs from the object-bundle rule (`crates/kairo-bundle/AGENTS.md`)
   on purpose. A grant is not an opinion: its effect is computed
   by the same capability evaluator against the object's genesis
   authority, so importing one confers nothing unless its chain
   roots there, and withholding it only turns cross-actor
   statements `Indeterminate`. Trust opinions have no such
   anchor and would be misread as authority, so they stay out
   until an opt-in trust transport exists. A pull is not
   complete until every listed actor's key events are fetched,
   because withholding a revocation is the attack
   `THREAT_MODEL.md` warns about. Object bundles follow the same
   split by shipping only each signer's `ActorGenesis`. The
   importing node is responsible for getting current key
   knowledge, which it may already have and which may have
   changed since export.

5. **Wire format: per-record endpoints under `/federation/v1/`.**
   - `GET /.well-known/kairo-node` returns the node document.
   - `GET /federation/v1/objects` lists published object ids.
   - `GET /federation/v1/objects/{id}` lists the ids in that
     object's closure, and `/objects/{id}/genesis` returns its
     `ObjectGenesis`. The genesis has no statement id, because
     its body derives the object id.
   - `GET /federation/v1/actors/{id}` returns an actor's
     genesis and key-event ids.
   - `GET /federation/v1/statements/{id}` and
     `/federation/v1/blobs/{id}` return single records.
   - `GET /federation/v1/objects/{id}/git` returns the Git pack.

   Statements are served as exactly the JSON DTO the store holds
   (the `kairo-statement` wire shape, not `FEDERATION.md`'s
   earlier camelCase sketch), and blobs as raw bytes. Paths and
   typed fields carry bare ids. Listings are ids only, ordered
   by id, and paged with `limit` (at most 1000) and an opaque
   `cursor`; the client diffs a listing against its own store
   and fetches only what it lacks, which makes re-pulls
   incremental. Errors use the `API.md` §7.2 envelope with
   `fed_*` codes. A whole-bundle stream was rejected because it
   resends everything on every pull and would wait on the tar
   transport.

6. **Serving is a separate, read-only process.**
   `kairo-federation-http` opens the store read-only and
   computes closures itself; it does not proxy the daemon API,
   whose reads are unauthenticated and assume socket
   permissions, so exposing them would publish the whole store.
   It serves plain HTTP and binds `127.0.0.1` by default. A
   non-loopback bind needs an explicit `--bind` with a
   non-loopback address and a startup warning. TLS is terminated
   by a reverse proxy (Caddy, nginx), and the node document's
   `base_url` names the public `https://` URL. The daemon stays
   Unix-socket only. Daemon-side `FederationService` integration
   (`DAEMON.md` §5.3, §10) comes after the MVP.

7. **The pulling side verifies more than bundle import does.**
   `kairo fetch --from` writes to the store directly, like
   `kairo bundle import`. Each record is fixity-checked (its id
   re-derived and compared with the id requested) and each
   statement's signature is verified against the actor's
   key-event history before `put_*`. Bundle import does not
   verify signatures today. Records go in dependency order
   (actors and key events, then genesis, then everything else),
   so an interrupted pull leaves a consistent store and a re-run
   resumes. A fixity, signature, or schema failure rejects that
   record and fails the pull, and so does a record that doesn't
   belong where the listing put it (an object statement for
   another object, a grant scoped elsewhere, any `ActorTrust`). A
   statement whose signing key is revoked at its `created_at` is
   skipped, not stored, without failing the pull: it is a verified
   fact about the actor rather than tampering, and the
   revocation came through the same checks. Records that verify but fail a
   capability check are stored anyway, because the store keeps
   conflicts and the resolver decides (`FEDERATION.md` "Conflict
   Handling"). The client requires `https://` except for
   loopback `http://`, which tests use, and enforces size and
   count limits on every response. Client TLS uses rustls. This
   is the workspace's first TLS dependency, which §8's
   shell-out choice deferred, and it stays confined to the
   federation client.

8. **Provenance is a local record of the immediate peer.** For
   every statement, blob, actor genesis, and object genesis that a
   pull stores or re-sees, the
   pulling node records `{ peer_url, peer_actor, fetched_at }`
   at `<store>/provenance/<XX>/<YY>/<id>.json`, using the
   standard shard scheme. A record keeps its first-seen entry
   and appends one per distinct peer that later serves the same
   id. Provenance is primary local state, not a materialized
   index: it cannot be rebuilt from `statements/`, so
   `rebuild-indexes` must not touch it. A record with no
   provenance was authored or imported locally. Authorship needs
   no provenance because the signature proves it. Only the
   immediate peer is recorded, not the route a relayed record
   took. A route can be added later, either as an unverified
   hint or through signed holder statements tied to
   `FEDERATION.md` holder discovery, without breaking this
   record.

9. **The MVP node document is unsigned.** It carries
   `node_actor`, the actor's `ActorGenesis` (so the client can
   check that the id derives from it), `base_url`,
   `protocol_versions: ["kairo-fed/1"]`, and `supports:
   ["object-pull"]`. Pulled data never depends on the node's
   identity, because every record verifies on its own. So in
   provenance, `peer_url` is the observed fact and `peer_actor`
   is recorded as claimed. Authenticating nodes, by signing the
   document and responses or by `KairoActor1` on requests,
   comes with push, which is the first operation that has to
   know who the peer is.

10. **Out of scope:** push and its acceptance policy; trust
    transport and `forget` for peer opinions (`PHASE_2.md` §4);
    token advertisements, indexes, the query protocol, DHT, and
    gossip; daemon `FederationService`; authenticated or private
    reads; rate limiting beyond the client's response limits;
    bundle-level signatures (`PACKAGE.md` §24); recording the
    full relay route.

Rationale: object pull is the smallest exchange that exercises
the whole trust design. Records verify independently of their
transport, conflicts survive, nothing is trusted on arrival,
and the receiver records where data came from. It uses only
statement types that exist today. Making the node an actor
reuses the identity, rotation, and trust machinery rather than
duplicating it for a second principal type, and the node
convention keeps a hot server key away from object authority.
Opt-in publishing, a read-only server, and loopback-by-default
binding mean one misconfiguration cannot expose a private store.

Affected specs:

- `PHASE_2_FEDERATION.md`: new slice plan.
- `PHASE_2.md` §4: checkboxes resolved against this section,
  and its `kairo federate push/pull` names replaced with
  `CLI.md`'s `kairo fetch` / `kairo federation` tree.
- `FEDERATION.md`: new status section scoping the MVP to this
  decision. The node document, envelope, endpoint, and
  blob-transfer examples now follow the real wire shape and
  `IDENTIFIERS.md`. `kairo:node:` references become actor
  references.
- `PROJECT_LAYOUT.md`: `kairo-federation-core` "Node identity"
  now means node actors. `kairo-federation-http` gains the
  serve binary and fetch client.
- `DAEMON.md` §5.3 and §10: the MVP serves from a separate
  process; daemon integration is deferred.
- `CLI.md` §15 and §25: `kairo fetch --from` and the
  `kairo federation` subcommands the MVP implements.
- `OBJECT_STORE.md`: federation endpoint examples use the
  `/federation/v1/` paths.

---

## 17. Node Configuration File

Decision: a node reads its configuration from **`<store>/config.toml`**,
which is `~/.kairo/config.toml` for the default store. Parsing is
**strict**, and the file accepts **only keys that are implemented**. It
starts with the `[federation]` section that `PHASE_2_FEDERATION.md`
slice 5 needs. A missing file means every default applies.

Sub-decisions:

1. **Location: inside the store root.** The store root is already
   the node's home (keys default to `<store>/keys`, the Git cache is
   `<store>/git`). Putting the config there keeps `--store` the one
   switch that selects a node, so tests and multiple local nodes
   stay isolated. It also means the config can't relocate its own
   store, so `DAEMON.md` §15's `[store] path` key is not supported.
   The global `--config <path>` flag (`CLI.md` §3) and a daemon-side
   `ConfigService` (`DAEMON.md` §5.8) come later and must keep this
   file as the default.

2. **Strict and incremental.** Unknown sections and keys are errors,
   so a misspelled `node_actor` can't quietly turn pinning off.
   `[api]`, `[policy]`, `[runtime]`, and the rest of `DAEMON.md` §15
   are added as their features land; until then they are rejected,
   not ignored. `kairo config validate` reports errors and `kairo
   config path` shows the location (`CLI.md` §27). The CLI never
   edits the file.

3. **`[federation]` keys.**
   - `enabled` (default `true`). `false` is local-only mode
     (`DAEMON.md` §10.1): `kairo fetch` refuses to run.
   - `[federation.limits]`: optional overrides of the pull client's
     limits (`max_json_bytes`, `max_blob_bytes`, `max_git_bytes`,
     `max_records`, `request_timeout_secs`, `git_timeout_secs`).
     Zero is invalid.
   - `[[federation.peers]]` entries, each with a `url` and an
     optional `node_actor` (a bare actor id).

4. **Fetch policy.** `kairo fetch` checks policy before any network
   I/O. `--offline` refuses. The peer must be listed under
   `[[federation.peers]]` unless `--allow-peer` is given for that one
   run. A listed `node_actor` is a pin: the pull compares it with the
   node document's claim before fetching anything, and refuses on a
   mismatch. An unpinned peer's claim is reported along with the
   line that would pin it. Policy refusals exit 7 (policy denied)
   and pull failures exit 11 (federation error), per `CLI.md` §7.

5. **State is not config.** What a node has done goes in the store,
   not the config file: the publish list (§16 sub-decision 3), per-record
   provenance (§16 sub-decision 8), and the last successful pull per
   peer (`<store>/federation/pulls.json`, shown by `kairo federation
   status`).

Rationale: federation policy has to live somewhere before discovery
or push widen who a node talks to. A strict file with only real keys
is the smallest form that won't need a compatibility parser later.
Keeping it in the store root follows the precedent of the keystore
and the Git cache.

Affected specs:

- `PHASE_2_FEDERATION.md` slice 5 — implemented against this.
- `CLI.md` §15, §25, §27 — `kairo fetch --allow-peer`,
  `kairo federation status`, `kairo config path | validate`.
- `DAEMON.md` §15 — the file's location and the keys implemented
  so far.
- `API.md` §14.2 — provenance is served at `GET
  /api/v1/provenance/{record}`, not inside the statement response,
  so that response stays exactly the signed record.
