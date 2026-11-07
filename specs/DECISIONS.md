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
  pool-sha1/                           # shared bare repos, one per Git
  pool-sha256/                         #   object format (§19)
    objects/                           # all Git objects/packs live here
  <XX>/<YY>/<object-id>/               # per-Kairo-object bare repo
    objects/info/alternates            # → ../../../pool-<format>/objects
    refs/heads/...                     # this object's refs only
```

(The single `pool/` this section originally specified is split by
object format in §19.)

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
kairo git fetch --object <id> --remote <url> [--refspec <ref>] [--object-format sha1|sha256]
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
   - its `ObjectGenesis`, plus every `ObjectBranch` and
     `ObjectVersionTag` whose body names it, and every
     `ObjectRevision` those refs (and the genesis's initial revision)
     reach along parent links (§20; it was every revision naming the
     object before);
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
   until an opt-in trust transport exists. The client fetches every
   key event the peer lists, but it cannot know about events the
   peer withholds. A withheld rotation is caught, because later
   statements signed with the new key fail verification. A withheld
   *revocation* is not: statements signed after it still verify.
   That is the attack `THREAT_MODEL.md` warns about, and one peer
   cannot rule it out. The defenses are re-pulling (actor listings
   are re-read on every pull, so events arrive incrementally) and
   pulling from more than one peer. Fetching an actor's key events
   from several peers is the eventual fix. Object bundles follow the same
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
   - `/federation/v1/objects/{id}/git` serves the object's Git
     history over Git's smart HTTP protocol v2 (§22).

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
   (§21 briefly gave it a pack cache under the Git cache's
   `outgoing/`; §22 removed it, so it writes nothing.) It serves plain HTTP and
   binds `127.0.0.1` by default. A
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
   stored and flagged in the pull report, without failing the pull.
   It is genuine, the origin holds it too, and the revocation is a
   verified fact that read-time verification reports. Skipping it
   would make a node's contents depend on the order key events
   arrive in, and would make resolvers on two nodes disagree.
   (Amended 2026-10-17; it was skipped before.) Records that verify but fail a
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

9. **The MVP node document is unsigned.** *Superseded by §25
   sub-decision 1, which signs it.* It carries
   `node_actor`, the actor's `ActorGenesis` (so the client can
   check that the id derives from it), `base_url`,
   `protocol_versions: ["kairo-fed/1"]`, and `supports:
   ["object-pull"]`. Pulled data never depends on the node's
   identity, because every record verifies on its own. So in
   provenance, `peer_url` is the observed fact and `peer_actor`
   is recorded as claimed. A `node_actor` pin (§17) therefore
   catches a re-pointed URL, not an impersonator who copies the
   real document, and the CLI says "claim matches, node not
   authenticated". Authenticating nodes, by signing the document
   and responses or by `KairoActor1` on requests, is a
   prerequisite for push and for discovery (§18): both act on
   *which node* answered. (Amended 2026-10-17; it was tied to push
   alone before.)

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
   `[api]`, `[policy]`, and the rest of `DAEMON.md` §15
   are added as their features land; until then they are rejected,
   not ignored. `kairo config validate` reports errors and `kairo
   config path` shows the location (`CLI.md` §27). The CLI never
   edits the file.

3. **`[federation]` keys.**
   - `enabled` (default `true`). `false` is local-only mode
     (`DAEMON.md` §10.1): `kairo fetch` refuses to run.
   - `[federation.limits]`: optional overrides of the pull client's
     limits (`max_json_bytes`, `max_batch_bytes` (§24),
     `max_blob_bytes`, `max_git_bytes`, `max_records`,
     `request_timeout_secs`, `git_timeout_secs`, and for file sources
     (§38) `max_file_bytes` and `file_timeout_secs`, each defaulting to
     1 GiB and 30 minutes).
     Zero is invalid.
   - `[[federation.peers]]` entries, each with a `url` and an
     optional `node_actor` (a bare actor id).
   - `[[federation.accept]]` entries (§26 sub-decision 2), each with
     an `object`, an optional `from`, and `publish` (default `false`).
     The same `(object, from)` twice is an error. An entry naming the
     pusher wins over one without `from`.
   - `[federation.limits]` also takes `max_push_bytes` (default
     1 GiB) and `max_inbox_entries` (default 64), for the receiving
     side (§26 sub-decision 3).
   - `[federation.sync]`: `interval_secs` (900), `inbox_secs` (30),
     and `max_concurrent` (2), the daemon's background sync schedule
     (§28 sub-decision 3). Zero is invalid.
   - `[[federation.bootstrap]]` entries, shaped like peers (`url`,
     optional `node_actor`), are asked for inventories and nothing
     else (§18 sub-decision 1, §29).
   - `[federation.on_demand]`: `on_read` (`false`) and
     `cooldown_secs` (300; zero is invalid) (§30).
   - `[federation.discovery]`: `hint_ttl_secs` (604 800; zero is
     invalid), `use_hints` (`false`), and `vouched_by` (the actor whose
     `federation:index` trust vouches for hints; required by
     `use_hints`) (§29 sub-decision 5).

   `[runtime]` (§33 sub-decision 4, §34): `allow_local_process`
   (`false`; development mode), `timeout_secs` (3600; zero is
   invalid), and `[[runtime.host.tools]]` entries (`provides`, an
   absolute `path`, optional `version`), the development-mode host
   tools. Host facts
   (`[runtime.host]`: system, architecture) are detected, never
   configured. Engine builds need neither `allow_local_process` nor
   signer trust (§35 sub-decision 5); `[[runtime.engines]]` entries (`kind`, absolute `path`)
   allowlist isolation engines, each probed before use (§35
   sub-decision 2); `[[runtime.seeds]]` entries name a `source` and a
   `revision` (§37 sub-decision 8).

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

---

## 18. Bootstrap Peers, Learned Peers, and Node Trust

Decision: discovery separates **bootstrap nodes** (asked where other
nodes are) from **allowed peers** (pulled from or pushed to). Peers
learned through discovery are **hints stored as state, never
permissions**. Trust between nodes is **`ActorTrust` signed by the
node actor** about the peer's node actor, and `ActorTrust` gains a
**scope**, so trusting a node's discovery hints is not trusting it
for anything else. This settles `PHASE_2_FEDERATION.md` §4.1 ahead of
the discovery phase. Nothing here is implemented yet.

Sub-decisions:

1. **Two roles in config.** Bootstrap nodes and allowed peers are
   both operator policy in `<store>/config.toml` (§17). One node may
   hold both roles, but they are separate settings: being asked for
   peers grants nothing else. The exact keys are set by the
   discovery slice plan. Today's `[[federation.peers]]` keeps its
   meaning as the allow list, with optional `node_actor` pins.

2. **Learned peers are state with provenance.** A peer learned
   through discovery is recorded under `<store>/federation/` with
   the node that advertised it and when, following the
   immediate-peer rule of §16 sub-decision 8. A learned peer may not
   be pulled from or pushed to until the operator allows it or a
   policy rule does. Such a rule may only be stated in terms of
   scoped trust (sub-decision 4), for example "advertised by a node
   I trust for `federation:index`".

3. **The node actor signs node trust.** A node's opinion of a peer
   is an `ActorTrust` statement signed by the node's own actor about
   the peer's node actor. That makes it what other nodes read when
   they follow hints transitively. A trust opinion is speech, not
   authority, so this does not conflict with the §16 sub-decision 1
   rule that a node actor holds no capabilities and never signs
   object history. It does mean the node's signing key, which sits
   on a networked server, now signs statements. Key compromise is
   handled by the existing actor machinery: operational rotation,
   plus emergency revocation through the node actor's cold
   attestation keys (`ACTORS.md` §5.5). Operators should give node
   actors real attestation keys, not throwaway ones.

4. **`ActorTrust` gains a scope.** The chain key becomes
   `(by_actor, trusted_actor, scope)`. Each statement names exactly
   one scope, and a successor must keep its predecessor's scope.
   This mirrors capability chains, which are keyed by
   `(grantor, grantee, scope)` (`CAPABILITIES.md` §5.1.1).
   - Existing unscoped opinions mean scope `general`: today's
     informational actor trust shown by `verify object`.
   - Federation scopes are named by the features that consume them.
     Discovery defines `federation:index` ("this node's hints are
     worth following"); push defines `federation:push` ("accept
     pushes from this node").
   - A query asks about one scope and reads only that scope's chain.
     There is no fallback between scopes in either direction, so
     `general` trust never satisfies `federation:index`.
   - The scope is added to `ActorTrust v1` in place, with no v2
     bump. Nothing is deployed yet, so no stored `StatementId`
     needs migrating. This is the precedent `ObjectBranch`'s
     `supersedes` set (`PHASE_2.md` §12). If trust statements are in
     deployed stores before this lands, it reverts to a v2 bump.

5. **Propagation stays opt-in and bounded.** "Peers that nodes I
   trust for `federation:index` trust for `federation:index`" uses
   the same per-truster resolution the actor graph already has. A
   node follows it only when its policy enables it, with a depth
   limit. Importing another node's trust statements never makes
   them this node's opinions (§16 sub-decision 4, `PHASE_2.md` §4).

Discovery therefore depends on three things that don't exist yet:
scoped trust (sub-decision 4), trust transport (fetching other
nodes' `ActorTrust`, `PHASE_2_FEDERATION.md` §4 item 2), and node
authentication (§16 sub-decision 9). Without authentication, a node
trusted for `federation:index` could be impersonated by anyone who
copies its node document.

Rationale: once nodes are actors, node-to-node trust is
actor-to-actor trust, so reusing `ActorTrust` gets chains,
withdrawal, the reverse index, and the "importing is not trusting"
rule without a parallel system. Having the node actor sign makes the
opinion portable to other nodes. That is the point of
node-to-node trust, and the reason it can't be the operator's
personal actor. Scope is the one missing piece: without it, a node
trusted to point at other nodes would also count as trusted for
pushes and, later, build results.

Affected specs:

- `PHASE_2_FEDERATION.md` §4.1 — now records this decision; the
  sequencing note says scoped trust comes before discovery.
- `STATEMENTS.md` §4.2c and `schemas/canonical/actor-trust-v1.md`
  — gain the scope in place (sub-decision 4; implemented, with
  `kairo trust --scope` and `?scope=` on the daemon's trust read).
- `PHASE_2.md` §12 — new item for scoped `ActorTrust`.

---

## 19. Git Object Formats

Decision: a Git revision id names its repository's object format,
**`git:sha1:<40 hex>`** or **`git:sha256:<64 hex>`** (lowercase). It
is parsed strictly, and the format comes from the label, never the
digest length. Each object's history has **one** format. The managed
cache keeps **one pool per format**. Git reads go through **gix with
both hash features**, so SHA-256 repositories verify like SHA-1
ones. This implements `OBJECT_STORE.md` "Revisions". It replaces the
Phase 1 choice of a single `git:sha256:` prefix (`PHASE_1.md` §11),
which had labeled SHA-1 commits as SHA-256 and left SHA-256
repositories unreadable.

Sub-decisions:

1. **Strict ids.** `GitRevision::parse` (in `kairo-statement`) accepts
   exactly the two shapes above. Any other `git:` value is malformed:
   no format segment, an unknown format, a wrong length for its
   label, or non-lowercase hex. A malformed value is an error, not
   "some other scheme". Values outside the `git:` scheme (e.g.
   `hg:…`) still pass through and leave the content layer
   indeterminate.

2. **Enforced where revisions enter a store.**
   `check_revision_ids` validates an `ObjectRevision`'s revision
   and parents, and requires the parents to share the revision's
   format. It runs at:
   - `kairo revision create` and `kairo object create
     --initial-revision`;
   - the daemon's `POST /api/v1/statements`;
   - `kairo bundle import`;
   - federation pull (as a schema rejection).

   Records already in a store are not re-validated on read.
   `kairo verify object` reports a malformed `git:` revision as an
   error.

3. **One format per object.** `revision create` refuses a revision
   whose format differs from the object's existing revisions. An
   object's per-object cache repo has one fixed format;
   `ensure_repo` with another format is
   `GitError::FormatMismatch`. A cache repo in another format can't
   hold a revision, so `verify object` treats it as a cache miss.

4. **Per-format pools.** The single `pool/` of §7 becomes
   `pool-sha1/` and `pool-sha256/` at the cache root. Each is created
   on first use with `git init --bare --object-format=<format>`, and
   a per-object repo's alternates point at the pool of its own
   format. Git cannot mix formats in one repository, so a single
   pool could only ever hold one of them. `GitCache::open` no longer
   initializes anything. Every cache operation takes or checks the
   format: `ensure_repo`, `ingest_pack_from`, `has_commit`,
   `set_ref`, `fetch`.

5. **Packs are checked for completeness before anything is pinned.**
   `index-pack` accepts a pack that carries a commit without its
   trees or blobs. After ingesting a pack (bundle import or
   federation pull), `GitCache::missing_objects` runs `git rev-list
   --objects` per named commit against its pool. Only when every
   closure is complete does the object's repo get created and the
   commits pinned. A failed check pins nothing and creates no repo,
   so a node never advertises a pack it can't serve. Objects the
   pack added to the pool stay until cache GC.

6. **Bundles name typed revisions.** `git_history.expected_commits`
   holds `git:<format>:<oid>` strings, so import knows which pool a
   shipped pack belongs to. A bundle that ships Git data but names
   no commits, or names commits in more than one format, is
   refused. This changes the field in place; nothing is deployed.

7. **`kairo git fetch --object-format`.** The format comes from the
   flag, the existing cache repo, or the object's stored revisions,
   and every source that is present must agree. If none is
   present, the command asks for the flag. *Amended:* the remote is a
   fourth source, read from the length of the id `git ls-remote`
   reports for the branch, and must agree with the others too, so a
   fetched history takes its source's format without a flag. When
   none of the four says, the command still asks, before creating
   anything. Its output names which source decided. History born in
   the archive (not fetched from a source) is **SHA-256**, the default
   for new objects; history that extends or mirrors a source keeps
   the source's format (§37).

8. **gix 0.89 with `sha1` and `sha256`.** Upgraded from 0.66, which
   only knew SHA-1. `kairo-git`'s read surface (discover/open,
   commit lookup, blob at path) needed one API change. Reads stay
   in-process, so `verify object` still needs no `git` binary for
   either format.

Rationale: the spec already asked for both formats, explicit labels,
and SHA-256 for new repositories. The code labelled SHA-1 commits
`git:sha256:`, inferred the algorithm from length, and could not read
a SHA-256 repository at all. Federation makes Git's hash the
integrity guarantee for content across nodes, so the label has to be
true and checked.

Affected specs:

- `PHASE_1.md` §11 — the single-prefix decision is superseded here.
- `DECISIONS.md` §7, `STORE.md` §4, `PHASE_2.md` §1 — the pool
  layout becomes per-format.
- `PACKAGE.md` — `expected_commits` holds typed revisions.
- `DECISIONS.md` §9 — `kairo git fetch --object-format`; `git cache
  status` reports each pool.
- `THREAT_MODEL.md` — the Git-cache row names the formats.

---

## 20. Revision History as a Git-like Log

Decision: an object's revision history is a **graph walked from a
starting point**, like `git log`. It is never one flat list of every
revision ever signed. The store keeps a **per-object index from
revision id to the statements naming it**, which is what following a
parent link needs. **What federates is what the object's refs reach**,
as in a Git fetch: branch and tag targets and the genesis's initial
revision, plus their parent chains. A revision no ref reaches stays
local, like a dangling commit.

Sub-decisions:

1. **The index.** `<store>/object_revisions/<XX>/<YY>/<object-id>.json`
   maps each revision id (`git:sha1:…`, `git:sha256:…`, or another
   scheme) to the `ObjectRevision` statements that name it for that
   object. Several statements can name one revision, because
   different actors may sign the same commit. `put_object_revision`
   maintains it under the index lock, and `rebuild_indexes`
   reproduces it. A revision names its parents by revision id, not
   statement id, so this lookup is what a walk needs at every step.

2. **The walk.** It starts from a set of revision statements and
   emits newest first by `(created_at, statement_id)` from a max-heap,
   queuing each emitted revision's parents through the index. Each
   statement is emitted once, so merges and shared ancestors appear
   once. Statements that aren't revisions of the object are ignored
   as starting points. The order is deterministic for a given store,
   so pages are an offset into it. A page costs the history walked,
   not the store's size.

3. **The local log starts from everything held.**
   - `GET /api/v1/objects/{id}/log?from=&limit=&cursor=` (page size up
     to 500, default 50).
   - `kairo revision log --object <id> [--from <statement>]...
     [--limit] [--skip]`.

   By default both start from every revision this node holds for the
   object, so a revision just signed shows before a branch moves to
   it. That's what the branch and tag pickers need. `from` scopes the
   walk to a tip. These replace `GET /api/v1/revisions/{object}` and
   `kairo revision list`, which returned one flat list sorted by
   time.

4. **The closure starts from the refs.** `revision_ref_tips` collects
   the targets of every `ObjectBranch` and `ObjectVersionTag` chain
   entry (superseded refs too, so their history stays reachable) and
   the statements naming the genesis's `initial_revision`.
   `reachable_revisions` walks from them. The federation closure
   (§16 sub-decision 4) and bundle export (`PACKAGE.md`) both carry
   exactly the reachable revisions and their manifest blobs. To
   share a revision, point a branch or tag at it. Bundle export no
   longer walks `statements/`; it reads the closure.

5. **The inspector pages.** The object page's Revisions table shows
   the log newest first, with "Load older revisions". The revision
   graph draws the loaded window, and parents beyond it render as
   stubs (the layout already supported that). The branch and tag
   dialogs offer the log's first page.

Rationale: no consumer actually needs every revision at once. People
look at recent history and walk back from a tip, and a peer needs what
the refs reach. The flat list walked every statement file in the store
for each request, and the federation server repeated that per
published object every few seconds. Following Git's reachability rule
also gives the closure a definition a person can predict: "what the
branches and tags point at".

Affected specs:

- `DECISIONS.md` §16 sub-decision 4 — revisions in the closure are the
  reachable ones.
- `PACKAGE.md` — object bundles carry reachable revisions.
- `crates/kairo-bundle/AGENTS.md` — the "goes in" list.
- `CLI.md` §5.1 — `kairo revision log`.
- `PHASE_2_FEDERATION.md` — the view-rebuild risk note is resolved.

---

## 21. Git Transfer Concurrency, Following Git Hosts

Decision: the managed Git cache and the federation pack endpoint
follow the practice of Git itself and large Git hosts (Gitaly,
GitHub, Gerrit). **Readers take no locks**; cache GC uses a grace
period instead. **Incoming packs are quarantined** until verified.
**Outgoing packs are cached per ref snapshot**, with one generation
per object at a time, a **cap on concurrent generation**, and a
**server-side transfer deadline**. No lock is ever held across
network I/O.

This replaces the slice 4 behavior, which had three problems:
- **Serving held the object's cache lock for the whole transfer.**
  One slow or stalled peer made every other operation on that object
  fail after the 2-second lock deadline.
- **Pulling held the format pool's lock while downloading.** That
  blocked all Git ingestion on the pulling node.
- **Nothing capped concurrent `pack-objects` runs.**

Sub-decisions:

1. **Lock-free reads.** Packing reads the object repo's refs into a
   `RefSnapshot`, then runs `git pack-objects --revs` with those
   commit ids on stdin. Git objects are immutable and the ids were
   read up front, so ref updates during the run can't change what
   goes in. Locks remain only for short local writes: initializing a
   pool or repo, `update-ref`, and moving quarantined packs into a
   pool. `GitCache::fetch` no longer holds the pool lock across
   `git fetch`, because Git writes fetched packs under temporary
   names and takes its own per-ref lock files (the refs are
   namespaced per object).

2. **GC grace period, not reader locks.** When cache GC lands
   (`STORE.md` §12), it may prune pool objects only after they have
   been unreachable from every per-object repo's refs for longer than
   a grace period. This is Git's own `gc.pruneExpire` model (two
   weeks by default). A reader that resolved commit ids moments ago
   never loses the objects it is reading, so readers never coordinate
   with GC.

3. **Quarantined ingest.** `GitCache::ingest_pack_from(format, source,
   required)` follows Git's `receive-pack` quarantine:
   - `git index-pack --stdin` writes into a fresh object directory
     under `<git root>/incoming/`, whose alternates point at the pool.
     No lock is held, so a slow download blocks nothing.
   - Every `required` commit's full closure must be visible from the
     quarantine; otherwise the quarantine is deleted
     (`GitError::IncompletePack`).
   - On success, the pack files move into the pool under a brief pool
     lock, `.pack` before `.idx`.

   Federation pull and bundle import both ingest this way. A failed
   pull leaves no objects in the pool (this resolves the slice 4
   "pool junk" note).

4. **Pack cache per ref snapshot.** *Superseded by §22
   sub-decision 4:* negotiated packs depend on the client's `have`s,
   so the whole-history cache under `<git root>/outgoing/` was
   removed, and the server writes nothing again.

5. **Bounded serving.** A semaphore caps concurrent pack-producing
   requests across objects (`--max-concurrent-packs`; default half
   the CPUs, at least one). A request that can't get a slot within 10
   seconds gets `503 fed_busy` with `Retry-After: 5`. §22
   sub-decision 4 keeps the cap and replaces this section's
   size-based transfer deadline with a 30-second stall timeout.

6. **Next: incremental fetch.** *Done by §22,* which adopts Git's
   smart HTTP protocol, so the client's `have`s make the server send
   only the difference.

Rationale: holding a lock across the network makes one slow client
everyone's problem. Git solved this long ago with immutable objects,
atomic refs, quarantine, and grace-period GC, and the hosts that
serve Git at scale add caching, deduplication, and concurrency limits
on top. Adopting the same shape keeps Kairo's behavior predictable to
anyone who knows Git operations.

Affected specs:

- `STORE.md` §4 and §12 — `outgoing/` and `incoming/` in the cache
  layout; the GC grace-period rule.
- `PHASE_2_FEDERATION.md` — the pack endpoint's behavior and `fed_busy`
  in §2.3; the pack-lock and pool-junk risk notes resolved;
  incremental fetch added to §4.
- `DECISIONS.md` §16 sub-decision 6 — the server writes its pack cache.

## 22. Git Smart HTTP as the Federation Content Plane

Decision: Git history moves between nodes over **Git's own smart
HTTP protocol, version 2**, not a Kairo pack endpoint. Each
published object's repo is served at
`/federation/v1/objects/{id}/git` (`info/refs` and
`git-upload-pack`), and a pull runs `git fetch` against it. Git's
have/want negotiation is the incremental fetch §21 sub-decision 6
called for. The signed statement plane stays on Kairo's per-record
endpoints (§16 sub-decision 5): statements are small, verified one
by one, and already fetched only when missing.

Storing statements themselves in Git (Radicle-style per-peer ref
namespaces and signed refs) was considered and deferred. Statements
form a set that nodes append to independently, so they don't map
onto one Git history. Their ids are Kairo hashes, not Git ids. And
moving them would change the store backend, not just the transport.
It is an open question for push and discovery
(`PHASE_2_FEDERATION.md` §4.1).

Sub-decisions:

1. **The server filters every request; `upload-pack` never sees a
   raw one.** Protocol v2's `upload-pack` serves any object it can
   read, whatever `uploadpack.allow*SHA1InWant` says (only v0
   enforces those). A per-object repo reads the whole shared pool
   through its alternates, which holds unpublished objects' commits
   too. So:
   - **`ls-refs` is answered by Kairo.** It advertises one
     `refs/kairo/revisions/<oid>` per published commit (the commits
     named by the object's reachable revisions,
     `ObjectClosure::commits`), with no `HEAD`.
   - **`fetch` is filtered.** Every `want` must be a published
     commit, or the request is refused with `400
     fed_bad_git_request`. A `have` that isn't one is dropped, so
     `upload-pack` can neither acknowledge an unpublished object (an
     existence oracle) nor send deltas against it. `want-ref` and
     `packfile-uris` are refused. The rest (`thin-pack`, `filter`,
     `shallow`, `done`, …) passes through.
   - **Every other command is refused** (`object-info`, `bundle-uri`).
     The capability advertisement is rewritten to `version`, `agent`,
     `ls-refs`, `fetch`, `object-format`, and `server-option`.
   - **Only `git-upload-pack` is served.** `git-receive-pack` gets
     `400` on `info/refs` and `405` on `POST`, and the
     `git-upload-pack` RPC is the federation server's one `POST`.
   - **Protocol v0 and v1 are refused** (`400`, no `Git-Protocol:
     version=2`), because their ref advertisement would come from
     the repo and bypass `ls-refs`. Git has defaulted to v2 since
     2.26.

   An unpublished object, or a published one with no Git history on
   the node, is `404 fed_not_found` on both paths, like every other
   record.

2. **Pinned Git on both sides.** `kairo-git`'s `protocol` module
   builds both commands with system and global Git config ignored
   (`GIT_CONFIG_NOSYSTEM`, `GIT_CONFIG_GLOBAL=/dev/null`) and
   repository-redirecting environment variables removed. A peer can
   therefore never reach the operator's credential helpers,
   `http.extraHeader`s, or `insteadOf` rewrites, and a node serves
   the same way whatever its operator's `~/.gitconfig` says.
   - **The fetch** allows only the `https` and `http` transports
     (the peer URL check of §16 still applies), follows no
     redirects, never prompts, keeps every fetched pack as a pack,
     and aborts below 1 KiB/s sustained for 60 seconds.
   - **`upload-pack`** runs with `--strict` and the advertisement
     settings off.

3. **Quarantined, negotiated fetch.** `GitCache::fetch_commits`
   fetches into a quarantine under `incoming/`, as §21's ingest does.
   Its refs are seeded with the object repo's current ref tips, so
   negotiation sends them as `have`s. The pool's own refs count too,
   through alternates. Git's connectivity check and Kairo's
   per-commit closure check must both pass before the packs move
   into the pool. Git has no fetch-side size cap
   (`receive.maxInputSize` is push-only), so the client kills the
   fetch if the quarantine grows past `max_git_bytes` (`TooLarge`)
   or the fetch runs past `git_timeout`. A pull now needs `git` ≥
   2.29 (`fetch --stdin`).

4. **Serving bounds, amending §21 sub-decisions 4 and 5.**
   - **The pack cache is gone.** A negotiated pack depends on the
     client's `have`s, so whole-history packs keyed by ref snapshot
     no longer match what is sent. The server writes nothing to the
     store again, so §16 sub-decision 6 is "read-only" without
     exception. A request-keyed cache, as Gitaly does through
     `uploadpack.packObjectsHook`, is future work if repeated full
     clones become a cost.
   - **The concurrency cap stays.** `--max-concurrent-packs` caps
     concurrent `fetch` requests, each holding its slot until its
     response ends. Over the cap, a request waits up to 10 seconds,
     then gets `503 fed_busy` with `Retry-After: 5`. Advertisements
     and `ls-refs` take no slot.
   - **The transfer deadline becomes a stall timeout.** A response's
     size isn't known up front, and `upload-pack` legitimately pauses
     while counting objects. So a peer that leaves a chunk unread for
     30 seconds is disconnected, and its `upload-pack` is killed.

5. **Any Git client can mirror published history.** `git clone
   --mirror https://<node>/federation/v1/objects/<id>/git` yields
   every published commit under `refs/kairo/revisions/`. It carries
   no signatures, so it is content only. Verification still needs
   the statements.

6. **Next.** Reachability bitmaps on the pools (served through
   alternates), then bundle or packfile URIs for static hosting.
   Both are now Git configuration, not Kairo protocol.

Rationale: §21 left Kairo one step from reimplementing Git's
transport. Using Git's own protocol gives negotiation, the
connectivity check, bitmaps, and partial-clone filters, and lets
ordinary Git tooling read published content. The one hazard is that
v2 `upload-pack` trusts its caller to scope objects, so that scoping
lives in Kairo's filter and its tests, including a fetch of a pooled
unpublished commit, which must fail.

Affected specs:

- `DECISIONS.md` §16 sub-decisions 5 and 6, and §21 sub-decisions 4–6.
- `PHASE_2_FEDERATION.md` — §2.1 endpoints, §2.3 `fed_bad_git_request`
  and `fed_busy`, §2.4 Git limits, slice 4, §4 (incremental fetch
  done), and §5 risk notes.
- `STORE.md` §4 — no `outgoing/` directory.
- `THREAT_MODEL.md` — pool scoping on the Git endpoint.

## 23. Actor Chains: `previous` on the Statement Envelope

Decision: the statement envelope gains a `previous` field. In a
**chained** kind, `previous` names the signer's current tips in the
statement's **chain**, so each actor's statements in a scope form a
hash-linked DAG. Every chained statement is its own head: the newest
one a node holds is the signer's attestation to everything before it.
A node holding a statement whose `previous` it lacks knows exactly
what is missing and whose it is. Withholding inside a chain therefore
becomes a detected gap instead of a silent `Indeterminate`
(`THREAT_MODEL.md` §5.15).

This realizes the **actor chains** of `CORE_LIBRARY.md` §9.2 and
`ACTORS.md` §7, which specified an `actor_seq` and a
`previous_actor_statement` on every statement but were never
implemented. It differs from them in three ways:
- There is no sequence number, because chain position is the DAG.
- Forks are allowed, and merged by naming several tips.
- The chain key is a scope (an object, the actor's key events, or a
  capability target), not just the object.

Alternatives considered (`PHASE_2_FEDERATION.md` §4.2):

- **Separate checkpoint statements** that commit to batches. These
  add one record per write, kept forever, to say what the data could
  say itself.
- **A single latest signed Merkle root** per actor and scope (AT
  Protocol, Hypercore, Certificate Transparency). This keeps O(1)
  state and allows proofs of membership and absence. But it needs one
  coherent writer per chain, while Kairo actors sign from several
  devices and from daemon session signing, and it gives no causal
  order. It remains the path to freshness and absence proofs, built
  over chain tips rather than replacing them (sub-decision 9).

Sub-decisions:

1. **Envelope.** The unsigned envelope is `type`, `version`, `actor`,
   `subject`, `created_at`, `previous`, `body`.
   - `previous` is a list of statement ids, ascending, deduplicated,
     at most 16. It is encoded in canonical bytes after `created_at`
     as a length-prefixed list of strings.
   - In JSON it is a required array. It is empty for unchained kinds
     and for a chain's first statement.
   - The envelope stays `kairo.statement.v1` and is amended in place,
     since nothing is deployed (as §18 did for `ActorTrust`). Every
     StatementId changes.
   - `ObjectGenesis` and `ActorGenesis` have no envelope and are
     unchanged.

2. **Chain key.** A chain is `(signer, scope)`, and a chained kind's
   scope comes from its body:

   | Kind | Scope |
   |------|-------|
   | `ObjectBranch`, `ObjectVersionTag` | `object <id>` (the body's object) |
   | `ActorCapabilityGrant` | the capability's scope: `object <id>` or `actor <id>` |
   | `ActorCapabilityRevocation` | the revoked grant's scope |
   | key events (rotation, revocation, emergency rotation and revocation, attestation key add and revocation, threshold change) | `keys` |

   - A revocation's scope is known only once its grant is held.
     Without the grant, its chain placement is `Indeterminate`, as its
     authority already is.
   - **Unchained kinds** must have an empty `previous`:
     - `ObjectRevision`, so that a revision no ref reaches stays local
       (§20) instead of becoming a required predecessor of the
       signer's next branch move;
     - `ActorTrust`, which is first-person, has its own `supersedes`
       chain, and never federates;
     - future kinds, unless their own decision chains them.
   - A new kind declares its chain rule when it is specified
     (`kairo-statement` `AGENTS.md`).

3. **What `previous` must name.** At signing time, the signer's
   current tips in that chain: its held chain statements that no other
   held chain statement names. Every entry must be a statement by the
   same signer, of a chained kind, in the same scope. Anything else
   makes the statement `Invalid` (`CORE_LIBRARY.md` §12.2, "claims a
   previous statement that belongs to another actor chain").
   - `previous` is about completeness and order. A branch's
     `supersedes` keeps its own meaning (which branch statement
     replaces which, possibly across actors, `CAPABILITIES.md` §6.2),
     and both fields can name the same statement.
   - A signer with more than 16 tips merges them across successive
     writes.

4. **Forks are normal.** Two devices that each write from the same
   tips leave two tips. The next write from either, after syncing,
   names both and merges them. Nothing is invalid in between, and the
   chain's set is everything under all tips. A signer cannot be forced
   into contradiction by a race, unlike `CORE_LIBRARY.md`'s earlier
   strictly increasing `actor_seq`.

5. **Every write path sets it.**
   - **CLI and daemon session signing** (§15) read the signer's tips
     from the store and sign with them.
   - **Signed-mode clients** (§13) read them from the daemon API
     (`GET /api/v1/actors/{id}/chains`, every scope and its tips). A
     statement that named all of its scope's tips becomes that scope's
     only tip, so a client writing in sequence can chain from what it
     just wrote. A client with stale tips creates a fork, which is
     merged later.
   - **Imports sign nothing.** Bundle import and federation pull only
     store other actors' statements.

6. **Completeness.** For one signer and scope, the tips are the held
   chain statements no held chain statement names.
   - **Completeness** is `Complete` (every `previous` of every held
     statement in the chain is held) or `Incomplete { missing }` (the
     named-but-absent ids). An absent statement's own predecessors are
     unknown until it arrives, so completeness is re-evaluated after
     each fetch.
   - **Revoked keys.** A statement signed with a key revoked at its
     `created_at` is stored and flagged (§16). Its `previous` is
     ignored for completeness, so a stolen key can't name ids that
     don't exist and pin a chain as permanently `Incomplete`. It still
     counts as held when a valid statement names it.

7. **Order.** Within one chain, `previous` is the order: a statement
   comes after everything it names, transitively. Resolution and
   ingest use chain order first, then `created_at` and statement id
   as tiebreaks only between statements the DAG leaves unordered
   (`CORE_LIBRARY.md` §15.2). In particular, key events are now
   causally ordered among themselves.

   Still open: an object-chain statement's key validity is judged by
   `created_at` against the `keys` chain. Letting it also name the
   signer's `keys` tip, as Keybase links carry a sequence number, would
   make "signed after this revocation" causal and defeat backdating
   with a stolen key. That is a separate decision.

8. **Federation and verification.**
   - **The closure is unchanged.** Every chained kind is already in it:
     branch and tag chains and authority are served whole (§16
     sub-decision 4), and key events come with the actor listing.
   - **`actor`-scope grants** are chained but don't federate today.
     Their chains are only evaluated locally.
   - **Ingest** orders each chain topologically.
   - **The pull report** lists each incomplete signer and scope with
     its missing ids. The pull still succeeds, keeping what verified.
     `kairo fetch --require-complete` turns any `Incomplete` into a
     failure (exit 11).
   - **`kairo verify object`** reports completeness for every chain
     the result consults:
     - the genesis creator's and grantors' chains on the authority
       path;
     - the chains of the resolved branch and tag signers;
     - the `keys` chain of every signer whose signature is checked.

     `Incomplete` on any of these makes the result `Indeterminate`,
     naming the signer, the scope, and the missing ids. So does a
     contradiction (a statement whose `previous` names something
     outside its chain): that statement is `Invalid`, and the result
     can't rule out that it was the one that mattered. The grantors
     consulted are those of object-scoped grants to a consulted signer,
     one hop. An incomplete
     chain the result doesn't consult is reported and changes nothing.
     The daemon API exposes the same, for the web client.

9. **What this does not prove: freshness.** Holding a statement
   proves its predecessors exist, and a peer can't hide what a held
   statement names. A peer can still serve an older, internally
   consistent state, because nothing signed only by the actor shows
   its newest statement. This is the same limit as Radicle's signed
   refs.

   Freshness needs parties other than the signer. The design to
   follow is Certificate Transparency's, also used by the Go checksum
   database: nodes sign a Merkle root over the chain tips they hold
   (the "latest signed root" alternative above, built on the tips
   rather than replacing them), and independent nodes co-sign roots
   as witnesses, so a split view or a stale serve becomes detectable.
   That is the signed form of `OBJECT_STORE.md`'s "observed log root"
   and node-published "Log Checkpoints", and it waits on node
   authentication (`PHASE_2_FEDERATION.md` §4 item 3).

Rationale: per-statement signatures prove each record is genuine but
say nothing about what is missing. Selective withholding of a
revocation is the federation attack with the worst consequence:
accepting a compromised key's statements, or authority that was
withdrawn. Putting `previous` in the envelope makes every chained
statement a commitment to its signer's history in that scope:
- no extra records, and no growth beyond the statements themselves;
- causal order within each chain, in place of signer-asserted
  timestamps;
- forks from several devices tolerated by construction.

One envelope field and one verifier path serve every chained kind,
current and future.

Affected specs:

- `STATEMENTS.md` §2 (envelope) and §4 (each kind's chain rule) — and
  every `schemas/canonical/*-v1.md` "Canonical Envelope Fields"
  section, at implementation.
- `CORE_LIBRARY.md` §9.1, §9.2, §13.1 step 7, §15.2 — actor chains as
  `previous`; `actor_seq` and `previous_actor_statement` removed.
- `ACTORS.md` §6 and §7 — the same.
- `PACKAGE.md` and `WORKSPACE.md` — records preserve `previous`; a
  write sets it.
- `API.md` — chain tips per actor and scope, and per-actor
  completeness.
- `PHASE_2_FEDERATION.md` §4 item 1 — decided here.
- `THREAT_MODEL.md` §5.15 — the fix, with the freshness limit.
- `GLOSSARY.md` — actor chain, chain tip.

## 24. Statement Batch Fetch

Decision: a pull fetches statements in batches. `POST
/federation/v1/statement-batch` takes up to 256 statement ids and
returns those the node serves, each with the exact JSON the store
holds. Per-record verification is unchanged: the client checks
fixity, section, and signature on every statement as before. A pull
of N statements costs about N/256 requests instead of N
(`PHASE_2_FEDERATION.md` §4 item 2; Radicle syncs a repository in one
fetch, §4.2).

Sub-decisions:

1. **Shape.**
   - **Request:** `{ "schema": "kairo.fed.statement-batch-request.v1",
     "ids": [ ... ] }`, with 1 to 256 ids and unknown fields refused.
   - **Response:** `{ "schema": "kairo.fed.statement-batch.v1",
     "statements": [ { "id": ..., "statement": <stored JSON> } ] }`,
     in request order, each id at most once. The `statement` value is
     the stored bytes embedded verbatim, not re-serialized, so the
     client verifies exactly what a per-record `GET` would return.
   - A malformed request is `400 fed_bad_batch`.

2. **Absence stays ambiguous.** An id the node doesn't hold and an id
   outside every published closure are both simply left out (§16
   sub-decision 3). The client fetches anything left out through the
   per-record endpoint, so a listed-but-unserved statement fails the
   pull exactly as before, with the same `404 fed_not_found`.

3. **Read-only in spirit.** The batch is a `POST` only because the id
   list doesn't fit a URL. It reads, like the Git `upload-pack` RPC
   (§22). These two are the federation server's only `POST`s.

4. **Advertised, with fallback.** The node document's `supports`
   gains `statement-batch`. A client uses the batch only when the
   node lists it, and otherwise fetches per record. Responses aren't
   cacheable the way per-record `GET`s are; a caching proxy keeps
   working for nodes or clients that use per-record fetches.

5. **Bounds.**
   - **Server:** the request body is capped at 64 KiB.
   - **Client:** requests at most 256 ids at a time and caps each
     response at `max_batch_bytes` (default 32 MiB), failing the pull
     with `TooLarge` beyond it, like any oversize response.
   - **Blobs stay per record.** They are few, large, and binary, and
     batching them would only make responses unbounded.

Affected specs:

- `PHASE_2_FEDERATION.md` §2.1 (endpoint), §2.3 (`fed_bad_batch`),
  §2.4 (`max_batch_bytes`), §4 item 2.
- `FEDERATION.md` — the endpoint list.

## 25. Node Authentication

Decision: nodes authenticate as their node actors in two ways. First,
the **node document is signed**, so a pull knows which node actor
answered at a URL. Second, every **request to an endpoint that acts on
who is asking** (push, and later discovery) carries a `KairoNode1`
signature. Both use the node actor's active operational key and
resolve it through the actor's key chain, so key rotation and
revocation work for nodes as for any actor (§16 sub-decision 1). The
§16 sub-decision 9 caveat, that `node_actor` is only a claim, ends
here.

Transport-level authentication (mutual TLS or Noise keyed to the node,
as Radicle does) was rejected. It would break the deployment §16
sub-decision 6 settled on, where TLS terminates at a reverse proxy,
and it would need key handling outside the actor model.

Sub-decisions:

1. **Signed node document.**
   - **Fields.** The document gains `signed_at` and `signature`. The
     signature is over the document's canonical bytes without the
     signature field, under the domain `kairo.fed.node-document.v1`.
   - **Binding.** The document already carries `base_url`, so the
     signature binds "this URL serves node actor X". A pull refuses a
     document whose `base_url` differs from the URL it pulled from
     (after normalization), or whose signature doesn't verify against
     the node actor's key active at `signed_at`.
   - **Freshness.** A node re-signs its document at startup, and
     `signed_at` gives an observer a recency signal.
   - **The pull report.** It says `peer_actor` is *authenticated*, not
     claimed. A `node_actor` pin (§17) then pins an authenticated
     identity rather than a trust-on-first-use claim.

2. **The node actor's key state is served.** A node always serves its
   own node actor's `ActorListing`, with its key event ids, at
   `/federation/v1/actors/{node_actor}`, whether or not any closure
   names it. A peer pulls that listing before verifying a node
   signature it can't yet verify, so a rotated or revoked node key is
   known.

3. **`KairoNode1` request signatures**, modeled on `KairoActor1` (§14).
   - **Header.**

     ```http
     Authorization: KairoNode1 node=<actor-id>, key_id=<key-id>,
       timestamp=<i64>, nonce=<32-hex>, signature=<base64>
     ```

   - **Signing string.**

     ```text
     kairo-federation-request-v1\n
     <method>\n
     <receiver base URL>\n
     <path-and-query>\n
     <body-sha256-hex>\n
     <timestamp>\n
     <nonce>\n
     ```

     The receiver's normalized base URL is signed, which `KairoActor1`
     didn't need on loopback. A request signed for one node can't be
     replayed at another.

   - **Replay.** A ±300-second window allows for clock skew across the
     internet. On top of it, the receiver keeps a cache of `(node,
     nonce)` pairs seen within the window. The cache lives in memory,
     so a replay is possible only within one window across a restart,
     which is accepted for v1.

   - **Refusal.** A missing or invalid signature, an unknown node
     actor, a non-`node` actor kind, or a revoked key is `401
     fed_unauthenticated`. Authorization (does this node hold
     `federation:push`?) is a separate `403`.

4. **Reads stay anonymous.** Pull endpoints neither require nor check
   `KairoNode1`. Published records verify themselves, and publishing is
   the access decision (§16). A signature on a read is ignored, not
   refused.

Affected specs (sub-decisions 1–3 implemented: `PHASE_2_FEDERATION.md`
§2.2 and §2.3, `FEDERATION.md`, `THREAT_MODEL.md` §5.18–§5.19, and
`schemas/canonical/node-document-v1.md`):

- `PHASE_2_FEDERATION.md` — §2.1 (the node document, the always-served
  node actor listing), §2.3 (`fed_unauthenticated`), §4 item 3.
- `DECISIONS.md` §16 sub-decision 9 — superseded by sub-decision 1
  here.
- `FEDERATION.md` — the node document shape.
- `THREAT_MODEL.md` — node impersonation.

## 26. Push

Decision: there are two kinds of push, chosen by role. Both land in a
receiver's **inbox** and are ingested through the same verifier a pull
uses, so the federation server still never writes the store itself.

- **Announce** (node to node): an authenticated `POST
  /federation/v1/announce` says "these objects have new state at my
  base URL". The receiver pulls them from the announcer through the
  ordinary pull path.
- **Upload** (a client node to its home node): an authenticated `POST
  /federation/v1/push` carries a Kairo bundle of the object. It serves
  a pusher the receiver can't reach, such as a laptop.

A receiver accepts either only from a node it trusts with
`federation:push` (§18), and only for objects on its accept list.

Sub-decisions:

1. **Who may push.** The request carries a valid `KairoNode1` signature
   (§25), and the receiving node's actor has an `ActorTrust` in scope
   `federation:push`, decision `trusted`, about the pushing node actor.
   There is no fallback from `general` trust (§18). Otherwise the
   request is `403 fed_not_trusted`.

2. **The accept list.** `[[federation.accept]]` entries in
   `config.toml` (§17), each with:
   - `object`: an object id;
   - `from` (optional): a node actor id. Absent means any node trusted
     with `federation:push`;
   - `publish` (default `false`): serve the object onward after
     ingest. `true` makes the node a public mirror of the object,
     `false` a private backup.

   A push for an object no entry covers is `403 fed_not_accepted`. The
   check runs when the request arrives and again at ingest, so a
   config change in between takes effect.

3. **The inbox.** `<store>/federation/inbox/` holds accepted
   announcements (a small JSON record each) and uploaded bundles
   (written to a temporary name, then renamed into place). This is the
   federation server's one write, amending §16 sub-decision 6 the way
   §21 briefly did for the pack cache. The server doesn't open the
   store for writing, and nothing in the inbox is trusted until
   ingested.
   - **Bounds.** Uploads are capped at `max_push_bytes` (default
     1 GiB). Each node may hold at most `max_inbox_entries` (default
     64) unprocessed entries, after which pushes get `503 fed_busy`
     with `Retry-After`.

4. **Ingest.** `kairo federation receive` processes the inbox. The
   daemon's `FederationService` (`PHASE_2_FEDERATION.md` §4 item 5)
   takes this over when it exists.
   - **An announcement** becomes a pull of each announced object from
     the announcer's signed `base_url` (§25 sub-decision 1), with the
     announcer's node actor as the expected pin.
   - **An upload** is ingested as a bundle, with the same per-record
     verification a pull does: fixity, section, signature, actor
     chains, and Git through quarantine. This means bundle import's
     signature verification (tracked in `PHASE_2.md` §11) is done as
     part of push. Records are stored with provenance naming the
     pushing node.
   - **After a successful ingest,** an entry with `publish = true`
     adds the object to the publish list. Rejected records are
     reported, and the entry is removed either way, with the outcome
     logged like a pull (`federation/pulls.json`).

5. **The pusher's side.**
   - `kairo push --object <id> --to <url>` uploads a bundle of the
     object's published closure.
   - `kairo federation announce --object <id> --to <url>` sends an
     announcement.

   Both sign with the local node actor's key, so a pushing node needs
   a node actor in its keystore.

6. **Uploads send the delta.** *Replaces "incremental upload is later
   work", decided when slice 5 was built.* An upload is two signed
   requests, as a Git push is: the receiver says what it has, and the
   pusher sends the rest.
   - **Plan.** `POST /federation/v1/push/plan` carries the pusher's
     inventory of the object's published closure: `{ schema:
     "kairo.fed.push-plan-request.v1", object, inventory: { statements,
     authority, actors: [{ actor, key_events }], blobs, commits } }`,
     the same ids a pull would list, and the typed Git revisions the
     revisions name.
     After the announce checks (signature, `federation:push` trust,
     an accept entry for the object), the receiver answers `{ schema:
     "kairo.fed.push-plan.v1", genesis, actors, statements, blobs,
     have_commits }`: whether it lacks the object genesis, the actors
     whose genesis it lacks, the statements (object statements,
     authority, and key events) and blobs it lacks, and which of the
     commits it already has. A plan request is at most 8 MiB and
     100 000 ids (`fed_bad_push`).
   - **Upload.** `POST /federation/v1/push?object=<id>` carries a
     closure bundle (sub-decision 8) of just what the plan asked for.
     Its Git pack is thin: built with the receiver's `have_commits`
     excluded, so shared history isn't resent, and completed against
     the receiver's pool on ingest. The object is in the signed query
     so every check but the signature runs before the body is read: a
     stranger can't make the receiver write anything. The body streams
     into the inbox up to `max_push_bytes` (`413 fed_too_large` past
     it) and is deleted if the signature over its SHA-256 then fails.
   - **The plan is advisory.** The receiver may have gained or lost
     records in between. Ingest verifies against whatever the store
     holds then, and a listed record that is neither held nor in the
     bundle fails the ingest as an unserved record fails a pull.
   - **What the plan reveals.** Which of the pusher's own ids the
     receiver holds, to an authenticated node it trusts with
     `federation:push`. Nothing about records the pusher didn't name.
   - Chain tips (§23) could later shrink plans for large objects:
     the receiver advertises its tips and the pusher sends what follows.
     Revisions and blobs are unchained and forks make "what follows"
     fuzzy, so the id inventory stays the exact form.

7. **Learning a pusher's keys, and the announcement's shape.** Added
   when slice 4 was built.
   - A receiver authenticates only node actors whose genesis and key
     events it holds (§25 sub-decision 3), and the server never
     writes them, so it never fetches anything on a request's
     behalf. The operator runs `kairo federation learn --from <url>`,
     which authenticates the peer by its signed node document and
     stores its node actor's key state under the same policy as
     `kairo fetch` (listed peer or `--allow-peer`, pin honored).
     Re-learning picks up a rotated key; until then the new key is
     refused as unknown.
   - A pusher that is never reachable (a laptop) is learned from a
     file instead: `kairo actor export --actor <id> --out <path>`
     writes `{ schema: "kairo.fed.actor-export.v1", actor, genesis,
     key_events: [{ id, statement }] }`, the genesis and every key
     event exactly as stored, and `kairo federation learn --file
     <path>` ingests it through the pull verifier's actor step: the
     genesis must derive `actor` and declare kind `node`, and each key
     event must verify, in chain order, against the keys before it.
     The file is self-verifying, so it can travel any way; what the
     operator must check out of band is the actor id, which is what
     the `federation:push` trust grant names. `--node-actor <id>`
     refuses a file for any other actor.
   - The announcement body is `{ schema: "kairo.fed.announce.v1",
     base_url, objects }`, with 1 to 256 distinct objects. `base_url`
     is where the receiver pulls from. It is the announcer's word,
     but the pull pins the announcer's node actor and refuses a
     document for another URL, so a wrong URL fails the pull rather
     than ingesting anyone else's data.
   - A push for several objects is refused whole if any isn't
     accepted, naming the ones that aren't.
   - The server reads the accept list and push limits from
     `<store>/config.toml` at startup; `receive` re-reads them.

8. **The closure bundle.** An upload is a new bundle type, the
   transfer form of a published closure: exactly what a pull fetches,
   including authority and every actor's key events, which object
   bundles deliberately leave out (`PACKAGE.md` MVP slice). That makes
   a push verify as a pull does even when the pusher can't be reached
   afterwards, as a laptop usually can't. It exposes nothing new: the
   pusher's node already serves all of it to anyone who pulls.
   - **Layout.** An uncompressed POSIX tar (`PACKAGE.md` §5's
     reference archive) of:

     ```text
     manifest.json                 kairo.closure-bundle.v1
     genesis.json                  the object genesis, if included
     actors/<actor-id>.json        actor geneses included
     statements/<statement-id>.json
     blobs/<blob-id>
     git/pack                      a (thin) Git pack, if any
     sources/<source-id>/...       pinned sources' closures (§38
                                   sub-decision 10)
     ```

   - **Manifest.** `{ schema: "kairo.closure-bundle.v1", object,
     created_at, inventory: { statements, authority, actors: [{
     actor, key_events }], blobs, commits }, git: { format, pack } }`.
     The inventory is the whole closure as the pusher holds it; the
     files are the part sent. Nothing in the manifest is trusted: the
     inventory plays the object listing's role, and every file is
     checked as the pull checks the record it stands for.
   - **Unpacking.** Only regular files at exactly these paths, named
     by valid ids; anything else (links, devices, `..`, absolute
     paths, a duplicate) refuses the bundle.
   - **Ingest** is the pull verifier reading from the unpacked bundle
     instead of HTTP: the same order, fixity, section, signature,
     actor-chain, and Git quarantine checks. Provenance and the pull
     log name the pusher as `push:<node-actor-id>` in place of a URL.

Rationale: the inbox keeps one ingestion path (the pull verifier) for
everything a node accepts from outside, whether pulled, announced, or
uploaded. It also keeps the network-facing server out of the store.
Accepting only from `federation:push`-trusted nodes, and only for
listed objects, means a node mirrors what its operator chose, never
what a peer volunteers.

Affected specs (all sub-decisions implemented: `PHASE_2_FEDERATION.md`
§2.1 and §2.3, `CLI.md` §15 and §25, §17 above, `PACKAGE.md`,
`PHASE_2.md` §11, and `THREAT_MODEL.md` §5.20):

- `PHASE_2_FEDERATION.md` — §2.1 (the announce and push endpoints),
  §2.3 (`fed_not_trusted`, `fed_not_accepted`), §4 items 3 and 5.
- `DECISIONS.md` §16 sub-decision 6 (the inbox write) and §17 (the
  `[[federation.accept]]` table, `max_push_bytes`,
  `max_inbox_entries`).
- `PHASE_2.md` §11 — bundle-import verification, done here.
- `CLI.md` — `kairo push`, `kairo federation announce|receive`.
- `THREAT_MODEL.md` — push flooding and unwanted mirroring.

---

## 27. Trust Transport and Forget

Decision: a node serves `ActorTrust` only for (truster, scope) chains
its operator publishes, and another node fetches one chain at a time
through the pull verifier. Fetched opinions stay their truster's:
resolution is always per truster (§18), so importing alice's opinions
never changes what this node trusts. `forget` deletes a non-local
truster's opinions outright. This fills the gap §16 sub-decision 4
left ("not `ActorTrust`") and is the trust transport discovery needs
(§18).

Sub-decisions:

1. **Publishing is opt-in, per (truster, scope).** `kairo trust
   publish --by <actor> --scope <scope>` adds the chain to
   `<store>/federation/published-trust.json`; `kairo trust unpublish`
   removes it. Nothing is published by default, not even the node
   actor's federation opinions: publishing is the access decision, as
   for objects (§16 sub-decision 3). A published chain is every
   `ActorTrust` the truster signed in that scope, about every trusted
   actor, including withdrawals, so a peer never sees a stale "trusted"
   whose withdrawal was held back.

2. **Serving.**
   - `GET /federation/v1/trust` lists published chains: `{ schema:
     "kairo.fed.published-trust.v1", chains: [{ by, scope }] }`.
   - `GET /federation/v1/trust/{by}?scope=<scope>` is `{ schema:
     "kairo.fed.trust-listing.v1", by, scope, statements: [ids] }`;
     an unpublished chain is `404 fed_not_found`, like an unpublished
     object.
   - The statements are served by the existing statement endpoints
     and batch, and the truster's actor listing (genesis and key
     events) is served, so the opinions verify.

3. **Fetching one chain.** `kairo trust fetch --by <actor> --scope
   <scope> --from <url>` connects and authenticates the peer as a
   pull does, fetches the truster's genesis and key events, then the
   chain's statements in order. Each must be an `ActorTrust` signed by
   `--by` in `--scope`; anything else is refused as out of section.
   Records get provenance. Policy is `kairo fetch`'s (§17 sub-decision
   4): listed peer or `--allow-peer`, pin honored, `--offline` refuses.

4. **Importing is not trusting.** A fetched opinion is stored under
   its truster like any statement, so `latest_trust(alice, bob,
   scope)` answers with alice's opinion and nothing else changes.
   `kairo trust list`, `show`, and `history` label opinions whose
   truster has no key in the local keystore as `imported`. `verify
   object` and the web inspector keep showing only the local
   truster's opinion until discovery defines how foreign opinions are
   used.

5. **`forget`.** `kairo trust forget --by <actor> [--scope <scope>]`
   deletes every `ActorTrust` that actor signed (in that scope),
   with their index entries and provenance, and unpublishes the
   chains. It refuses an actor with a key in the local keystore: those
   are this node's opinions, withdrawn with `kairo trust withdraw`.
   It is deletion only. A later explicit `trust fetch` can bring them
   back; a background sync (`FederationService`) will need its own
   "don't follow" list, decided with it. The truster's genesis and key
   events stay, since other records may need them.

Rationale: trust is speech, and the per-truster resolution §18
settled already keeps imported speech from becoming local belief, so
transport needs no new trust semantics, only an access decision
(publishing) and a verified path (the pull verifier). Fetching a
single chain matches how both resolution and discovery read trust and
keeps a fetch from pulling opinions nobody asked about.

Affected specs:

- `PHASE_2_FEDERATION.md` — §2.1 (the trust endpoints), §4 item 4.
- `CLI.md` — `kairo trust publish | unpublish | fetch | forget`, and
  the `imported` label.
- `schemas/canonical/actor-trust-v1.md` — "local in MVP" now says how
  trust is published and fetched.
- `PHASE_2.md` §4 and §12 — trust transport and `forget`.

---

## 28. FederationService: Follows and Background Sync

Decision: a node keeps current only what its operator **follows**: an
object, or a (truster, scope) trust chain. A sync engine in
`kairo-federation-service` pulls every follow from every listed peer,
and processes the inbox, through the same verifier as `kairo fetch`.
The daemon runs it on timers; the CLI runs the same engine on demand.
On-demand fetch when a read finds data missing (`DAEMON.md` §9) waits
for discovery. This is `PHASE_2_FEDERATION.md` §4 item 5.

Sub-decisions:

1. **Follows are store state.** `kairo federation follow --object
   <id>` or `--trust-by <actor> --scope <scope>`, and `unfollow`, edit
   `<store>/federation/follows.json`, like the publish list (§16
   sub-decision 3); the CLI never edits `config.toml` (§17
   sub-decision 2). `kairo federation status` lists follows with their
   last result. Nothing is fetched in the background unless followed.

2. **Sources: every listed peer.** A sync of one follow tries each
   `[[federation.peers]]` entry, with its pin, and treats `404
   fed_not_found` as "this peer doesn't publish it", not a failure. A
   follow is current if some peer served it. Pulling from all of them
   is the multi-peer defense against withholding (§16 sub-decision 4);
   peers learned through discovery join later, under §18's rules.

3. **One engine, two drivers.**
   - The crate `kairo-federation-service` holds the engine, with no
     scheduler of its own: *sync these follows once* and *process the
     inbox once*. `kairo federation receive` moves into it.
   - The daemon (`DAEMON.md` §5.3) runs follows on an interval
     (`[federation.sync] interval_secs`, default 900) and the inbox
     every `inbox_secs` (default 30), so announcements and uploads are
     received without a manual `receive`. At most one run per follow
     at a time, and at most `max_concurrent` (default 2) pulls; pulls
     run on the blocking pool.
   - `kairo federation sync [--object <id> | --trust-by <actor>
     --scope <scope>]` runs now, directly, and `POST
     /api/v1/federation/sync` asks the daemon to.
   - With `[federation] enabled = false` neither runs (`DAEMON.md`
     §10.1).

4. **Results are recorded per follow.** The last attempt and the last
   success, each with the peers tried and per-peer outcome (fetched,
   already held, not published, failed with reason, incomplete
   chains), go in `follows.json`. `GET /api/v1/federation/status`
   serves follows and results; the task model (`DAEMON.md` §14) is an
   in-memory record of runs.

5. **Forgotten trust stays forgotten.** `kairo trust forget` also
   removes matching trust follows, so background sync never brings a
   forgotten chain back; only a new `follow` does. No separate deny
   list.

6. **On-demand fetch is deferred.** `DAEMON.md` §9 needs to know who
   holds missing data, which is discovery (§4 item 6). Until then an
   incomplete object reports its gaps, and following it fills them on
   the next sync. Reads stay network-free.

Rationale: following is the operator's choice of what to hold, as
publishing is the choice of what to serve, so the two are parallel
store state. Polling every listed peer needs no new protocol and gets
the withholding defense for free. Keeping the engine scheduler-free
means the CLI and daemon can't drift apart.

Affected specs (all implemented: `CLI.md` §25, `PROJECT_LAYOUT.md`,
`DAEMON.md` §5.3 and §15, `API.md` §14.2, and §17):

- `PHASE_2_FEDERATION.md` §4 item 5 — the slice plan.
- `DAEMON.md` §5.3, §10, §15 — `FederationService` in the daemon,
  `[federation.sync]`.
- `DECISIONS.md` §17 — the `[federation.sync]` keys.
- `CLI.md` §25 — `follow`, `unfollow`, `sync`; `receive` unchanged in
  behavior.
- `API.md` — `GET /api/v1/federation/status`, `POST
  /api/v1/federation/sync`.
- `PROJECT_LAYOUT.md` — `kairo-federation-service` is created.

---

## 29. Discovery, Step One: Inventories and Hints

Decision: each node signs an **inventory**, the published objects it
serves at its base URL, and peers fetch inventories on the sync
schedule and keep the newest per node as **hints**. A node also relays,
one hop, the inventories it learned from nodes it trusts with
`federation:index`. Hints answer "who has object X?" and are never
permissions (§18 sub-decision 2): sync pulls from a hinted holder only
when the operator turns that on, and only for holders vouched for by
`federation:index` trust. This is `PHASE_2_FEDERATION.md` §4 item 6's
first step; token indexes and the query protocol come later.

Sub-decisions:

1. **The inventory is a signed document, not a statement.** `{ schema:
   "kairo.fed.inventory.v1", node_actor, base_url, objects, signed_at,
   signature }`, signed like the node document (§25 sub-decision 1)
   with the node actor's active key over canonical bytes under the
   domain `kairo.fed.inventory.v1`. `objects` is the publish list, so
   it reveals nothing a pull wouldn't. The newest `signed_at` per node
   replaces older ones; history isn't kept, because a hint is only a
   hint. The node signs it when its published view is rebuilt.

2. **Serving.**
   - `GET /federation/v1/inventory`: this node's own inventory.
   - `GET /federation/v1/inventories`: the inventories this node
     relays (sub-decision 4), each with the signing node's actor
     export (`kairo.fed.actor-export.v1`, §26 sub-decision 7), so a
     receiver can verify a node it has never contacted.
   - Both are anonymous reads (§25 sub-decision 4). Nodes advertise
     `inventory`.

3. **Spreading by pull.** `FederationService` fetches inventories from
   every `[[federation.peers]]` entry and every `[[federation.bootstrap]]`
   entry (§18 sub-decision 1: a bootstrap node is asked where others
   are, and that grants it nothing else) on the follow interval.
   `kairo federation discover` runs it now. Each inventory must
   verify against its node's key state: a direct one after the
   source's node document authenticates the source (§25), a relayed
   one against its bundled actor export, through the pull verifier's
   actor step. A relayed inventory whose `base_url` doesn't serve that
   node fails only when it's used (sub-decision 5).

4. **Relaying is one hop, by trust.** A node relays an inventory only
   if its own node actor trusts the inventory's node, or the node it
   learned it from, with `federation:index`. Relayed inventories keep
   their own signatures and are never re-signed. A receiver that
   learned an inventory by relay records the relaying node; it does
   not relay relayed inventories again, so reach is bounded at one
   hop past each node's own peers. Bootstrap nodes are simply nodes
   that relay many.

5. **Hints are state; using them is opt-in.**
   - `<store>/federation/hints.json` keeps, per node actor, the
     newest verified inventory, who it was learned from (the
     immediate peer, §16 sub-decision 8), and when. Hints older than
     `[federation.discovery] hint_ttl_secs` (default 604 800, a week)
     by `signed_at` are ignored and dropped on the next write.
   - `kairo federation holders --object <id>` lists the hinted holders.
   - With `[federation.discovery] use_hints = true` (default
     `false`), syncing a followed object also tries each hinted holder
     that this node's actor trusts directly with `federation:index`,
     or whose hint was relayed by a node it so trusts. *Amended when
     slice 4 was built:* the sync engine runs without a node identity
     (the CLI and the daemon both drive it), so "this node's actor" is
     named in config, `vouched_by = "<actor-id>"`, which `use_hints`
     requires. The pull
     goes to the hint's `base_url` with the holder's node actor
     pinned, so a hint pointing somewhere else fails safely. Listed
     peers are still tried first.

Rationale: hints are cheap, signed claims that age out, which fits a
signed document better than an append-only statement. Pull on the
existing schedule needs no new authenticated endpoint. One-hop relay
under `federation:index` trust is the bounded, opt-in propagation §18
sub-decision 5 asks for, and keeping use of hints opt-in and
trust-gated means discovery never turns into "pull from whoever a
stranger names".

Affected specs:

- `PHASE_2_FEDERATION.md` §4 item 6 — the slice plan.
- `FEDERATION.md` — holder discovery's first form.
- `DECISIONS.md` §17 — `[[federation.bootstrap]]`,
  `[federation.discovery]`.
- `CLI.md` §25 — `kairo federation discover`, `holders`.
- `THREAT_MODEL.md` — hint poisoning.
- `schemas/canonical/` — `inventory-v1.md` (implemented, with the
  endpoint, in slice 1).
- Slice 2 implemented `hints.json`, `[[federation.bootstrap]]`,
  `[federation.discovery]`, `kairo federation discover`, and
  `holders`; slice 3 the relay (`GET /federation/v1/inventories`);
  slice 4 the daemon's discovery runs, `use_hints` with `vouched_by`,
  `FEDERATION.md`, and `THREAT_MODEL.md` §5.22.

---

## 30. On-Demand Fetch

Decision: a node can **acquire** an object from wherever it is held:
the sources sync uses (listed peers, then, with `use_hints`, hinted
holders vouched for by `federation:index` trust), with those that
served the object before tried first. Acquisition is always available
explicitly, and reads that find data missing may queue one when the
operator opts in. A read never waits for it. This is `DAEMON.md` §9,
deferred by §28 sub-decision 6 until discovery could say who holds
what (§29).

Sub-decisions:

1. **Sources and order.** The allowed sources are exactly §28's and
   §29's: every `[[federation.peers]]` entry, then, with
   `[federation.discovery] use_hints`, each fresh hinted holder the
   `vouched_by` actor trusts with `federation:index` (directly or as
   its relay), pinned. Among them, the ones the object's provenance
   names (§16 sub-decision 8) come first, most recently seen first,
   since they held it before. Provenance never adds a source: a peer
   that served the object once under `--allow-peer` and isn't listed
   or vouched for isn't tried. Provenance entries that aren't URLs
   (`push:<node>`, `file:<path>`) are skipped. The acquisition stops at
   the first source that serves the object.

2. **Explicit acquisition.** `kairo fetch --object <id>` without
   `--from`, and `POST /api/v1/federation/fetch` with `{ object }`,
   acquire any object a source publishes, including one this node has
   never held. A pull is incremental, so for a held object only the
   missing parts move. An acquired object isn't followed; `follow`
   does that.

3. **On read, opt-in.** With `[federation.on_demand] on_read = true`
   (default `false`), a daemon read that finds an object missing (a
   `404` for its genesis) or incomplete (`verify-object` reporting
   incomplete chains or missing records) queues an acquisition of it.
   That includes objects never held: following a link to one fetches
   it. Off by default, because a read then tells peers what this node
   is looking at, and can bring objects into the store.

4. **Reads never wait.** A read answers at once from what is held. It
   says what it did with an `acquisition` field on `verify-object`, or
   a `Kairo-Acquisition` response header on a `404`: `queued`,
   `running`, `cooling_down`, `no_sources`, or `disabled`. A later read
   sees the result. The daemon must not fabricate completeness
   (`DAEMON.md` §9): what's still missing stays indeterminate.

5. **Bounds.** At most one acquisition per object at a time, sharing
   `[federation.sync] max_concurrent` with sync. After an attempt, a
   read-triggered acquisition of the same object waits
   `[federation.on_demand] cooldown_secs` (default 300); explicit
   requests skip the cooldown but not the single flight. Results are
   recorded as `fetch` runs in `GET /api/v1/federation/status`.

Rationale: acquisition reuses the sync sources so there is one rule for
where data may come from; provenance only reorders them. Keeping read
triggers opt-in respects that a read becoming network traffic is a
privacy and storage decision the operator makes. Never waiting keeps
reads fast and bounded whatever the peers do.

Affected specs:

- `DAEMON.md` §9 — implemented as above.
- `API.md` — `POST /api/v1/federation/fetch`, the `acquisition` field,
  the `Kairo-Acquisition` header.
- `CLI.md` §15 — `kairo fetch` without `--from` (implemented in
  slice 1, with the engine's `acquire`).
- Slice 2 implemented `POST /api/v1/federation/fetch`,
  `[federation.on_demand]`, the queue, single flight, and cooldown;
  slice 3 the read triggers, the `acquisition` field, and the
  `Kairo-Acquisition` header (`API.md`, `DAEMON.md` §9).
- §17 — `[federation.on_demand]`.
- `PHASE_2_FEDERATION.md` §4 — the slice plan.

---

## 31. `git-remote-kairo`

Decision: a Git remote helper, `git-remote-kairo`, makes `git clone
kairo://<host>/<object-id>` and `git fetch` work against a Kairo node,
and Git only ever sees history Kairo verified. The helper first runs
an ordinary Kairo pull of the object into the user's store, then
answers Git from that store's Git cache. Nothing goes from the peer to
Git directly. This is `PHASE_2_FEDERATION.md` §4 item 7, after
Radicle's `git-remote-rad`.

Sub-decisions:

1. **Verify, then serve locally.** On `list`, the helper pulls the
   object from `https://<host>` (`http://` for a loopback host, as
   `kairo fetch` allows) with the same checks as `kairo fetch`: the
   node is authenticated by its signed document (§25), every record
   verifies, chains are checked (§23), and Git commits enter the cache
   only through the quarantine with their full closures (§21, §22).
   `fetch` then copies the requested commits from the cache's
   per-object repository into the clone. The content check of `kairo
   verify object` (manifest in commit) is not run; the bar is the
   pull's.

2. **The user's store.** The pull goes into the default store (`$HOME/.kairo`,
   or `KAIRO_STORE`), so records, provenance, and the Git cache are
   shared with `kairo` and the daemon, and a second clone reuses them.

3. **Refs.**
   - The object creator's branches are `refs/heads/<name>`, with
     `head` (when present) as `HEAD`; the creator's version tags are
     `refs/tags/<version>`.
   - Every other actor's branches are `refs/kairo/actors/<actor-id>/<name>`,
     visible but never mistaken for the creator's.
   - A ref is listed only when its signed revision names a `git:`
     commit present in the cache. Revisions without Git history, or
     whose commit didn't arrive, are left out, and the helper says so on
     stderr.
   - `list` reports the object's Git format (`:object-format`), and
     refuses a clone in the other format.

4. **Policy.** The host must be a listed `[[federation.peers]]` entry,
   with its pin honored, unless `git config kairo.allowPeer true` is
   set; federation must be enabled (§17 sub-decision 4). Refusals say
   which setting to change.

5. **Read-only.** `push` is refused with a pointer to `kairo push`
   (§26). Pushing over Git would need Kairo-signed revisions for each
   commit, which a Git push doesn't carry.

Rationale: the pull already does every check Git can't, so the helper
adds only a translation layer, and serving from the cache keeps the
"Git sees only verified history" guarantee structural rather than a
filter over a live connection. Namespacing other actors' branches
keeps the creator's view unambiguous without hiding collaborators.

Affected specs:

- `PHASE_2_FEDERATION.md` §4 item 7 — the slice plan.
- `CLI.md` — `git-remote-kairo` and `kairo://` URLs.
- `PROJECT_LAYOUT.md` — the `kairo-git-remote` crate (implemented in
  slice 1, with `CLI.md` §15.1 and the peer policy).

---

## 32. Verified Bundle Import

Decision: `kairo bundle import` verifies an object bundle through the
same verifier as a pull: the bundle is one more source for it, next
to a peer, a closure bundle, and an actor export (§26 sub-decisions 7
and 8). Fixity-only import stays available only behind an explicit
`--unverified`. This closes `PHASE_2.md` §11's gap: until now the
store had two entry points with different guarantees.

Sub-decisions:

1. **Same checks, same order.** Actor geneses, the object genesis,
   then revisions before branches and tags, each checked for fixity,
   section (it names the bundled object), revision ids, and signature
   against the key state this node holds; then blobs against their
   hashes; then the bundle's Git pack, if any, through the quarantine
   (§21), admitted only with every bundled revision's commit closure.
   Actor chains are reported as for a pull (§23 sub-decision 8).

2. **Stop at the first failure, like a pull.** What verified before it
   stays; the error names the record and why; re-running after fixing
   the cause resumes (records already held are skipped).

3. **Missing key state is refused with guidance.** Object bundles
   deliberately leave key events out (`PACKAGE.md`, the bundle crate's
   rules), so a statement by a signer who rotated keys can't be checked
   unless this node already holds their key events. That fails like any
   bad signature, and the error says how to get them: `kairo fetch` the
   object from a node that has them, then import again (a node actor's
   can also come from `kairo federation learn --file`, which takes node
   actors only). Object bundles keep their current contents.

4. **Revoked keys are stored and flagged**, as a pull does (§16
   sub-decision 7), so nodes don't diverge; `verify object` reports
   them invalid.

5. **`--unverified`** keeps today's fixity-only import, for archives
   whose provenance the operator vouches for out of band. Its output
   says the records were not signature-checked.

6. **No provenance.** *Amended when built:* provenance names the peer
   that served a record, by URL and node actor (§16 sub-decision 8),
   and a bundle has neither, so a verified import records none, as an
   `--unverified` one never did.

Rationale: every way into the store should give the same guarantee,
and the pull verifier already encodes it. Refusing missing key state
(rather than shipping key events in object bundles) keeps the bundle
format's rule that key knowledge belongs to the actor, and the error
points at the tools that move it.

Affected specs:

- `PHASE_2.md` §11 — the bundle-import verification item.
- `PACKAGE.md` — the MVP slice's import semantics.
- `CLI.md` — `kairo bundle import` and `--unverified` (all
  implemented).


---

## 33. Build MVP: Build Targets, Local Resolution, Native Executor, `BuildResult`

Decision: Phase 2 §7–§8 ship a minimum that actually builds. An object
declares build targets in `kairo.toml` (manifest schema 1, extended
in place). A resolver
pins the target's requirements against this node's store and host into
a hashed resolved manifest. `kairo build --plan-only` stops there.
Without it, a best-effort native executor — §34's development mode —
runs the target, gated by config and by trust in the revision's
signer, stores the declared outputs as an artifact tree, and keeps an
unsigned execution record.
The acting actor then signs a `BuildResult` statement. `kairo
reproduce` re-runs a cited result and compares artifacts. Run targets,
isolation engines, provider chains and signed provides claims come
later. *Amended by §34 before the resolver slice:* resolution is a
closure over artifact providers from the start, the host supplies
facts rather than labels, and the native executor is development mode.

This resolves the conflicts the specs had among themselves and with
the code:

- **Who executes.** PLANNER §7 has the planner run commands and sign.
  Per §6 (CORE_LIBRARY is canonical), planning produces a plan and
  executes nothing; the executor runs it; the caller decides what to
  store and sign (EXECUTOR §14.3).
- **Build results are keyed on snapshots**, not revisions (§2, §19).
  FEDERATION's `resolution-success:…:revision:…` examples are stale.
- **Statement names.** New types use the PascalCase, `version: 1`
  convention of the implemented kinds. The dotted
  `kairo.statement.{build,provides,observation,inference}.v1` sketches
  in STATEMENTS §4.3–§4.6 and BUILD §8 are superseded; this entry
  defines `BuildResult`, and the others are defined when built.
- **Build results stay off the frontier.** OBJECT §2.3's "future
  `Build` statements join the frontier" is withdrawn: a build is
  evidence about a snapshot, so it must not change the snapshot.
- **"Capability" has three senses.** Actor delegation (GLOSSARY,
  CAPABILITIES) and runtime permission (SANDBOX) keep the word. What
  an object declares under `[[provides]]` and requires through
  `kind = "provides"` is a **provides label**. Labels are
  `kind:specifier(:detail)*`; environment labels are spelled
  `environment:<os>/<arch>` with `x86-64`, not `x86_64`
  (`environment:linux/x86-64`).
- **CLI shape.** `kairo build <snapshot-ref>` (CLI.md §17) and `kairo
  provider list|show|resolve` (PHASE_2 §8). PHASE_2's `--object` and
  FEDERATION's `kairo providers` are dropped.

Sub-decisions:

1. **Manifest schema 1, extended in place.** No objects exist outside
   development yet, so the manifest stays `schema = 1` rather than
   bumping, and the canonical form grows two things. The first is
   `[[build.targets]]`: `name` (`[a-z][a-z0-9-]*`, unique), `command`
   (a non-empty argument array, run without a shell), `workdir`
   (relative, default `"."`), nested `[[build.targets.dependencies]]`
   (the same two kinds as top-level `[[dependencies]]`), and
   `[[build.targets.outputs]]` (`name`, `path`, `artifact_path`,
   `kind = "file" | "directory"`). A target's requirements are the
   top-level dependencies plus its own. The second is a provides
   dependency's `version` requirement, which the parser accepts today
   and then drops. Versions use Cargo's requirement syntax (`">=4,
   <5"`). Both are part of the canonical manifest, so `manifest_hash`
   pins the recipe. Both are omitted from the canonical form when
   absent, so a manifest without them hashes as it does today.
   `[run]` stays rejected until run targets land.

2. **Local resolution, as a closure** (`kairo-resolver`; amended by
   §34 sub-decisions 2 and 5). A provides requirement is satisfied by
   one of:

   - an **artifact provider**: an object this node holds whose
     `[[provides]]` entry names a `target` and `output`, resolved to
     that output of a successful `BuildResult` for the object's
     current snapshot — or, when none is held, scheduled as a build
     the plan needs first;
   - a **content provider**: a `[[provides]]` entry with no target,
     meaning the object's own tree (seeds, data, source libraries);
   - an **environment contract**: `environment:*` labels resolve to a
     contract the host has an allowlisted engine for, or in
     development mode to `none`;
   - a **development-mode host tool**, configured under
     `[[runtime.host.tools]]` (`provides`, an absolute `path`,
     optional `version`), which marks the result host-dependent.

   An object requirement is satisfied by the pinned snapshot, or by
   the newest of the creator's version tags that meets its `version`.
   Labels match exactly; a declared version is read leniently
   (`3.81` is `3.81.0`). Candidates are held objects at their
   creator's `head`, whose trees this node holds. Ties are broken
   deterministically — artifact providers before content providers
   before host tools, then by object id — and the losers are listed
   in the plan's output. Resolution recurses
   into each provider's own target, producing a DAG; a cycle or an
   unsatisfied requirement fails resolution and names it. No signed
   provides claims, provider chains or hosting rules yet.

3. **The resolved manifest** is the canonical JSON document
   `kairo.resolved-manifest.v1`. It holds:

   - the object, snapshot, target, and environment contract (`none`
     in development mode);
   - each requirement as requested;
   - what resolved it: `artifact` (object, snapshot, target, output,
     and the artifact id when held), `content` (object, snapshot),
     or `host-tool` (label, version);
   - the plans of the builds it needs first, by plan hash, in order.

   It is stored as a blob, and its `BlobId` is the plan hash, which
   therefore covers the whole closure and excludes the engine (§34
   sub-decision 4). `kairo build --plan-only` prints it and writes
   nothing else. Content providers are materialized as their trees,
   read-only; artifact providers as their artifact trees.

4. **Policy.** `[runtime]` joins the config (§17):

   - `allow_local_process` (default `false`): development mode;
   - `timeout_secs` (3600; zero is invalid);
   - `[[runtime.host.tools]]`: development-mode host tools.

   A build is refused, exit 7 (policy denied), unless
   `allow_local_process` is on. It is also refused unless the
   revision's signer is an actor in this node's keystore or is trusted
   at `general` scope by one. `--plan-only` needs neither. EXECUTOR
   §21's "clearly display host access risk" is met by the build output
   naming the executor `native (unsandboxed)`.

5. **Best-effort native executor** (`kairo-build`), §34's
   development mode. It runs the command with no shell in a fresh
   temporary directory that holds a copy of the snapshot's tree;
   providers are materialized beside it under `deps/<object>`. The
   environment is scrubbed to `HOME` (the temp dir), `LANG=C`,
   `SOURCE_DATE_EPOCH` (the revision's timestamp; *amended by §52:*
   the target's, or 1980-01-01), `KAIRO_SRC` and
   one `KAIRO_PROVIDER_<LABEL>` per requirement naming the provider's
   path (or the host tool's), and a `PATH` made of the artifact
   providers' `bin/` directories followed by the directories of the
   declared host tools — never the host's own `PATH`, so a command
   not provided by anything can't start. The wall-clock limit kills the process group. Only
   declared outputs are collected; a missing one fails the build
   (EXECUTOR §14.1). Paths are confined to the workspace (EXECUTOR
   §10.3). There is no network or filesystem isolation and no
   canonical mount path, and the record says so.

6. **Artifacts are blob trees**, not Git trees and not objects (BUILD
   §12). Each output file is stored as a blob. The canonical JSON
   document `kairo.artifact-tree.v1` lists `artifact_path`, blob id and
   executable bit, sorted by path. It is stored as a blob too, and its
   `BlobId` is the artifact id (BUILD §6/§9's "artifact snapshot").

7. **Execution records** are unsigned local JSON under
   `<store>/executions/<id>.json`, the id a `BlobId` over the bytes. Each records the plan hash, snapshot,
   target, executor id and version, the host (OS, architecture,
   kernel, tool versions for the resolved host labels), start and end
   times, exit status, the artifact id, and log blobs. They are
   operational evidence (EXECUTOR §15). The bundle section for them
   (PACKAGE §4.4) comes with `kairo reproduce`.

8. **`BuildResult` statement**, version 1, signed by the acting actor
   (`kairo build --actor`) after every completed build, success or
   failure. Its body holds:

   - `object`, `snapshot`, `target`;
   - `resolved_manifest` (`BlobId`);
   - `result` (`success` or `failure`);
   - `artifact` (`BlobId`, present on success only);
   - `executor` (e.g. `native/1`);
   - `environment` (the contract label, or `none`);
   - `host_dependent` (`true` whenever the executor implements no
     contract or a host tool was used; such results are never
     substitutable, §34 sub-decision 10);
   - `execution` (the signer's execution record id, so the evidence
     can be asked for; *added when built*).

   It is chained in the signer's object chain, rides in object bundles
   and federation closures, and is accepted by the daemon's statement
   endpoint without object authority: any actor may report a build.
   The resolver treats an artifact as held when a successful result
   for `(object, snapshot, target)` by an accepted signer — a keystore
   actor, or one a keystore actor trusts at `general` scope — names a
   blob this node has. `kairo build` builds what the plan needs first,
   one at a time, re-resolving after each so its result pins the next.

   It is indexed by object but never enters the frontier. Timeouts and
   policy refusals sign nothing: they are operational errors, not
   build outcomes. The federation `ResolutionSucceeded` token is a
   view over successful `BuildResult`s and their resolved manifests,
   defined with discovery step two, not a second statement now.

9. **`kairo reproduce statement:<id>`** loads the cited
   `BuildResult`'s resolved manifest, materializes exactly what it
   pins, re-runs the target under the same policy, and signs its own
   `BuildResult` citing the same manifest. It reports `matches` or
   `differs`, with the two artifact ids. A difference is a finding,
   not an error (exit 0). Strict reproduction mode (EXECUTOR §16.1)
   is deferred. *Amended when built:* it never re-resolves; when the
   plan's pinned inputs (its blob, prerequisite artifacts, host tools)
   aren't all here it refuses and lists them.

   Execution evidence travels in object bundles: `kairo bundle export
   --include-executions` writes, for each of the object's
   `BuildResult`s whose record this node holds,
   `executions/<record id>/record.json` and the blobs it names (plan,
   artifact tree and files, logs) under `blobs/`, and lists the ids in
   the manifest's `executions`. Import fixity-checks the record and
   each blob against its own domain. So a second node with only the
   bundle can reproduce the result. Each record carries a random
   per-run id, so two runs are two records.

10. **The CLI drives this in-process.** `kairo build` and `kairo
    reproduce` call the resolver and executor directly, as other
    write commands do through Phase A (§13). A daemon RuntimeService
    with task progress (DAEMON §8) comes later and must reuse the same
    crates.

Rationale: §7's value is doing work, and every piece here is the
smallest that is honest about its guarantees: the recipe is pinned by
the manifest hash, the inputs by the resolved manifest, the output by
a content hash, and the claim by a signature, while the executor
plainly says it is not a sandbox. Gating on config plus signer trust
meets EXECUTOR §21 without the policy engine. One result type with an
outcome keeps the statement set small until federation needs more.

Affected specs:

- `BUILD.md` — targets, dependencies, outputs, statement (§2–§8).
- `PLANNER.md` — §1/§7 (who executes), §6 (resolved manifest shape),
  label spelling.
- `EXECUTOR.md` — §21 (the MVP local executor).
- `STATEMENTS.md` — §4.3–§4.6 superseded; `BuildResult` added.
- `OBJECT.md` — §2.3 (build results off the frontier), §4 (build
  targets).
- `schemas/canonical/object-manifest-v1.md` (build targets, provides
  dependency `version`); `build-result-v1.md`, `resolved-manifest-v1.md`,
  `artifact-tree-v1.md` (new).
- `CLI.md` — §17 `kairo build`, `kairo reproduce`, `kairo provider`.
- `GLOSSARY.md` — provides label.
- `FEDERATION.md` — snapshot-keyed resolution tokens.
- `PROJECT_LAYOUT.md` — `kairo-resolver`, `kairo-build` first;
  `kairo-planner`/`kairo-provider` when chains land.
- `PHASE_2.md` §7–§8.

---

## 34. Strict Build Model: Environment Contracts, Isolation Engines, and the Bootstrap

Decision: the end state of build and run is that **guest software never
runs on the host**. The host runs only trusted code: Kairo itself and
a small, well-known set of isolation engines. Everything a build
consumes — compilers, build tools, libraries, data, shells — is an
artifact of an object in the store, and the chain of artifacts bottoms
out at a per-(system, architecture) seed of auditable machine code.
§33's native executor and host tools are a development mode on the
way there, never a provider of any environment. This entry fixes the
model; §33 is amended below to fit it from the first resolver slice.

Sub-decisions:

1. **Three layers.**

   - An **environment contract** is what a label like
     `environment:linux/x86-64` promises the guest: a (system,
     architecture) pair, a kernel ABI, the canonical filesystem layout
     (sub-decision 6), no network, a fixed uid, a fixed clock, and a
     defined environment-variable set. Each contract is a document
     under `schemas/environment/`. A change that could alter build
     output is a new label, never a revision of an existing one.
   - An **isolation engine** is host-side code that implements a
     contract: Linux namespaces (bubblewrap), an OCI runtime
     (Docker, Podman, Apptainer), a microVM with a minimal kernel, an
     emulator. Engines are **executor adapters** in EXECUTOR.md's
     terms. Which engines a host has is a host fact.
   - **Native execution** is reserved for Kairo and for engines. No
     guest command, build or run, ever runs natively in the strict
     model.

   Builds depend on contracts, never on engines. A chain reads
   `environment:msdos/x86` ← DOSBox artifact, which requires
   `environment:linux/x86-64` ← *some engine on this host*. The host
   never provides a contract directly.

2. **The host provides facts, not software.** `[runtime.host]` is
   detected, not declared: system, architecture, kernel, and the
   engines found. The host's `PATH` never enters a build. Host tools
   exist only as §33's development-mode escape hatch, and a result
   that used one is marked and can never satisfy an archival policy.

3. **Engines are the trusted part.** The host has to trust whatever
   runs directly on it, and in this model that is Kairo and the
   engines, so the set of engines is deliberately small and
   well-known, and each must strongly assert isolation: filesystem,
   network, process, and time. Every engine adapter names the contract
   it implements, its own version, and its isolation claims, and each
   `ExecutionRecord` names the engine used. Policy allowlists engines
   (`[runtime.engines]`). An engine that is itself an archive artifact
   (bubblewrap, a minimal kernel) may run natively only when
   allowlisted by object id and backed by reproduction evidence; that
   is how the host dependency shrinks over time without ever running
   unknown software natively.

   Order of engines: Linux namespaces (bubblewrap) first, as the
   smallest and archive-buildable; a microVM with an archive-built
   kernel second, as the strict and bootstrap target; OCI runtimes as
   a parallel adapter for hosts that already have one.

4. **Artifacts are portable across engines.** A `BuildResult` cites
   the contract label and the plan hash; only the `ExecutionRecord`
   cites the engine. The plan hash excludes the engine and its
   mounting strategy, so an artifact built under any engine that
   implements a contract is a valid input to a build under any other.
   Equivalence of engines is a claim made by trusted adapter code and
   checked empirically: one plan reproduced under two engines yielding
   the same artifact id is evidence for both the artifact and the
   engines; a disagreement is a recorded finding that names both
   engines, so it points at a nondeterministic build or a leaky
   engine.

5. **Plans are closures.** The resolver produces a DAG: every
   requirement resolves to a provider artifact `(object, snapshot,
   target, output)` backed by a `BuildResult`, or to an object's own
   content (seeds, data), recursively. The plan hash covers the whole
   closure. A plan whose artifacts aren't all held is still a plan:
   the builds it needs first are listed, in order.

6. **Canonical paths; engine-chosen mounting.** Inside a contract,
   each artifact closure appears read-only at `/kairo/artifact/<id>`,
   the source at `/kairo/src`, outputs under `/kairo/out`, so anything
   that embeds a build path sees the same one everywhere. How an
   engine realizes that — bind mounts, assembled layers, per-artifact
   read-only images cached by artifact id and attached to a VM, or a
   copy — is the engine's choice, named in the record and absent from
   the plan hash.

7. **Substitution is trust policy.** A node accepts an artifact it
   didn't build when `k` actors it trusts at the `build` scope have
   signed `BuildResult`s agreeing on the artifact id for the same plan
   hash; policy may also require engine diversity among them. This
   rides on federation discovery (§16, step two). Nothing is
   substituted silently: the record of a substituted input names the
   results relied on.

8. **Durable content is sources, seeds, manifests, and statements.**
   Artifacts are a cache that any node can re-derive from the plan.
   An archive guarantees the former; the latter is kept for
   convenience and as evidence.

9. **The bootstrap is a goal of the archive.** Each (system,
   architecture) pair has a **seed**: an object whose provided binary
   is its own content (a few hundred bytes of hand-auditable machine
   code, stage0's `hex0`). A seed has no `BuildResult`; it is a root
   of trust alongside signing keys. Policy lists the seeds a node
   accepts, and seeds accrue independent audit attestations from other
   actors. Every later stage is an ordinary object whose build target
   depends on the previous stage's outputs. The first bootstrap import
   is the existing chain from the Bootstrappable Builds project
   (`stage0-posix` → `M2-Planet` → Mes → tcc → an early gcc → a
   modern toolchain), archived as objects rather than reinvented. The
   strict target runs that chain under the microVM engine with a
   minimal builder kernel, so no host kernel is in the guest's trusted
   base; cross-architecture bootstraps through an emulator engine come
   after.

10. **Development mode is marked.** §33's native executor implements
    no contract. Its results carry `environment = "none"` and a
    `host_dependent` flag, are never substitutable, and never satisfy
    an archival policy. It exists so builds can be exercised before
    the first engine lands and for local iteration afterwards.

Rationale: an archive's claim is "this can be rebuilt from what the
archive holds," which is only true when nothing outside the archive
takes part in a build except a CPU, an isolation boundary, and Kairo.
Making engines the only natively-run code concentrates the trust
decision where it can be audited, and makes the set of such code
small by construction. Keying artifacts on contracts rather than
engines is what lets independent nodes with different engines confirm
each other's results, which is the evidence an archive of software
needs most.

Affected specs:

- `DECISIONS.md` §33 — amended (see its sub-decisions 2–5 and 8).
- `ENVIRONMENTS.md` — §2 (contract vs engine), §3–§5 (descriptor
  becomes the contract document; executors match on contract).
- `EXECUTOR.md` — §1–§4 (engines as adapters), §10–§11 (canonical
  paths), §21 (local process executor is development mode).
- `PLANNER.md` — §9–§12 (provider chains end at an engine, not the
  host), §16 (host facts).
- `SANDBOX.md` — isolation claims per engine.
- `POLICY.md` — engine allowlist, accepted seeds, substitution.
- `GLOSSARY.md` — environment contract, isolation engine, seed.
- `PHASE_2.md` §7–§8 and the roadmap after Phase 2 (engines,
  bootstrap).
- `THREAT_MODEL.md` — the executor and engine entries land with the
  code.

---

## 35. The Bubblewrap Engine and the `linux` Contract

Decision: the first isolation engine (§34 sub-decision 3) is Linux
namespaces through **bubblewrap**, implementing the contract
`environment:linux/<arch>` for the host's own architecture. A guest
sees only `/kairo`, a minimal `/dev`, `/proc`, and an empty `/tmp`:
no host `/usr`, `/lib`, or `/bin`. Every program a build runs, its
shell included, comes from an artifact or content provider in the
store. Results are not host-dependent, so they are the first that can
count toward substitution and archival policy.

Sub-decisions:

1. **The contract** is documented in
   `schemas/environment/linux-v1.md`, label `environment:linux/<arch>`
   (`x86-64`, `aarch64`, ...). It promises:

   - the Linux system-call interface of the running kernel (its
     release is recorded, not promised);
   - this filesystem, and nothing else:
     - `/kairo/src` — the snapshot's tree, writable; the command runs
       in `/kairo/src/<workdir>` and outputs are collected from there;
     - `/kairo/artifact/<artifact id>` — each artifact provider,
       read-only;
     - `/kairo/content/<snapshot id>` — each content provider's tree,
       read-only;
     - `/kairo/home` and `/tmp` — empty and writable;
     - `/proc`, and bubblewrap's minimal `/dev` (`null`, `zero`,
       `full`, `random`, `urandom`, `tty`, `ptmx`, `pts`, `shm`, and the
       standard-stream links);
   - no network (a fresh namespace with only loopback), no IPC or
     process visibility outside the sandbox, hostname `kairo`, uid and
     gid 1000;
   - the environment: `HOME=/kairo/home`, `LANG=C`, `TZ=UTC`,
     `SOURCE_DATE_EPOCH`, `KAIRO_SRC=/kairo/src`,
     `KAIRO_PROVIDER_<LABEL>` naming each provider's canonical path,
     and `PATH` made of the providers' `bin/` directories in
     requirement order. *Amends §33 sub-decision 5:* content
     providers' `bin/` directories join `PATH` too, in development
     mode as well, so a toolchain can be a content provider (a seed).

   The clock is **not** frozen: bubblewrap has no time namespace
   control, so wall-clock reads are a known source of nondeterminism,
   recorded as such. Freezing it is a property a later engine may add
   under a new contract label (§34 sub-decision 1).

   *Amends §34 sub-decision 6:* outputs are collected relative to the
   workdir under `/kairo/src`, as in development mode; there is no
   separate `/kairo/out` in contract v1.

2. **Engines are allowlisted in config**, never auto-detected:
   `[[runtime.engines]]` entries with `kind = "bubblewrap"` and an
   absolute `path` to `bwrap`. Before offering a contract, Kairo probes
   each listed engine with a trivial sandbox; one that fails the probe
   (no user namespaces, a wrong path) is reported and not offered.
   Nothing is enabled by default.

3. **Selection.** A target that requires `environment:linux/<arch>`
   resolves to an allowlisted engine implementing it when one passes
   its probe; failing that, to the host in development mode as today
   (§33). A target that names no environment runs in development mode
   only: requiring the contract is how a build asks for isolation.

4. **No host tools under an engine.** Once the plan's environment
   resolves to an engine, `[[runtime.host.tools]]` are not candidates;
   a requirement only a host tool could satisfy fails resolution and
   says so. Host tools remain a development-mode fallback.

5. **Policy.** An engine build does not need `allow_local_process`,
   and does not require trust in the revision's signer: running code
   one doesn't vouch for inside an allowlisted sandbox is what the
   engine is for (EXECUTOR §21 applies to the local process executor
   only). The residual risk is the kernel and bubblewrap's own attack
   surface, named in `THREAT_MODEL.md`. Accepting another signer's
   *results* is unchanged (§33 sub-decision 8).

6. **Records and results.** The executor is `bubblewrap/1`. Its
   record names the engine path and `bwrap --version`, the kernel
   release, the isolation claims above, and "wall clock not frozen" as
   known nondeterminism. Its `BuildResult` carries the contract label
   as `environment` and `host_dependent = false`.

7. **Workspace.** Kairo still assembles the workspace on the host
   (`<store>/build/`), then binds its pieces to their canonical paths;
   mounting strategy stays the engine's own (§34 sub-decision 6), and
   none of it enters the plan hash.

Rationale: an empty root is the only honest reading of §34's "nothing
outside the archive takes part"; bubblewrap is small, rootless, and
widely packaged, and is itself buildable from the archive later. The
clock is the one promise bubblewrap can't keep, so the contract says
so instead of implying it.

Affected specs:

- `schemas/environment/linux-v1.md` (new).
- `ENVIRONMENTS.md` — the first contract.
- `EXECUTOR.md` — §20 (the bubblewrap executor), §21 (scope of the
  local-process rules).
- `SANDBOX.md` — what the engine enforces.
- `THREAT_MODEL.md` — the engine boundary.
- `CLI.md` — §17 engine selection and output.
- `DECISIONS.md` §17 — `[[runtime.engines]]`.
- `PHASE_2.md` §7.

---

## 36. Bootstrap Import: stage0-posix for AMD64

> *Superseded in part by §37 sub-decision 8:* the bootstrap is now
> stage0-posix and its submodules as source objects, archived from
> their upstream URLs, plus one native wrapper; sub-decisions 1, 2, and
> 4's two flattened objects and their packaging, and sub-decision 3's
> `seed:*` labels, are retired. Seeds are `(source, revision)` pairs.
> The pinned release, the one target, its glue, and the answers check
> stand.

Decision: the archive's first bootstrap (§34 sub-decision 9) is the
Bootstrappable Builds project's **stage0-posix**, pinned at
`Release_1.9.1` (commit `45d90f5955b6907dc6cdea9ebafce558359edcd3`),
for `environment:linux/x86-64`. It is imported as two objects — a seed
and the sources that build from it — and built by one target under the
bubblewrap engine (§35), from 847 bytes of hand-auditable machine code
to M2-Planet, mescc-tools, and mescc-tools-extra with no host tool
taking part. Only seeds the operator lists can satisfy a `seed:*`
label. The next stages (Mes, tcc, gcc, from live-bootstrap) come
later and depend on this one's artifact.

Sub-decisions:

1. **The seed object** (`bootstrap-seeds`, upstream commit
   `cedec6b8066d1db229b6c77d42d120a23c6980ed`) holds that repository's
   tree unchanged, plus a `kairo.toml` declaring the content provider
   `seed:stage0-posix/amd64`. Its AMD64 roots are `hex0-seed` (229
   bytes) and `kaem-optional-seed` (618 bytes); their hex sources sit
   beside them for audit.

2. **The sources object** (`stage0-posix`) holds the upstream tree
   with its submodules flattened in, except `bootstrap-seeds/`, which
   is replaced by a symbolic link to the seed's canonical path,
   `/kairo/content/<seed snapshot id>`, valid inside the contract
   (§35). It adds `kairo.kaem`, the three lines of upstream's
   `kaem.amd64`, so the seed `kaem` runs the whole chain, and a
   `kairo.toml` requiring `environment:linux/x86-64` and
   `seed:stage0-posix/amd64`, with one target `amd64`:

   - command `["./bootstrap-seeds/POSIX/AMD64/kaem-optional-seed",
     "./kairo.kaem"]`: phases 0–23, ending in upstream's own
     `sha256sum -c amd64.answers`, so a build that disagrees with
     upstream's published hashes fails;
   - output `AMD64/bin` as the directory `bin`;
   - `[[provides]]` from that output, version `1.9.1`: `tool:kaem`,
     `tool:hex2`, `tool:M1`, `tool:blood-elf`, `tool:M2-Planet`,
     `tool:M2-Mesoplanet`, `tool:get_machine`, and the
     mescc-tools-extra utilities (`tool:catm`, `tool:cp`, `tool:chmod`,
     `tool:mkdir`, `tool:rm`, `tool:sha256sum`, `tool:match`,
     `tool:replace`, `tool:untar`, `tool:ungz`, `tool:unbz2`,
     `tool:unxz`, `tool:wrap`).

   One target, because upstream's scripts carry state between phases
   in the tree; splitting it means rewriting their glue, which can
   come later without changing the objects' identities.

3. **Seed policy, minimal.** `[[runtime.seeds]]` lists accepted seed
   objects by bare id. A `seed:*` label is satisfiable only by a
   content provider (no `target`) of a listed object; anything else is
   passed over and named in the resolution's diagnostics. Seeds are
   roots of trust alongside signing keys: accepting one is an operator
   decision, never inferred from trust in its signer.

4. **Packaging.** `bootstrap/stage0-posix/` holds the two `kairo.toml`
   templates and `import.sh`, which takes a local checkout of the
   pinned upstream (with submodules), flattens it, commits both trees,
   and creates and branches both objects through the `kairo` CLI,
   printing the config lines to accept the seed and allowlist the
   engine. The test that runs the full chain is `#[ignore]` and needs
   that checkout (`KAIRO_STAGE0_DIR`), since tests don't use the
   network; it checks the resulting artifact against
   `amd64.answers` too. *Amended when built:* the import commits
   every file of the checkout, ignore rules notwithstanding
   (`git add --force`), since upstream's `.gitignore` files cover the
   placeholder files that keep `AMD64/artifact/` and `AMD64/bin/`, the
   directories the chain writes into. The artifact therefore also
   holds `bin/placeholder`. Commits use a fixed author and date, so
   re-importing the release gives the same revisions, and the SHA-256
   object format (`git:sha256:` revisions, §19), the default for new
   objects' history.

Rationale: stage0-posix already runs, unmodified, inside §35's empty
root, so the import exercises the strict model end to end at the cost
of two small files of glue. Separating the seed makes the one thing a
node must take on faith explicit, small, and listed.

Affected specs:

- `DECISIONS.md` §17 — `[[runtime.seeds]]`.
- `PHASE_2.md` §7 — the bootstrap import.
- `GLOSSARY.md` — Seed (the policy).
- `CLI.md` §17 — seeds in resolution.

---

## 37. Source Objects: Archiving External Git Repositories

Decision: the archive holds two kinds of object.

- A **source object** archives one external Git repository faithfully.
  It is identified by its source: its genesis is derived from the
  canonical repository URL alone, so every node that mirrors the same
  repository computes the same object id. Its history is upstream's
  own commits, in upstream's object format, unmodified and with no
  `kairo.toml`. What actors sign about it are **observations** —
  "fetching this URL at this time, I saw these refs at these commits,
  and hold their full closures" — which independent observers can
  make and compare.
- A **native object** is born in the archive, with SHA-256 history
  (§19 sub-decision 7) and a `kairo.toml`. It references source
  objects as dependencies, pinned by upstream commit id, and carries
  the richer metadata: build targets, provides, and how the sources
  fit together.

Sub-decisions:

1. **Deterministic source genesis.** A source genesis holds only the
   canonical URL. Its object id is derived from it under its own
   domain; it has no creator, timestamp, or nonce, and is not signed,
   since there is nothing in it to vouch for. Canonical form: scheme
   and host lowercased, a default port dropped, one trailing `/` and a
   trailing `.git` dropped, the path otherwise kept as written.
   Only network URLs identify a source: `https` and `git`. An `ssh`
   URL (`ssh://git@host/path` or `git@host:path`) canonicalizes to
   `https://host/path`, so the same repository has one id however it
   was reached. `file` URLs have no source identity: a local path
   means nothing on another node. (Tests serve repositories over
   `git://` on localhost.)

2. **Observations, not authorship.** A source object's statements are
   observations by an actor: the URL fetched, the time, every ref
   seen (`refs/heads/*`, `refs/tags/*`) with its commit as a typed
   revision, and the claim that the observer holds each commit's
   closure. Commit ids are content hashes, so two observers agreeing
   on a ref's commit have independently retrieved the same content.
   Trust policy decides whose observations a node relies on; they
   never override each other. The OBJECT §4 rule that a revision's
   tree carries `kairo.toml` applies to native objects only. *Amended
   when built:* source fetches follow a redirect on the initial
   request (`http.followRedirects=initial`, Git's default), unlike
   federation fetches, which follow none: the operator named the
   repository, hosts move them (Savannah redirects every request to
   another host), and every object is checked against its hash however
   it is served. One
   statement per fetch lists **every** ref the remote advertised, so
   it is a faithful picture of one moment: a ref absent from a later
   observation was deleted, and two observers compare in one step.
   *Amended when built:* each ref is recorded at the object it names
   (`target`), which for an annotated tag is the tag object, not its
   commit, so the tagger's message and signature are archived too.
   The refs are fetched by name into a quarantine and read back from
   it, so an observation names exactly what was fetched, never a
   listing that upstream could change in between.

3. **Nothing seen is ever dropped.** Fetching a tracked source again
   is additive: every commit any observation has named stays pinned
   in the Git cache (`refs/kairo/observed/<oid>`) and is exempt from
   cache garbage collection. The fetch lands in a quarantine and is
   admitted to the pool only once every observed object's closure is
   there (§19 sub-decision 5); what the source already holds is sent
   as "haves", so only new history crosses the network. A ref that upstream moves, rewrites, or
   deletes gets a new observation, and the old commit stays
   reachable through the old one. Ref history is the sequence of
   observations, so a force-pushed branch or a moved tag is visible,
   not lost.

4. **Tags are mirrored as observations** and managed on the archive's
   side: what upstream's tag `v1.2` pointed at, when, and by whose
   observation, with every value it has had. Version tags
   (`ObjectVersionTag`) stay a native-object feature for now; deriving
   them from observed tags can come later without changing what is
   recorded.

5. **Submodules are their own source objects.** A commit's
   `.gitmodules` and gitlinks name `(url, commit)` pairs; each URL
   (relative ones resolved against the parent's) is a source object
   by sub-decision 1, so the link from parent to submodule is
   computable on every node. Archiving a source commit archives each
   submodule commit it pins, recursively, by default, each into its
   own source object and recorded in that source's own observation;
   recursion stops at a source already holding the pinned commit,
   guards against cycles, and has a depth limit. An archive that holds
   a commit but not what it pins is incomplete. *Amended when built:*
   the top source's submodules are those its observed refs' commits
   (tags peeled) pin; a submodule source's are those of the commits
   its parents pin. Each submodule source is fetched by ref as usual;
   a pinned commit its refs no longer reach is then fetched by id and
   listed in its observation's `pinned` (tagged, present only when
   non-empty). The link itself needs no statement: the gitlink is in
   the parent's tree and the URL in its `.gitmodules`, both archived
   content. A link with no `.gitmodules` URL, a URL with no source
   identity, or a commit nobody serves is reported, and `kairo source
   fetch` says `complete = no`; `--require-complete` fails then.
   Depth defaults to 8; `--no-recurse-submodules` skips it all.

6. **Native objects pin sources by upstream revision.** An object
   dependency on a source names the source (its URL, or its object
   id) and `revision = "git:<format>:<oid>"`. That pin is the same on
   every node, unlike a snapshot id, which names signed statements
   and differs between observers. There are no version ranges for
   sources: using a newer upstream commit is a conscious edit of the
   pin, and so a new revision of the native object. *Built:* `kind =
   "source"` with `source` (any spelling; hashed canonical), `revision`,
   and optional `path` and `seed`; the resolver requires the source and
   that exact commit to be held, and never re-resolves.

7. **Placement.** A dependency may name a `path` inside the source
   tree (`path = "M2libc"`); the executor materializes it there,
   read-only, instead of at `/kairo/content/<snapshot>` only, so
   upstream layouts that expect sibling repositories build unchanged.
   *Amended when the bootstrap was rebuilt on sources:* a placed source
   is a **writable copy**, made fresh for each build, not a read-only
   mount, because upstream builds write into their own trees
   (stage0-posix writes into its `AMD64/` submodule). Placements may
   **nest**, as submodules do: outer paths are copied first, inner ones
   into the holes their gitlinks leave; two at one path, or one over
   files the outer tree already holds, are refused. Purity doesn't
   rest on read-only: the plan pins every input by commit and only
   declared outputs leave. Without `path`, a source stays read-only at
   `/kairo/source/<source id>/<oid>`.

8. **The bootstrap becomes sources plus a native wrapper.**
   stage0-posix, its eleven submodules, and bootstrap-seeds become
   source objects at the commits Release_1.9.1 pins. The objects of
   §36 are replaced by one native object (SHA-256) holding
   `kairo.toml` and `kairo.kaem`, depending on each source at its
   pinned commit and path, so the archive proves exactly which
   upstream commits it built. *Amends §36 sub-decision 3:* seeds are
   named as `(source, revision)` pairs under `[[runtime.seeds]]`; a
   native object marks a dependency `seed = true`, and it resolves
   only if that exact pair is listed, so accepting a seed is
   accepting specific bytes at a specific upstream commit. `seed:*`
   labels are retired with §36's seed object.

9. **Sources federate like objects.** *Added when built:*

   - A source can be published (`kairo federation publish --object
     <source id>`), and its closure is every observation of it, every
     observer, and every object an observation names (ref targets,
     annotated tags' tag objects included, and pinned commits) as
     servable history. Its genesis route serves the unsigned
     `SourceGenesis`.
   - Pulling one (`kairo fetch --object <source id>`, or `kairo source
     pull <url> --from <peer>`, which derives the id from any spelling
     of the URL) checks the genesis by re-deriving the id from its
     URL, verifies each observation under its signer, fetches the
     history the observations name through the quarantine, and pins
     it with the never-removed observed prefix. Pulled observations
     are evidence under their signers, never re-signed; a node can
     observe upstream itself and compare. Two nodes so converge on one
     source id.
   - Publishing a native object also serves the sources its revisions
     pin, at those commits, and pulling it brings them, so a pulled
     wrapper can build. Such a source's served history is only the
     pinned commits (often ancestors no observation names directly);
     the receiver requires exactly the pins its own copy of the
     manifest names. *Built:* the server reads each published native
     object's revisions' `kairo.toml` from its Git cache and serves
     each pinned source it holds with all of its observations but only
     the pinned commits' history, merged with any direct publication;
     the native object's listing names those sources (`sources`, first
     page). After pulling the native object, the client reads the
     manifests from the history it just fetched and pulls each listed
     source it pins with exactly those commits required, so it fetches
     what its own copy of the signed manifests names, not what the
     peer says. `kairo fetch` reports them (`sources = N`).
   - Push does not carry sources yet.

Rationale: an archive of software has to keep what it was given
exactly as given, and say where it came from in a way others can
check. Deriving a source's identity from its URL lets independent
archives converge on one object and compare what they saw; pinning by
upstream commit makes every downstream build name exactly which
upstream content it used; and never dropping a seen commit is what
makes an archive outlast the repositories it mirrors.

Affected specs:

- `OBJECT.md` — the two kinds; §4's `kairo.toml` rule for native
  objects only.
- `STATEMENTS.md` — the source genesis and observation statements.
- `schemas/canonical/` — `source-genesis-v1.md`, the observation.
- `OBJECT_STORE.md`, `STORE.md` — observed-commit retention.
- `BUILD.md`, `object-manifest-v1.md` — `revision` and `path` on
  dependencies.
- `FEDERATION.md` — sources as shared objects across nodes.
- `DECISIONS.md` §20 (unreferenced commits stay local, except
  observed ones), §36 (the bootstrap's layout).

---

## 38. File Sources, the `linux-root` Contract, and live-bootstrap

Decision: the second bootstrap milestone builds the Bootstrappable
Builds project's **live-bootstrap** from the archive's stage0-posix
artifact through Mes to tcc-0.9.27, under the engine, with
live-bootstrap's own scripts and its own published-hash checks running
unmodified. Three pieces make that possible: **file sources** for the
tarballs live-bootstrap consumes, a **`linux-root`** contract in which
the build tree is the guest's root filesystem, and **placement** for
artifact dependencies too.

Sub-decisions:

1. **File sources are identified by content.** A single file (a
   distfile, a release tarball) is a source object whose identity is
   the SHA-256 of its bytes: its genesis holds that hash and nothing
   else, unsigned, so every node that holds the same bytes derives the
   same id however it got them. The bytes are a blob in the store.
   What actors sign are **file observations**: "fetching URL U at time
   T, I got bytes hashing to H". Several mirror URLs serving one file
   are several observations of one object, and a URL that later serves
   other bytes is an observation of a different one. (Git sources stay
   identified by URL, §37: a repository is a history that changes; a
   file is its bytes.)

2. **Fetching a file source.** `kairo source fetch-file <url> --sha256
   <hex>` downloads the URL with the source transport rules (§37
   sub-decision 2 as amended: `https` only for files, or `http` for a
   loopback host, as federation peers allow; one redirect followed;
   `max_file_bytes` and `file_timeout_secs`), refuses bytes that don't
   hash to the expected value, stores the blob, and signs a file
   observation. Without `--sha256` it records whatever it got, and the
   output names the hash. Nothing fetched is ever dropped.

3. **File dependencies.** A manifest dependency `kind = "file"` names
   a file source by `sha256` and places it at `path`, a file path in
   the source tree (`external/distfiles/mes-0.27.1.tar.gz`); it
   resolves only if the blob is held.

4. **Placement grows two options**, for source and file dependencies
   alike, and artifact dependencies gain placement:
   - `from = "<subpath>"` places a subtree of the source rather than
     all of it;
   - `overlay = true` lets a placement fill in around files the
     native tree already holds, which take precedence, instead of
     requiring an empty path; so a wrapper can supply its own
     configuration over an upstream tree;
   - a provides or object dependency may name `path` to copy what it
     resolved to there, an artifact or an object's tree; an environment
     or host tool can't be placed.

5. *Superseded by §41, which folds this contract into
   `environment:linux`.* **The `linux-root` contract** (`schemas/environment/linux-root-v1.md`,
   label `environment:linux-root/<arch>`): the guest's `/` is the
   build tree, writable, with `/proc`, the minimal `/dev`, and an
   empty `/tmp` mounted over it; no `/kairo` paths and no host paths.
   Every dependency must be placed, since there is nowhere else for it
   to appear. Everything else matches `linux` v1: no network, hostname
   `kairo`, uid and gid 1000, the same environment variables with
   `KAIRO_SRC=/` and `HOME=/tmp`, the clock not frozen. The command runs in `/<workdir>`;
   declared outputs are collected from the tree into the store as
   always. It is a separate label because where the guest finds things
   is part of the promise and builds bake those paths into their
   outputs (tcc embeds its prefix), so results under the two contracts
   must not be confused. The bubblewrap engine implements both.

6. **The live-bootstrap wrapper** (`bootstrap/live-bootstrap/`), for
   **x86** (sub-decision 7):
   - live-bootstrap is a source object at a pinned commit; its `steps/`
     and the files of its `seed/` are placed where live-bootstrap's own
     `rootfs.py` puts them (`/steps`, `/`);
   - each distfile the milestone's steps list is a file source,
     placed in `/external/distfiles`;
   - the stage0-posix wrapper's x86 `bin` artifact (§36–§37) is
     placed at `/x86/bin`, and stage0-posix's M2libc at `/M2libc`,
     since live-bootstrap pins exactly the same stage0-posix commit;
     this is Kairo's artifact chaining between native objects,
     replacing live-bootstrap's rerun of stage0;
   - the wrapper supplies `bootstrap.cfg` and a manifest truncated
     after the milestone's last step (sub-decision 11), over
     live-bootstrap's (`overlay = true`), and
     starts from live-bootstrap's `seed/after.kaem`;
   - its output is the installed `usr/` tree, and its `[[provides]]`
     include `tool:mes` and `tool:tcc`.

   live-bootstrap's own checks run unmodified: each step verifies its
   distfiles' hashes and its built packages against the checksums it
   publishes, so a passing build means the archive's output matches
   upstream's.

7. **The milestone is x86, not AMD64.** live-bootstrap's own path,
   its CI and its published checksums, is x86 (32-bit): for AMD64 it
   publishes checksums only through mes-0.27.1, and its tcc-0.9.27 step
   targets i386. Running it unmodified with its checks therefore means
   x86. An x86-64 Linux kernel with IA32 emulation runs i386 code
   natively, so on such a host bubblewrap also implements
   `environment:linux/x86` and `environment:linux-root/x86`, but only
   when its probe shows it: a tiny static i386 program must run inside a
   sandbox. Where it doesn't, the x86 contracts aren't offered. The
   stage0-posix wrapper gains an `x86` target alongside `amd64`, from
   the same seeds repository and stage0-posix's x86 submodule.

8. **Artifact offers match the consumer's architecture.** Binaries are
   architecture-specific while provides labels (`tool:kaem`) are not,
   so an artifact offer is a candidate only when the environment its
   build target declares has the same architecture (the label's part
   after `/`) as the environment the consuming target declares. Where
   either declares none (development mode), offers are not filtered.
   Content offers are never filtered. Rejected offers are listed among
   what the resolver considered. *Amended by §53:* the offer's side is
   the environment its artifacts run in, the object's, and the
   consumer's the one its build runs in, which a target may name apart
   from its object's.

9. **File sources federate like Git sources** (§37 sub-decision 9).
   *Added when built:* a held file source can be published; its
   closure is every observation of it, every observer, and its bytes as
   the blob its hash names, and its genesis route serves the unsigned
   `FileSourceGenesis`. Pulling one (`kairo fetch --object <id>`, or
   `kairo source pull <sha256> --from <peer>`) checks the genesis by
   re-deriving the id from the hash, verifies each observation under
   its signer, and checks the bytes against the hash, within
   `max_file_bytes` rather than the manifest blob limit.
   Publishing a native object also serves the file sources its
   revisions pin, whole, and pulling it brings them, so a pulled
   wrapper such as live-bootstrap's can plan and build.

10. **Pushes carry sources too** (§26 sub-decisions 6, 8). *Added when
    built:*
    - A Git or file source can be pushed like any object: the plan and
      the bundle carry its unsigned genesis like an object's, and the
      receiver ingests it as a pull would.
    - Pushing a native object also carries the sources its revisions
      pin, as a pull from the pusher would serve them: a Git source's
      observations and only the pinned commits' history, a file source
      whole. The plan request lists each source's inventory
      (`sources: [{ object, inventory }]`, at most 1024) and the plan
      answers for each (`sources: [{ object, plan }]`); the bundle
      carries each source with anything to send as a bundle of its own
      under `sources/<source id>/` (its own `manifest.json`, records,
      and Git pack; no further `sources/`), and the top manifest lists
      them (`sources`).
    - The native object's accept entry and the pusher's
      `federation:push` trust cover the sources it carries. After
      ingesting the object, the receiver reads its own copy of the
      manifests and ingests only the carried sources they pin, each at
      exactly the pinned commits; anything else in `sources/` is
      ignored.
    - Wire format: the plan request, the plan, and the closure bundle
      manifest gain the optional `sources` fields; a receiver that
      predates them refuses a request or bundle that uses them.

11. **The milestone extends through bash-2.05b.** *Added when built:*
    after tcc-0.9.27 the wrapper runs every remaining step
    live-bootstrap builds with kaem (make 3.82, patch 2.5.9, gzip
    1.2.4, tar 1.12, sed 4.0.9, bzip2 1.0.8, coreutils 5.0, oyacc 6.6,
    bash 2.05b), with its `improve: reconfigure` step, nine more
    distfiles pinned as file sources, and the manifest truncated after
    `build: bash-2.05b`, the lines kept verbatim. Every step's
    upstream checksums pass under uid 1000; nothing before bash needs
    root. The bash-run steps that follow (`setup_repo`, `merged_usr`,
    `populate_device_nodes`, ...) are the next milestone.

12. **Then through the first musl toolchain.** *Added when built:*
    the wrapper runs on into live-bootstrap's bash-run steps: its
    `setup_repo`, `update_env`, `merged_usr`, and
    `populate_device_nodes` improvements, then tcc and musl 1.1.24
    rebuilt against each other three times, grep 2.4, and sed and
    bzip2 rebuilt on musl, stopping after the second `bzip2-1.0.8`.
    It needs §39: `merged_usr` makes `usr/sbin -> bin`, the installs
    make absolute links such as `bunzip2 -> /usr/bin/bzip2`, and the
    guest runs as root. `populate_device_nodes` creates nothing: the
    contract's `/dev` already has every node it checks for. Each of
    these steps also packages what it installed and checks the package
    against live-bootstrap's `SHA256SUMS.pkgs`; every one matches, so
    the packages, kept as the artifact's `repo` output, are
    byte-for-byte upstream's.

13. **Then through perl 5.6.2.** *Added when built:* on through m4,
    heirloom-devtools, both flex builds, three bison builds, diffutils,
    coreutils 5.0 again and 6.10, gawk, and the perl chain 5.000,
    5.003, 5.004_05, 5.005_03, 5.6.2, stopping after the manifest's
    second `uninstall:`, of the four older perls. `uninstall:` is
    live-bootstrap's own: a `helpers.sh` function, run in the guest,
    that removes an earlier package's files from `/` (keeping any a
    later step overwrote). Kairo provides nothing for it; the artifact
    is what is left, and the removed tools' packages stay in `repo`.
    Thirteen more distfiles are pinned; heirloom-devtools' is fetched
    over `https` from SourceForge, since upstream lists it over
    `http` and its mirrors serve other bytes. All 25 packages match
    `SHA256SUMS.pkgs`. The build needs more than the default hour, so
    the end-to-end test raises `runtime.timeout_secs`.

14. **Then through the first gcc.** *Added when built:* on through
    the autotools chain (nine autoconfs from 2.52 to 2.69, eight
    automakes from 1.6.3 to 1.15.1, help2man, libtool 2.2.4), binutils
    2.30, musl and tcc rebuilt once more, `populate_device_nodes`
    again, and gcc 4.0.4, the first GCC built from the seeds, stopping
    after its first build. Twenty-two more distfiles are pinned; for
    any listed on kernel.org's GNU mirror, `import.sh` also tries
    `ftp.gnu.org`, since one slow or unreachable mirror shouldn't stop
    an import. All 49 packages match `SHA256SUMS.pkgs`, gcc's
    included.

15. **Then through upstream's CI pass 1.** *Added when built:* on
    through findutils, musl 1.2.5, the Linux 4.14 headers, gcc 4.0.4's
    second build, util-linux, e2fsprogs, dhcpcd, kbd, make, ed, and bc,
    with live-bootstrap's `clean_sources` and `clean_artifacts`
    improvements, stopping before `jump: break ( INTERNAL_CI == pass1
    )`, where upstream's own CI ends its first pass. The kernel steps
    are skipped by the `bootstrap.cfg` conditions (`CHROOT=True`).
    Seventeen more distfiles are pinned. One, `gnulib-8e128e.tar.gz`,
    has no upstream URL: live-bootstrap's `mirror.sh` makes it with
    `git archive` from gnulib's Git history, and the archive pins the
    copy archive.org serves, which hashes as published. util-linux
    installs `libblkid.so -> ../../usr/lib/i686-unknown-linux-musl/`,
    with a trailing `/`, so §39 sub-decision 2 now ignores empty
    segments in relative targets. All 60 packages match
    `SHA256SUMS.pkgs`. The end-to-end test raises `file_timeout_secs`
    for the 100 MB Linux tarball.

16. **Then into CI pass 2, through zlib.** *Added when built:* on
    through `finalize_job_count`, `finalize_fhs`, musl 1.2.5 again,
    curl, bash 5.2.15, xz, file, libtool 2.4.7, tar 1.34, coreutils
    9.4, pkg-config, make 4.2.1, gmp, autoconf-archive, mpfr, mpc, flex
    2.5.33, bison 2.3, and zlib, stopping before the perl chain.
    `finalize_fhs` creates `/etc` and the rest of the FHS skeleton and
    mounts what `mount` doesn't list; under `linux-root` that is only
    `/sys`, which the guest, as root in its namespaces, can mount
    itself. Forty-one more distfiles are pinned. Most of upstream's URLs
    are plain `http`; their `https` forms serve the same bytes, except
    that `file-5.44` is fetched from `astron.com` (its listed
    `ftp.astron.com` serves other bytes over `https`), and four gnulib
    snapshots and coreutils 9.4's generated tarball are archive.org's
    copies, which hash as published. All 77 packages match
    `SHA256SUMS.pkgs`.

Rationale: live-bootstrap is the canonical path from stage0 to a real
toolchain, and running it unmodified keeps the strongest evidence
available, agreement with upstream's published hashes. Identifying
files by content is the only identity that survives mirrors and
link rot; giving the guest its tree as `/` is the honest way to run
software that assumes an FHS root, and naming that as its own contract
keeps it from being mistaken for the `linux` contract's results.

Affected specs:

- `schemas/canonical/` — file source genesis and observation.
- `schemas/environment/linux-root-v1.md` (new).
- `OBJECT.md` §4.4, `object-manifest-v1.md` — `kind = "file"`;
  `from`, `overlay`, and `path` on artifact dependencies.
- `CLI.md` — `kairo source fetch-file`.
- `PHASE_2.md` §7 — the second milestone.

---

## 39. Symbolic Links in Artifacts, and Root in `linux-root`

Decision: artifacts can hold **symbolic links**, under rules that keep
them inside the artifact and keep every write from following one; and
the `linux-root` contract's guest runs as **root inside its own
namespaces**. Both come from the first bash-run steps of live-bootstrap
(§38 sub-decision 11), but neither is specific to it: most software
installs symlinks, and software that assumes it owns `/` assumes it is
root there.

Sub-decisions:

1. **A link entry.** An `artifact-tree-v1` entry is either a file, as
   before (`path`, `blob`, `executable`), or a link: `{ "path": ...,
   "link": "<target>" }`, fields in that order, sorted with the files
   by `path`. A tree without links encodes exactly as before, so no
   existing artifact id changes.

2. **Relative targets stay inside the artifact; absolute targets are
   the guest's.** A relative target (no NUL) names a path in the
   artifact: resolved lexically from the link's own directory, it
   never climbs above the artifact's root (`..` within it is fine;
   empty segments, from a trailing `/` or `//`, mean nothing, as in
   POSIX path resolution), and one that escapes fails the build with
   the link named. An absolute target names a path in the filesystem of
   whatever guest uses the artifact and is kept exactly as written:
   under `linux-root` it resolves in the build tree, under `linux` it
   may dangle. It is never rewritten. Any target may dangle; the file
   it names may come from another artifact placed beside it. *Amended
   when built:* the first draft refused absolute targets, but upstream
   build systems make them routinely (bzip2's `bunzip2 ->
   /usr/bin/bzip2`, binutils' triplet links), and Kairo never follows
   a link of either kind (sub-decision 3), so recording them costs no
   safety.

3. **Nothing is written through a link.** No entry's path may lie
   under a link entry's path; a tree that has one is refused, on
   collection and on load. Collecting an output records a link and
   never follows it. Materializing an artifact and placing a
   dependency create links as links, after checking no part of the
   destination path is one (§38 sub-decision 4's rule, now applied to
   links an artifact brings). So a link can't make Kairo write outside
   a build directory or an artifact's destination.

4. **Root in `linux-root`.** Under `environment:linux-root/<arch>` the
   guest runs as uid 0 and gid 0 with every capability, in its own
   user, mount, PID, IPC, UTS, network, and cgroup namespaces. On the
   host it is still the unprivileged user who runs Kairo. What that
   gives, and what it doesn't:
   - files can be chown'd to 0, and checks for root pass;
   - tmpfs, proc, and the like can be mounted inside the guest's own
     mount namespace;
   - device nodes can't be created (the kernel refuses `mknod` of
     devices in a user namespace), and files can't be owned by any id
     but 0;
   - the minimal `/dev`, `/proc`, and empty `/tmp` are as before, and
     `/sys` is empty (sub-decision 6).

   Software that needs real device nodes, other ids, `/sys`, or a
   kernel of its own belongs on a future microVM engine under its own
   contract. The `linux` contract keeps uid 1000.

5. **An amendment, not a new label.** §34 sub-decision 1 makes a
   change that could alter output a new contract label. `linux-root`
   v1 is amended in place instead, because nothing built under it has
   left this repository: it was defined in §38 and none of its results
   are published. Once a contract's results are shared, this rule
   applies to it in full.

6. **An empty, read-only `/sys` under `linux-root`.** *Added when
   built:* live-bootstrap's `finalize_fhs` mounts whatever `mount`
   doesn't list, and the only thing `linux-root` lacked was `/sys`, so
   a guest that is root in its namespaces mounted the host kernel's
   sysfs and could read the host's devices and CPUs. The contract now
   mounts an empty, read-only tmpfs at `/sys`, which `mount` lists, so
   such scripts leave it alone and see nothing of the host. A guest
   that mounts sysfs elsewhere on purpose still can; the contract says
   so under what it doesn't promise. Like sub-decision 4, an amendment
   to v1 while nothing built under it is published.

Rationale: symlinks are ordinary output, and refusing them blocks most
real installs; confining their targets lexically and never writing
through one keeps the guarantee that a build or a placement touches
only its own directory. Root inside a user namespace is free on the
host and is what software that owns `/` expects; the cases it can't
cover (devices, many ids, sysfs) are exactly the ones a VM engine
covers, so they wait for it rather than being half-emulated.

Affected specs:

- `schemas/canonical/artifact-tree-v1.md` — link entries.
- `schemas/environment/linux-root-v1.md` — identity and capabilities.
- `BUILD.md`, `EXECUTOR.md` — outputs may contain links.
- `DECISIONS.md` §33 sub-decision 6 (artifacts), §38 sub-decision 5.

---

## 40. Mirrors for Source Fetches

Decision: `kairo source fetch` can fall back to a **mirror** of a
source, a different URL believed to serve the same repository, when
the source's own URL can't be fetched; what it fetches is still filed
under the source's own id, and its observation says where it came
from.

Sub-decisions:

1. **Fallback only.** A mirror is tried only after fetching from the
   source's own URL fails, for the top source or any submodule; a
   fetch that works isn't redirected. The whole fetch of that source
   (its refs and its pinned commits) then goes through the mirror.

2. **Identity is unchanged.** The source is the one its canonical URL
   names (§37 sub-decision 1); a mirror never becomes a source of its
   own by being fetched through. Git objects are named by the hash of
   their content, so a pinned commit fetched from a mirror is exactly
   the commit the parent's gitlink names, or the fetch of it fails.

3. **The observation names the mirror.** `SourceObservation` gains an
   optional `via` (`schemas/canonical/source-observation-v1.md`),
   present only for a fetch through a mirror, so its claim is
   "fetching this source through that mirror, I saw these refs". A
   mirror may lag its upstream, so its refs are evidence about the
   mirror; nobody is told they are upstream's. Observations without
   `via` encode as before.

4. *Added:* **Retried before falling back.** A fetch, of the top source
   or a submodule's pinned commits, that git gives up on mid-transfer
   (the `http.lowSpeedLimit` abort, a dropped connection, a pack that
   ends early) is run again from the start, up to three attempts, five
   then fifteen seconds apart; only then is the source's fetch counted
   as failed and a mirror tried. A request that is wrong rather than
   interrupted (a missing repository, refused authentication, an
   unknown commit) fails at once, and the size and time limits are
   never retried.

5. **Mirrors are named explicitly.** `--mirror <source url>=<mirror
   url>`, repeatable, both canonicalized as source URLs; nothing is
   guessed. The stage0-posix import names the maintainer's GitHub
   mirror of mescc-tools, whose Savannah home was unreachable during
   the work on §38.

Rationale: a source archive shouldn't depend on one host being up,
and content addressing makes a mirror's commits as good as the
original's. What a mirror can't vouch for is the original's refs, so
the observation says which host it read them from rather than letting
a mirror stand in for upstream silently.

Affected specs:

- `schemas/canonical/source-observation-v1.md` — `via`.
- `CLI.md` §25.1 — `--mirror`.

---

## 41. One Linux Contract, Package Roots, Child Objects, and Overlay Provisioning

Decision: there is **one Linux environment contract**, in which the
guest is root in its own namespaces and `/` is an overlay of its
dependencies; a build's artifact is what it stages under
**`/kairo/package`**, with an **install root** saying where it lands
for the objects that use it; one build can define **child objects**,
first-class objects whose ids derive from their parent's, each with its
own artifact; and dependencies are **provisioned by overlay** wherever
the engine can, by copying otherwise. This replaces §38 sub-decision 5's
separate `linux-root` contract and the "build tree as `/`" layout, and
generalizes how artifacts are mounted (§35).

Sub-decisions:

1. **One contract.** `environment:linux/<arch>`
   (`schemas/environment/linux-v1.md`) is amended to the following, and
   the `linux-root` label is retired; nothing built under either has
   been published (§39 sub-decision 5). The guest runs as uid and gid 0
   with every capability inside its own namespaces (§39 sub-decision
   4), and sees:

   | Guest path | Contents | Access |
   |---|---|---|
   | `/` | an overlay: each dependency's artifact at its install root (sub-decision 3), earlier-listed dependencies winning where two provide the same path, over a writable layer for this build | read-write |
   | `/kairo/src` | the snapshot's tree, with relative placements in it; the command runs in `/kairo/src/<workdir>` | read-write |
   | `/kairo/package` | empty; the build stages its artifact here (sub-decision 2) | read-write |
   | `/kairo/artifact/<id>` | each dependency whose artifact declares no install root | read-only |
   | `/proc`, `/dev`, `/tmp`, `/sys` | as now: the sandbox's processes, the minimal devices, an empty `/tmp`, an empty read-only `/sys` (§39 sub-decision 6) | — |

   Placements with an absolute `path` land at that guest path, in the
   writable layer; relative ones land in `/kairo/src`, as before.
   Everything else in the contract (no network, hostname `kairo`, the
   environment variables, the clock not frozen) is unchanged.

2. **The artifact is `/kairo/package`.** A build stages its artifact
   under `/kairo/package`, laid out as it wants to be installed (bash at
   `/kairo/package/usr/bin/bash`); the tree there after the command
   exits is the artifact. A target's `[[build.targets.outputs]]` still
   work: each copies a path from the working directory into the package
   at its `artifact_path`, after the command, so existing manifests keep
   their meaning. Nothing outside `/kairo/package` is ever collected:
   not the provisioned dependencies, not `/kairo/artifact`, not the
   rest of the writable layer. A build that installs into `/` itself,
   as live-bootstrap does, copies what it means to ship into
   `/kairo/package` at the end.

3. **Install roots.** A target declares where its artifact lands in the
   environments of the objects that use it:

   ```toml
   [build.targets.artifact]
   root = "."        # a subdirectory of /kairo/package; the default
   install = "/"     # an absolute guest path; the default
   ```

   `install = "/"` means the artifact's `usr/bin/bash` is `/usr/bin/bash`
   for a consumer. `install = false` keeps the artifact out of the
   overlay, at `/kairo/artifact/<id>`, for tools that are only invoked
   by path. A consumer's own `path` placement (§38 sub-decision 4)
   still overrides where it lands.

4. **Child objects.** One build can define several objects, as Occam
   did:

   ```toml
   [[build.targets.children]]
   name = "bash"
   root = "bash-5.2.15"      # under /kairo/package
   install = "/"
   [[build.targets.children.provides]]
   provides = "tool:bash"
   version = "5.2.15"
   ```

   - **Identity.** A child's genesis is `(parent object id, target,
     name)`, unsigned, and its id derives from it under its own domain,
     as a source's derives from its URL (§37 sub-decision 1). The
     parent's signed manifest declares it, so the parent's signer
     vouches for it.
   - **History.** A child exists at every parent revision whose
     manifest declares it; it has no history or branches of its own.
     Software that should evolve on its own deserves its own object.
   - **Artifact.** A child's artifact is the part of its parent's
     artifact under its `root`, re-rooted there, with its own
     content-addressed artifact id; its build result is its parent's.
   - **Like any object.** A child can be depended on by id or by its
     provides labels, searched, followed, and pulled; it carries its
     own install root, provides, and runtime dependencies. Its relation
     to a dedicated object for the same software is the label's: both
     may provide `tool:bash`, and policy chooses.

5. **Provisioning by overlay.** The engine assembles the guest's `/`
   as an overlay: each dependency's artifact is materialized once on
   the host as a layer laid out from `/` (so keyed by artifact id and
   install root), cached and shared read-only across builds, and
   stacked in dependency order, the first dependency on top; the
   build's writable layer is on host disk, with overlayfs's work
   directory in the same filesystem, and the absolute placements are
   copied into it before the command runs. Deletions a build makes in
   the overlay (whiteouts) never reach an artifact: artifacts only hold
   what is staged under `/kairo/package`. *Amended:*
   - **bubblewrap's own overlay, from 0.11.** bubblewrap 0.11.0 added
     `--overlay-src` and `--overlay`, mounting with `userxattr`
     (Linux 5.11 or later, and not in setuid installs); the engine
     uses them when its probe finds them. No helper runs inside the
     sandbox: a Kairo program in every guest would be one more thing
     the guest didn't ask for, and the copy below already gives the
     same tree.
   - **Copying otherwise.** On an older bubblewrap, or when an overlay
     can't be mounted, the engine copies the dependencies into the
     build's `/` in the same order, the first one winning, as slice 1
     does. The guest sees the same tree either way, so the execution
     record says which was used and the plan doesn't.
   - **Short layer names.** The kernel takes the whole layer list in
     one page of mount options. bubblewrap 0.12 names each layer by a
     short `/proc/self/fd` path, but 0.11 passes the host path, which
     for a cached artifact is about 100 bytes, so some 40 layers. The
     engine therefore names layers through a per-build directory of
     short symbolic links; a stack that still doesn't fit is copied.
   - **The development host.** Ubuntu 24.04 ships bubblewrap 0.9.0
     with no backport; 26.04 ships 0.11.1, Debian 13, Fedora 44, and
     Alpine 3.24 ship 0.12.0. A host on 24.04 can build 0.12 into a
     directory of its own and allowlist it under `[[runtime.engines]]`
     without replacing the system's.

6. **live-bootstrap.** Its wrapper places live-bootstrap's steps and
   seed files at their absolute paths (`/steps`, `/`) and its distfiles
   at `/external/distfiles`, runs as now, and at the end extracts each
   package in `/external/repo` into `/kairo/package/<package>/`; each
   package is a child object rooted there, installed at `/`, providing
   the tools it holds. So a later object can depend on exactly
   live-bootstrap's bash, or gcc and its toolchain, at their real
   paths. The kaem-built steps, which make no packages, stay part of
   the parent.
   - **Which children carry labels.** live-bootstrap rebuilds some
     packages (`tcc-0.9.27_0` through `_3`, `musl-1.1.24_0` through
     `_3`), and two revisions of one version would be two offers no
     version requirement tells apart. So only the last revision of each
     package name and version provides its label, with the version;
     earlier revisions are children reachable by id. Different versions
     of one tool (autoconf 2.52 to 2.69, perl 5.000 to 5.6.2) each
     provide it, as live-bootstrap's own later steps use particular
     ones. The labels the parent provided for packaged tools move to
     the children, so the parent and a child never offer the same one.

Rationale: a contract per filesystem layout multiplies labels for one
idea; root in the guest's namespaces costs the host nothing, and an
overlay root lets every build see its dependencies where they expect
to be without copying them. Staging the artifact apart from the
environment keeps an artifact to what was built, never what was
provisioned. Child objects let one build yield many pieces of
software, each with an identity and metadata of its own, so that
consumers take what they need rather than the whole build.

Affected specs:

- `schemas/environment/linux-v1.md` — the amended contract;
  `linux-root-v1.md` retired.
- `schemas/canonical/object-manifest-v1.md` — `[build.targets.artifact]`
  and `[[build.targets.children]]`.
- New `schemas/canonical/child-genesis-v1.md`.
- `BUILD.md`, `EXECUTOR.md`, `ENVIRONMENTS.md` — package roots,
  install roots, overlay provisioning.
- `OBJECT.md` — child objects.
- `DECISIONS.md` §35, §38 sub-decisions 5–6, §39 sub-decision 4.

---

## 42. Label Kinds, and Summaries and Tags for Search

Decision: what a piece of software **gives** a consumer is said by the
**kind** of each provides label it declares (`tool:`, `lib:`, `doc:`,
`data:`, `environment:`), from a registered set; what it **is**, for
people looking for it, is said by an optional `summary` and `tags`,
on native objects and on child objects alike. The resolver reads only
labels; search reads both. This settles how a child is typed (§41
sub-decision 4) and gives `SEARCH.md`'s `object.tags` a field to come
from.

Sub-decisions:

1. **Role belongs to labels, not to objects.** One package often holds
   several kinds of thing (a library, its headers, its manual), so a
   single type per object or per child would be wrong as often as
   right. Each label carries its own role, and a child (or object)
   providing `lib:zlib`, `lib:zlib:headers`, and `doc:zlib` is all
   three. A build that does stage roles apart can declare a child per
   role (`zlib`, `zlib-dev`, `zlib-doc`, with different roots); the
   labels say the same thing either way. `[kairo] kind` (`software`,
   `tree`) is unchanged: it says what kind of object this is, a
   different question, and children have none.

2. **Registered label kinds.** A provides label is
   `kind:specifier(:detail)*` (§33), and `kind` is one of:

   | Kind | Provides | Details by convention |
   |---|---|---|
   | `tool` | programs a build or user runs | — |
   | `lib` | libraries to compile or link against | `static`, `shared`, `headers`, `source` |
   | `doc` | documentation | `man`, `info`, `html` |
   | `data` | files programs read: datasets, game data, programs for an emulator | the format, as `data:exe:mz` already does |
   | `environment` | an environment contract (§34) | `<os>/<arch>` as the specifier |

   A manifest whose `[[provides]]`, child `provides`, or `kind =
   "provides"` dependency names any other kind is refused, so a
   misspelt `libs:zlib` fails at parse time instead of never
   resolving. The specifier must be non-empty; specifiers and details
   are otherwise free and matched exactly, as before. A new kind is
   added by a decision, not by use. Every label in the repository
   today is already one of these kinds.

3. **`summary` and `tags`.** `[kairo]` gains `tags`, and each
   `[[build.targets.children]]` entry gains `summary` and `tags`
   (`[kairo]` already has `summary`):

   ```toml
   [kairo]
   schema = 1
   kind = "software"
   name = "live-bootstrap"
   summary = "A full bootstrap from a few hundred bytes of seed."
   tags = ["bootstrap", "toolchain"]

   [[build.targets.children]]
   name = "bash-5.2.15"
   root = "bash-5.2.15"
   summary = "GNU Bourne-Again shell."
   tags = ["shell"]
   provides = [{ provides = "tool:bash", version = "5.2.15" }]
   ```

   A tag is lowercase letters, digits, and `.`, `+`, `-`, starting with
   a letter or digit; a list holds no repeats and keeps its order.
   They are part of the manifest hash (so the signer vouches for them)
   and encoded only when present, so a manifest without them keeps its
   hash. The resolver ignores them.

4. **No inheritance.** A child doesn't take its parent's summary or
   tags: a description of live-bootstrap is wrong for its bash. Search
   shows a child with its parent, which is context enough.

5. **Search.** The local index covers each object's and each child's
   name, summary, tags, and provides labels; a child is a result of
   its own, carrying its parent's id. Queries add `tag:<tag>`,
   `provides:<label>` (exact), and `provides:<kind>:*` (every label of
   that kind), so `provides:doc:*` finds documentation and
   `provides:lib:* tag:compression` finds compression libraries.
   Results stay candidates, never claims of validity (`SEARCH.md` §2).

Rationale: the resolver already treats labels as the contract between
a consumer and a provider, so they are where the role must live;
registering their kinds turns a convention into something a manifest
can be checked against. Descriptions and keywords help people, not the
resolver, and are kept out of resolution so that editing a summary
never changes what a build gets.

Affected specs:

- `GLOSSARY.md` — provides label kinds.
- `schemas/canonical/object-manifest-v1.md` — `[kairo] tags`; child
  `summary` and `tags`; label kind validation.
- `OBJECT.md` §2.5, §4.1 — tags and summaries.
- `SEARCH.md` §4–§6 — indexed fields, filters, child results.

---

## 43. The Highest Version by Default

Decision: when more than one offer satisfies a requirement, the
resolver takes the **highest version**; and an object dependency that
gives neither `version` nor `snapshot` selects the object's **highest
version tag**, instead of being refused.

Sub-decisions:

1. **Among offers.** A `kind = "provides"` requirement resolves to the
   highest version among the offers whose versions satisfy it, versions
   read leniently (`2.69` as `2.69.0`). Offers whose version doesn't
   parse, or that declare none, come after every versioned one; among
   equals, the index's order stands (artifact providers before content
   providers, then object id). *Amended by §48:* owners rank before
   versions, and among equals the newest published comes before object
   id. Before this the choice among satisfying
   offers was by object id, which for live-bootstrap's nine autoconf
   children (§41 sub-decision 6) meant an arbitrary one.

2. **Within a target.** Where a target requires one label more than
   once, an offer that satisfies all of those requirements comes before
   one that satisfies only the requirement being resolved, so an
   unversioned `tool:autoconf` beside `tool:autoconf` `<2.60` takes the
   highest version both accept. The rule looks no further than the
   target: a provider's own build is planned on its own, and the
   resolver doesn't search for one set of versions across a whole plan.
   *Amended by §46 sub-decision 4:* a requirement no offer satisfies
   together with the others on its label is a conflict, not a second
   provider, unless the providers co-exist.

3. **Object dependencies.** `kind = "object"` with only `object` means
   the highest of the object's version tags, as `version = "*"` would
   but without saying so; it encodes as no selector, so manifests that
   give one keep their hashes. An object with no version tags is
   unsatisfied, and the diagnostic asks for a snapshot. The plan pins
   the snapshot chosen either way, so a later tag changes a new plan,
   never an old one.

Rationale: the highest version is what a consumer that names no version
almost always means, and making it the rule turns an arbitrary choice
into a predictable one. Requiring a selector on every object dependency
made the common case verbose without making it safer, since the plan
pins what was chosen.

Affected specs:

- `OBJECT.md` §4.4 — object dependency selectors.
- `schemas/canonical/object-manifest-v1.md` — `ObjectDependencySelector`.

---

## 44. Child Versions, Local Search, and Children over Federation

Decision: a child object's **version** is declared with it, in the
manifest its parent's revision signs, and selecting a child by id with
a version (or none) chooses among the parent's current and
version-tagged snapshots by that version; **`kairo search`** finds
objects and children by name, summary, tags, and labels by scanning
what the store holds; and a command given a child's id for
**fetching, following, or publishing** acts on its parent. This
completes §41 sub-decision 4's "like any object" and amends §43
sub-decision 3 for children.

Sub-decisions:

1. **A child's version is its own.** `[[build.targets.children]]`
   takes an optional `version`, the version of the software the child
   holds (bash's `5.2.15`, not live-bootstrap's). A `provides` entry
   without a version of its own takes the child's, so the manifest
   says the version once; the canonical form records the child's
   version when given, and each label's after the default is applied.
   Children need no `ObjectVersionTag`: the parent's signer vouches
   for the version as for everything else in the manifest, and a child
   still has no statements of its own. The parent's own version tags
   say nothing about a child's version and aren't matched against it.

2. **Selecting a child by id.** For `kind = "object"` naming a child:
   - with `snapshot`, that snapshot of the parent, whose manifest must
     declare the child;
   - with `version`, the parent's current snapshot and each of its
     version-tagged snapshots that declare the child at a version
     satisfying the requirement; the highest child version wins, and
     between equal versions the current snapshot, then the higher tag;
   - with neither, the same, taking the highest child version (§43);
     a child declared without a version is found this way only when no
     declaration of it has one.
   The parent is found from the current manifests the resolver
   indexes; failing that, from the manifests at each held object's
   version-tagged snapshots, or, with `snapshot`, the object that
   snapshot belongs to. So a child that only an older, tagged or
   pinned, revision of its parent declares still resolves.

3. **Local search.** `kairo search <query>` covers native objects at
   their current snapshots and their children: name, summary, tags,
   provides labels and versions, and a child's version. Free words
   match name, summary, and tags without regard to case; `tag:<tag>`,
   `provides:<label>`, and `provides:<kind>:*` filter (`SEARCH.md`
   §5); unknown filters are ignored and reported. Each result carries
   its id, name, summary, tags, provides, the parent of a child, and
   `source = local`, `validation = unverified` (`SEARCH.md` §6). There
   is no index yet: a query reads what the resolver's index reads, so
   a store's own size bounds its cost, and an index backend waits for
   query patterns that need one (`STORE.md`). Federated search stays
   with discovery (`CLI.md`).

4. **Children over federation.** A child has no statements, so
   `kairo fetch`, `kairo federation follow`, `unfollow`, `publish`,
   and `unpublish` given a child's id act on its parent, found as in
   sub-decision 2, and say so (`child = <id>`); a peer that serves the
   parent serves its children, whose artifacts travel with the
   parent's execution records (§41 slice 2a). The parent is found from
   the manifests this node holds, or, for `kairo fetch --from` of a
   child whose parent this node has never held, from the peer (§45).
   An id neither can place is taken as given.

Rationale: the version a consumer asks of bash is bash's, so it must
be the child's own; declaring it in the parent's manifest keeps
children free of statements while letting the parent's history carry
every version a child has had. Scanning for search is enough until a
store is large enough to need more, and saves committing to an index
format before anything queries it.

Affected specs:

- `schemas/canonical/object-manifest-v1.md` — child `version`.
- `OBJECT.md` §2.5 — child versions and selection.
- `SEARCH.md` — the local query and result.
- `CLI.md` — `kairo search`; child ids in federation commands.
- `PHASE_2.md` §9 — what's searchable, and no index yet.

---

## 45. Peers Say Which Object a Child Belongs To

Decision: a node serves **`GET /federation/v1/children/{id}`**,
answering for each child object that some revision of one of its
published objects declares, with the parent, target, and name its id
derives from; and `kairo fetch --from <peer>` of an id this node can't
place asks the peer, checks the answer, and pulls the parent. This
closes the gap §44 sub-decision 4 left: a child's id is a hash, and
only a node holding the parent's manifest can say whose child it is.

Sub-decisions:

1. **The answer proves itself.** The response is `{ schema:
   "kairo.fed.child.v1", child, parent, target, name }`. The client
   re-derives the child's id from `(parent, target, name)`
   (`schemas/canonical/child-genesis-v1.md`) and refuses an answer
   that doesn't give the id it asked about, so a peer can't redirect a
   fetch to some other object; nothing about the answer needs trust.
   What is then pulled from the parent is checked as every pull is.

2. **From what the node publishes.** A node answers only for children
   of objects it publishes, read from the manifest at every revision
   in the published closure, so a child an older revision declares is
   answered for too. Anything else is `404 fed_not_found`, as for an
   unpublished object; a node lists no children, so the endpoint
   reveals nothing a client couldn't learn by pulling the parent.

3. **Advertised and optional.** Nodes that serve it list
   `"children"` in their node document's `supports`; a client asks
   only such nodes, and treats a `404` as "not a child here", taking
   the id as given.

4. **Only when needed.** `kairo fetch` asks the peer only when the id
   is neither an object this node holds nor a child of one: the local
   answer (§44) needs no request.

Rationale: a child is fetched through its parent, and a node that has
never seen the parent has nothing to derive the parent from. The peer
that publishes the parent can say, and because the claim is checkable
from the id alone, saying it costs the client no trust.

Affected specs:

- `PHASE_2_FEDERATION.md` — the endpoint and the `supports` value.
- `CLI.md` — `kairo fetch` of a child.

---

## 46. Runtime Requirements and What a Build Confers

Decision: an artifact can declare what it **requires** wherever it is
installed, and a provides label can declare what using it at build
time **confers** on the artifact built, as a runtime requirement; the
resolver gathers both across the plan's whole closure and chooses
**one offer per label** that satisfies every requirement on it, or
installs more than one only where the providers say their versions
**co-exist**, and refuses the plan, naming both chains, otherwise.
Runtime requirements are computed when a build is planned and
recorded in its plan, so a consumer of its artifact reads them from
the plan that built it and declares nothing.

Sub-decisions:

1. **`requires`: wherever the artifact is installed.** A build target
   or child declares `requires = [{ provides, version? }]`: labels
   its artifact needs beside it, at build time or at run time. gcc
   requires `tool:binutils` (it runs `as` and `ld`) and its C library's
   headers and startup files; a script requires its interpreter. Each
   is a provides requirement like any other, with a version requirement
   or none.

2. **`confers`: what using a label passes on.** A provides entry, of an
   object or a child, declares `confers = [{ provides, version? }]`:
   what a build that resolves this label gets as a runtime requirement
   of its own artifact, without declaring it. It belongs to the label,
   not the object: `tool:g++` confers `lib:stdc++`; `tool:gcc` confers
   `lib:gcc`; a library's `lib:zlib` confers `lib:zlib`, since linking
   against it means needing it, while `lib:zlib:static` and headers
   confer nothing. Nothing is conferred that a manifest doesn't say:
   using make at build time doesn't make an artifact need make, and
   nothing is inferred from binaries.
   - **No version means this build.** A conferred entry without a
     version pins the label to the provider's own build: the sibling
     (the object itself, or one of its children at the same snapshot)
     that provides that label, exactly what the artifact was linked
     against. A provider whose library doesn't promise compatibility
     across versions says nothing more.
     *Amended by §51:* only a label conferring itself may omit the
     version, and the pin is to the offer the build resolved for it; a
     conferral of another label gives a range. Only the build's own
     direct dependencies confer, and the build may say otherwise with
     `confer`.
   - **A version means a range.** `version = ">=11.4"` says any
     offer satisfying it will do, which is how a provider states the
     forward compatibility the gcc runtime libraries and glibc keep;
     the manifest's author decides, never a default.
   - Conferred requirements apply to the target's artifact and to each
     of its children, which were built in the same environment.

3. **Computed at plan time, read from the plan.** Which provider a
   label resolved to is a fact of the plan, so the resolver records in
   each plan what the artifact it builds will require at run time:
   the target's and children's declared `requires`, and what each
   resolved build dependency conferred, each with `via` saying which
   label brought it. A consumer whose requirement resolves to the
   artifact reads these from the plan that built it, which the build
   result cites and the store holds, or from the prerequisite plan when
   the build is still to come; a child's are its parent's plan's
   entries for the target and for that child. Runtime requirements are
   themselves artifacts with requirements, so the reading recurses.

4. **Unified, one offer per label.** Every requirement on a label in
   the closure, the consumer's own, the declared, and the conferred, is
   a constraint; the resolver chooses the highest offer that satisfies
   all of them (§43 extended from the target to the closure). A
   constraint that arrives after a label was chosen re-chooses it, and
   what the earlier choice brought is recomputed, until nothing
   changes. A pin satisfies a range when its version does; two
   different pins on one label, or a pin outside a range, can't be
   satisfied by one offer. `>=11` and `>=13` on `lib:stdc++` give one
   libstdc++ at 13.

5. **Co-existing versions.** A provides entry may declare
   `coexists = true`: its files are versioned so that other versions
   of the label can be installed beside it at the same root. When no
   one offer satisfies every constraint on a label and every offer the
   resolver would install declares `coexists`, it installs more than
   one, each serving the constraints it satisfies; the executor refuses
   the build if two of them share a file path, since one would
   silently shadow the other. How a built artifact targets the
   versioned name at link time is the provider's business. Offers at
   different install roots co-exist without saying so.

6. **Otherwise a conflict, and the plan is refused.** The error names
   the label and every constraint on it with its chain
   (`lib:stdc++ pinned to gcc-11.4.0's by tool:foo, built with tool:g++
   from object:...; >=13 by tool:bar ...`). The consumer's own
   requirement is one constraint among these, not an override; a way
   to force a choice is a later feature if one is ever needed.

7. **Installed with what needs them.** A plan lists the runtime
   closure of its build dependencies as requirements of the build
   environment, after the declared ones, each with `via`; the executor
   installs them like any requirement. Build dependencies' runtime
   requirements are not conferred onward unless a `confers` entry says
   so.

Rationale: a build-time choice fixes what the artifact needs at run
time, and the one place that knows the choice is the plan. Letting the
provider say what it confers, once, spares every consumer from
knowing, while keeping compatibility claims where the knowledge is:
with the library's own manifest, as a range, or absent, as a pin.
Unifying across the closure is what lets the satisfier find the one
version that serves everyone when there is one, and say clearly when
there isn't.

Affected specs:

- `schemas/canonical/object-manifest-v1.md` — `requires`, `confers`,
  `coexists`.
- `schemas/canonical/resolved-manifest-v1.md` — `runtime` and
  `requires` sections of the plan.
- `OBJECT.md`, `BUILD.md`, `PLANNER.md` — runtime requirements and
  conferral.
- `CLI.md` §17 — plans show the runtime closure and chains.

## 47. Reads Trust the Storage; `kairo store verify` Finds Where It Failed

Decision: Kairo checks content against its id when the content enters
the store (a fetch, an import, a pull, a build's outputs) and not each
time it is read back. A read assumes the storage returns the bytes it
was given. Finding where storage didn't is a separate check,
`kairo store verify`, which re-hashes every record and blob in the
store against the id it is filed under. An operator runs it when the
disk is in doubt, or on a schedule; the better defence against the same
faults lives below Kairo, in a checksumming file system, scrubs, and
ECC memory, which the system's administrator manages.

Sub-decisions:

1. **Blob bytes are read as stored.** A file source's bytes, an
   artifact's files, logs, and manifests are returned by the store
   without being re-hashed. Their hash was checked when they were put
   (`put_file_source`, bundle import, a federation pull, an executor
   storing what a build made).

2. **Records still derive their ids on read.** A record (actor,
   object genesis, statement, source genesis) is parsed on every read,
   and its id is re-derived from the parsed body as part of decoding
   it. That costs next to nothing beside the parse and catches a
   record filed under the wrong name, which is a bug, not a storage
   fault. The rule in `crates/kairo-store/AGENTS.md` stands for
   records.

3. **`kairo store verify`.** Walks the store's actors, objects,
   statements, sources, file sources, execution records, and blobs,
   re-derives each one's id from its bytes, and lists every one that
   doesn't match its file name: `corrupt: <path>: <reason>`. It goes on
   past a mismatch and exits `10` (store error) if it found any, `0`
   otherwise. A blob's id is `sha256(domain || bytes)`, and nothing on
   disk records the domain, so each blob is tried against the domains
   blobs are stored under (`kairo_build::STORED_BLOB_DOMAINS`, the most
   common first). A blob that matches none is reported, so a new kind
   of blob must add its domain to that list. Derived state is not
   checked: indexes are rebuilt from the statements
   (`kairo store rebuild-indexes`), the Git cache is Git's to check
   (`git fsck`), and build layers are rebuilt from artifacts.

4. **Nothing is repaired.** Verify reports; it doesn't delete or
   re-fetch. A corrupt file comes back from a peer, a mirror, or a
   backup, once the storage under it is trusted again.

Rationale: re-hashing each blob on every read spends time on every
build to catch a fault the storage stack is better placed to prevent,
and a check on read can't catch a fault in the page cache after it,
while one in the copy can't be told from one on the disk. A check run
on request, over the whole store, says which files are bad wherever
the fault happened, and the operator who suspects the hardware decides
when to run it.

Affected specs:

- `CLI.md` §24 — `kairo store verify` is implemented.
- `GLOSSARY.md` "Fixity" — where fixity is checked.
- `crates/kairo-store/AGENTS.md` — the fixity rule covers records;
  blob bytes are read as stored.

---

## 48. Owners Rank Offers Before Versions; the Newest Published Breaks Ties

Decision: when more than one offer satisfies a requirement, the
resolver ranks them by their **owner** first: owners the node names,
then the node's own actors, then actors those trust, then everyone
else. It takes the highest version only among offers whose owners rank
alike. Among offers equal on owner and version, an artifact provider
comes before a content provider, as before; then the offer **published
most recently**; and only then the lower object id. A stranger's offer
is used only when no better-ranked offer satisfies the requirement.
Disambiguation is by owner and by version, never by the shape of an
offer: a child is not preferred over a whole object, nor the reverse.

Sub-decisions:

1. **An offer's owner is its object's creator.** The resolver reads
   only the creator's `head` branch and version tags (§33), so the
   creator is the actor vouching for what an object offers. A child's
   owner is its parent's creator. Owners rank in four tiers:
   - **preferred:** actors named by `kairo build --prefer <actor>`
     (repeatable, most preferred first), then by `[resolve] prefer` in
     `<store>/config.toml` (bare actor ids, in file order), each by its
     position in that combined list;
   - **local:** actors this node holds keys for;
   - **trusted:** actors a local actor trusts at `general` scope, the
     rule development mode already uses for signers (§33 sub-decision
     4). An unreadable trust record leaves the owner unranked;
   - **unranked:** everyone else.
   With no preference given, and no keys held, every owner is
   unranked, and the order is §43's.

2. **Owner before version.** A preference that a higher version from
   anyone overrides would not protect anything. With owner first, an
   object a stranger publishes as `tool:gcc 99.0` and a federation pull
   brings in can't take over every build that names `tool:gcc`. The
   cost is that a trusted owner's older version beats a stranger's
   newer one; the plan's diagnostics show the newer one and why it
   lost, and naming its owner, or pinning its object, takes it.

3. **The newest published, then object id.** Among offers equal on
   owner, version, and kind, the one whose `head` branch statement was
   signed most recently wins. Its `created_at` is the owner's own
   claim, and it only orders offers that owner and version can't tell
   apart: in practice, the same owner publishing the same label twice,
   where the later is what they mean now. Exactly equal offers fall to
   the lower object id. This supersedes §43 sub-decision 1's
   tie-break, "the index's order (artifact providers before content
   providers, then object id)".

4. **Diagnostics say why.** Each requirement's losing offers are listed
   in ranking order with their version, owner, and tier, and the first
   rank they fell behind on: `its owner ranks lower`, `a lower
   version`, `content, not an artifact`, `published earlier`, `after it
   by object id`. `kairo provider resolve` ranks as the resolver does,
   with the configured preference and local actors, and shows each
   candidate's owner and tier; `kairo search` shows each result's owner.

5. **Release candidates need nothing new.** Versions order by semver
   (§43), so `4.0.4-rc.1` sorts below `4.0.4`, and a range such as
   `^4` doesn't match a pre-release unless it names one. An early build
   tagged as a pre-release gives way as soon as the release is
   published.

6. **Plans pin the choice.** Owner preference is node policy, like the
   engine, and isn't in the plan hash; a plan records what was chosen,
   so a later change to `prefer` changes new plans, never existing
   ones.

Rationale: the duplicates that made ties matter came from one owner
publishing the same label twice, so the ordering among equals is what
decides them, and the most recent publication is what an owner means.
Disambiguating between owners belongs to the person running the
build, who knows whom they trust; versions, pre-releases included,
belong to the owner. A rule about an offer's shape (a child against a
whole artifact) speaks to neither.

Affected specs:

- `DECISIONS.md` §43 sub-decision 1 — its tie-break is superseded.
- `PLANNER.md` §4.2 — the ranking.
- `CLI.md` §17 — `kairo build --prefer`, the `alternatives` lines,
  `kairo provider resolve`, and `kairo search`'s `owner`.

---

## 49. live-bootstrap in Passes

Decision: Kairo's live-bootstrap runs in **passes**, each a native
object of its own that goes on from the previous pass's whole root
rather than one object whose manifest truncation moves further each
milestone. Pass 1 (`bootstrap/live-bootstrap/`, §38) runs to
`zlib-1.3.1`; pass 2 (`bootstrap/live-bootstrap-perl/`) runs
upstream's perl chain, to `perl-5.42.0` and the uninstall of perl
5.6.2 and 5.8.9; pass 3 (`bootstrap/live-bootstrap-gcc-4.7.4/`) runs
openssl, ca-certificates, curl, and patch to gcc 4.7.4, then binutils
2.41 and musl 1.2.5 rebuilt with it; pass 4
(`bootstrap/live-bootstrap-python/`) runs the python chain, guile, and
autogen; pass 5 (`bootstrap/live-bootstrap-gcc-10.5.0/`) runs gcc 10.5.0
and binutils 2.41 rebuilt with it; and pass 6
(`bootstrap/live-bootstrap-gcc-15.2.0/`), the last, runs gcc 15.2.0 and
the rest of upstream's manifest, whole. Building on a pass never
rebuilds the passes before it.

Sub-decisions:

1. **A pass's whole root is a label.** Each pass's parent provides
   `data:live-bootstrap:<arch>:<last step>`, its target's whole
   artifact: `data:live-bootstrap:x86:zlib-1.3.1` for pass 1,
   `data:live-bootstrap:x86:perl-5.42.0` for pass 2. The label names
   the step the root is after, so the next pass depends on exactly the
   state it continues from, and labels match exactly (§33). It is
   `data`: files the next pass's scripts read (§42). Where the last
   step is built more than once, the label names its package instead:
   pass 3's is `data:live-bootstrap:x86:musl-1.2.5_2`.

2. **What a pass ships.** Its artifact holds the installed `usr/` and,
   after the first pass, `etc/` (ca-certificates installs into
   `/etc/ssl`; the rest of `/etc` is `finalize_fhs`'s); every package
   so far in `repo/`; and in `state/` what the steps left
   outside the packages that the next pass needs: `/steps/env` and
   `/steps/bootstrap.cfg` as the improve steps edited them, and the
   `script-generator` the seed built and checked. Each package the pass
   made is extracted as a child (§41 sub-decision 6); a pass's children
   are its own packages only.

3. **How a pass goes on.** The artifact installs at `/`, so `/usr` is
   where live-bootstrap expects it and, with overlay provisioning,
   nothing is copied. The pass's start script links `/external/repo` to
   `/repo` (package revisions are counted there, and `uninstall` reads
   earlier packages' file lists from it), puts `state/` back in
   `/steps`, and remakes the rest of the root outside the packages as
   the earlier steps made it: `merged_usr`'s links from `/`, then
   upstream's own `populate_device_nodes` and `finalize_fhs`, which
   check before they make. Upstream's `script-generator` then writes
   the scripts for upstream's manifest truncated at this pass's end, so
   pass numbers and predicates come out as in one continuous run, and
   the start script runs the generated script from the step after the
   previous pass's last. Every pass after the first shares one start
   script and one packaging script
   (`bootstrap/live-bootstrap/pass/`); its target's command names the
   line the previous pass ended on. The generated script runs with
   upstream's environment, as `rootfs.py` runs it (`env -i` and
   `PATH=/usr/bin`), not the guest's: musl takes an unset `LANG` as
   `C.UTF-8` and the guest's `LANG=C` as ASCII, and python 3.1's codec
   generation in pass 4 can't write its tables in ASCII. *Found when
   pass 4 first failed; passes 2 and 3 had run with the guest's
   environment, and their packages matched upstream's anyway.*

4. **The checks are upstream's.** Every package a pass makes is
   checked against live-bootstrap's `SHA256SUMS.pkgs`, as in pass 1, so
   a continuation that diverged from a continuous run where it matters
   fails at the step that diverged.

5. **Distfiles with no stable upstream bytes.** The gnulib snapshots
   name no URL, and GitHub's archive tarballs aren't promised stable;
   upstream's `mirror.sh` makes both with `git archive`. Pass 2's
   import fetches each distfile from its upstream URL, then from
   live-bootstrap's maintainers' mirrors, then its archive.org mirror;
   the SHA-256 pins the bytes whichever serves them.

Rationale: a milestone a few hours past the last would otherwise mean
rebuilding everything before it on every attempt, and one object
holding the whole chain would grow without bound. Passes keep each
build to its own steps, give each stretch of the chain its own
children and labels, and use the same continuity upstream's own CI
relies on between its passes: the root, the packages, and the
generated scripts.

Affected specs: none beyond this entry; the wrappers' READMEs
describe the layout.

---

## 50. An Object That Doesn't Build Is Its Own Artifact

Decision: a content object, one whose `[content] kind = "tree"` and
whose provides entries name no build target (§33 sub-decision 2), is
its own artifact: its tree, or the part of it under `[content] root`,
with its files, executable bits, and links. It is installed like a
built artifact, at `[content] install`, when one of its labels is
required, as a dependency or in a runtime closure (§46). Data objects
and the objects a build's outputs become (children, §41) are then the
same shape: an object whose files are what it provides.

Sub-decisions:

1. **`root` and `install`.** `[content]` gains the two settings
   `[build.targets.artifact]` has (§41 sub-decision 3):
   ```toml
   [content]
   kind = "tree"
   root = "files"
   install = "/"
   ```
   `root` is a relative path in the tree, default `"."`. It is needed
   because the tree holds the object's own `kairo.toml`. `install` is an
   absolute guest path, or `false`.

2. **Not installed unless it says where.** Unlike a built artifact,
   whose install root defaults to `/`, a tree without `install` stays
   at `/kairo/content/<snapshot>`, as before: a tree holding its own
   manifest can't sensibly land at `/` by default, and every content
   object so far is read where it is, not installed. A content object
   without `root` or `install` is resolved, placed, and hashed as
   before; its manifest's canonical bytes and its plan entries are
   unchanged.

3. **The same install as an artifact.** In an engine's guest, the part
   under `root` is recorded as an artifact tree (its files stored as a
   build's outputs are, its links as links, under §39's rules) and
   installed with the other artifacts in requirement order, copied or
   overlaid, the first winning where two provide the same path. Placed
   at a path, or mounted at `/kairo/content/<snapshot>`, a content
   object is likewise the part under its `root`. Nothing runs: a
   content object needs no environment and no engine of its own.

4. **In plans.** A `content` resolution gains `install` and `root`, each
   present only when the object declares it
   (`schemas/canonical/resolved-manifest-v1.md`).

5. **The first such object: `linux-filesystem`.** `bootstrap/linux-filesystem/`
   holds the merged-/usr links, `bin → usr/bin`, `lib → usr/lib`, and
   `sbin → usr/sbin`, committed as links under `files/` and installed at
   `/`, and provides `data:linux-filesystem:merged-usr`. It depends on
   nothing. A package that needs the layout wherever it is installed
   requires the label; live-bootstrap's shared musl does (§51), since
   its loader is at `usr/lib/ld-musl-i386.so.1` and what is linked
   against it names `/lib/ld-musl-i386.so.1`. `/usr/sbin → bin` is left out: a package
   with files under `usr/sbin/` (live-bootstrap's util-linux) would
   collide with it.

Rationale: an object that doesn't build already has exactly what a
build would produce, its files; making those its artifact, installed
by the same rules, lets a layout, a dataset, or a configuration be a
small object that others require, without a build step that runs
nothing and depends on a tool only to copy files.

Affected specs:

- `OBJECT.md` §4.2 — `root` and `install`.
- `schemas/canonical/object-manifest-v1.md` — `ContentSection`.
- `schemas/canonical/resolved-manifest-v1.md` — `content` fields.
- `schemas/environment/linux-v1.md` — where content lands.

---

## 51. Conferring from What a Build Chooses, `one_of`, and musl in Parts

Decision: a runtime requirement is conferred on an artifact only by
what its own build chose, its **direct dependencies**: through the
provider's declaration of what using its label passes on, which a
label states about itself, or through the building object's own
`confer` flag on the dependency. Nothing passes on because a build
dependency needed it. A provider that needs its builder to choose
among alternatives says so with **`one_of`**, an obligation the builder
meets by requesting one itself. live-bootstrap's musl is split into the
parts linking needs of it, so a build chooses static or shared musl
and only the shared one is required later.

Sub-decisions:

1. **Only direct dependencies confer.** What a build's dependencies
   require in its build environment (§46 sub-decision 7) is theirs, not
   the artifact's; a build dependency of a build dependency confers
   nothing on it. The conferral is computed from the offer the build
   resolved for its own dependency on the label, so it describes what
   the build used.

2. **A label confers itself; another label carries a range.** A
   provides entry's `confers` naming its own label without a version is
   pinned to the offer the build resolved; with a version, it is that
   range: `lib:musl:shared` confers `lib:musl:shared` `^1.2.5`, since a
   later musl 1.x runs what 1.2.5 linked. The range is the provider's to
   give, by its own compatibility: a caret for musl, `>=` for
   libstdc++, whose ABI has been the same since gcc 3.4. A `confers`
   entry naming another label must give a version; a pin to some
   sibling could name a different offer than the one the build
   resolved, which is how a gcc conferring its own musl came to require
   a musl the build hadn't linked against. Amends §46 sub-decision 2.

3. **`confer` on a dependency.** A `kind = "provides"` dependency may
   say `confer = true` or `confer = false`. `false` passes on nothing
   from it, for a dependency used only while building. `true` passes
   on its label even when its provider doesn't: at the provider's own
   conferral of the label if it gives one, else the dependency's
   version requirement, else pinned to the offer resolved. Without the
   flag, the provider's declaration stands. Only a provides dependency
   takes it.

4. **`one_of`.** A `requires` entry may be
   `{ one_of = [{ provides, version? }, ...] }`, at least two
   alternatives. It isn't installed beside the artifact; it obliges any
   build that uses the artifact, as a dependency or in its runtime
   closure, to request one of the alternatives itself, among its own
   dependencies, satisfying the alternative's version. A build that
   requests none is refused at plan time, naming the chain and the
   alternatives. Plans record the obligation in `requires` as a
   `one_of` entry.

5. **musl in parts.** Each live-bootstrap musl package is laid out as
   the children its uses need (`bootstrap/live-bootstrap/split-musl.sh`):
   `<package>-headers` (`lib:musl:headers`: the headers, startup files,
   and empty stub archives, needed to link either way),
   `<package>-static` (`lib:musl:static`: `libc.a`), and, for the one
   package built with a shared libc, `musl-1.2.5_2`, `<package>-shared`
   (`lib:musl:shared`: `libc.so`, the loader, `ldd`, and
   `etc/ld-musl-i386.path`; conferring itself `^1.2.5` and requiring
   `data:linux-filesystem:merged-usr`, §50). The packages are still
   checked against upstream's checksums; the children are their files,
   laid out by use.

6. **gcc requires a choice and confers nothing.** live-bootstrap's gcc
   children require binutils, `lib:musl:headers`, `one_of`
   `lib:musl:static` or `lib:musl:shared`, and the kernel headers, and
   confer nothing: whether what is built needs a libc at run time
   depends on which the builder chose. gcc 4.7.4's libstdc++ is static
   only, so its label is `lib:stdc++:static`, conferring nothing. A gcc
   is built for one C library; another libc (glibc) is another gcc,
   requiring its own libc's parts, as different targets are different
   gccs.

Rationale: the object being built knows how it links; a compiler
doesn't. Making the builder choose, and conferring only from what it
chose, keeps an artifact's runtime requirements to what it really
needs, at the compatibility its libraries promise, and says why: the
builder asked for it.

Affected specs:

- `DECISIONS.md` §46 sub-decision 2 — conferral without a version.
- `schemas/canonical/object-manifest-v1.md` — `Requirement`, `confer`.
- `schemas/canonical/resolved-manifest-v1.md` — `one_of` entries.

---

## 52. Artifacts Found by What a Build Consumes

Decision: a plan carries a **build key**, the hash of exactly what its
build's guest sees and runs, and a held build whose plan has the same
key is this plan's artifact, at whatever snapshot it was built. Edits
to what the key leaves out (an object's summary, tags, versions, its
children's declarations, what its artifact requires or confers) no
longer rebuild it. For the key to be a proof, what it leaves out is
kept from the guest: `/kairo/src` doesn't hold `kairo.toml`, and
`SOURCE_DATE_EPOCH` is no longer the revision's time.

Sub-decisions:

1. **What the key is.** A `BlobId` under `kairo.build-key.v1` over: the
   target's command, workdir, artifact root, and declared outputs; the
   environment contract and `host_dependent`; each requirement as
   requested (label, version requirement, placement) and what it
   resolved to by content (an artifact as the build key of the plan
   that makes it, with the output, install root, and child root used;
   a source's commit; a file's SHA-256; a host tool's path; a content
   object by what is under its `[content] root`, with its install root);
   the runtime closure; the target's tree (sub-decision 7); and
   `SOURCE_DATE_EPOCH`. The object and snapshot ids are not in it, nor
   anything only read after the build.

2. **The guest doesn't see `kairo.toml`.** The executor reads the
   target from it and removes it before the command runs. A build that
   needs a value its manifest holds declares it as an input another
   way, such as a file in its tree.

3. **`SOURCE_DATE_EPOCH` is an input.** It is the target's
   `source_date_epoch`, or 315532800 (1980-01-01T00:00:00Z, the earliest
   time a ZIP archive holds) when the target names none. It was the
   revision's `created_at`, which made every revision a new input. What
   a build embeds from it is now fixed until the target says otherwise;
   when things happened is still in the signed revision and result. The
   `linux` contract is amended in place: nothing built under it is held
   anywhere that must keep its old meaning, and every plan's key is new
   anyway.

4. **Lookup.** A dependency's artifact is found first, as before, by a
   build of the very snapshot (*amended by §54:* if that build's plan
   has the key the provider plans to); else the provider is planned, which
   gives its key, and a held, accepted, successful build of the same
   object's target whose plan has that key is used; else it is built
   first. A provider held but not plannable here (its tree or sources
   not held) is used as held, and its artifact id stands in for its key
   in a consumer's key. `kairo build` on a target whose key is held
   runs nothing and says `status = held`, naming the result it found.

5. **Nothing new is signed.** A `BuildResult` says that its plan's
   inputs produced its artifact; that stays true for every snapshot
   whose plan has the same key, so reuse needs no new statement, and no
   record says a build ran after the update it serves. The new
   snapshot's plan is stored as usual; its `requires` and children are
   read from it, and its children are slices of the held artifact.
   Signing a result for the new snapshot that cites the earlier
   execution, marked as reuse, is left for later.

6. **Plans change once.** Every plan gains `build_key` and
   `source_date_epoch`, so every plan hash is new, and every held build
   is built once more under the new rules.

7. **What of a tree is normative is a root.** A build target may name
   `tree = "<path>"`, the part of its object's tree that is
   `/kairo/src`; its workdir, outputs, and relative placements are then
   within that part. Without it, `/kairo/src` is the whole tree but
   `kairo.toml`. A content object's normative part is its
   `[content] root` (§50), the part that is installed. A build key
   covers each by the hash of that tree's entries (mode, name, and Git
   object id, which names everything under an entry), with `kairo.toml`
   left out at the top of a whole tree; so an edit beside the root, to a
   README or documentation, changes no key, and the guest can't see it
   either. A root says what is normative in one path; an exclude list
   would say what isn't, and a file added later would reach a build
   unless someone remembered to list it. Plans record a content
   resolution's `tree`. *Added after §52 was first implemented:* a
   content object was keyed by its snapshot, and a build by its whole
   tree but `kairo.toml`.

Rationale: identical inputs make the same build; whether they make the
same bytes is a separate question, about determinism, which `kairo
reproduce` answers. Keying artifacts by inputs and keeping everything
else out of the guest's reach turns a metadata edit from a rebuild into
a lookup, without any statement claiming more than it did.

Affected specs:

- `schemas/canonical/resolved-manifest-v1.md` — `build_key`,
  `source_date_epoch`, an artifact's `build_key`.
- `schemas/canonical/object-manifest-v1.md` — `source_date_epoch`,
  `tree`.
- `schemas/environment/linux-v1.md` — `/kairo/src` and
  `SOURCE_DATE_EPOCH`.
- `CLI.md` §17 — `status = held`.

---

## 53. What an Object Runs In, What It Is Built In, and Target Triples

Decision: an object's top-level environment dependency says what its
artifacts **run in**; a build target may name its own environment
dependency, which says what its build **runs in** instead. A 64-bit
program built by a 32-bit environment declares
`environment:linux/x86-64` at the top and `environment:linux/x86` on its
target. Every tool that produces code for a target, a compiler or
binutils, carries that **target triple** in a label of its own; only a
native tool, whose target is the architecture it runs on, offers the
plain label too. This is what cross-compiling needs, from the x86
bootstrap to x86-64 first.

Sub-decisions:

1. **Two environments, one by default.** A target's build environment
   is its own environment dependency, or else its object's; its
   artifacts' environment is its object's, or else the target's. With
   one environment declared anywhere, as every object before this one,
   both are it, and nothing changes. A target that names its own
   environment doesn't resolve its object's as a build requirement:
   the plan's `environment` is the build's, and the engine is chosen
   by it.

2. **Offers by where they run, builds by where they run.** An artifact
   offer is matched to a consumer by its artifacts' environment against
   the consumer's build environment (§38 sub-decision 8): a 64-bit
   program is offered to builds in a 64-bit environment, where it can
   run, whatever built it. So a cross build's dependencies are tools
   that run where it is built; the target's libraries come with its
   cross toolchain, as its sysroot, not by label.

3. **Target triples.** A compiler or binutils for `<arch>-<os>-<abi>`
   provides `tool:gcc:<triple>`, `tool:g++:<triple>`, and
   `tool:binutils:<triple>`, the triple spelled without its vendor
   field (`i686-linux-musl`, `x86_64-linux-musl`), one spelling since
   labels match exactly. A native one also provides the plain
   `tool:gcc`, `tool:g++`, `tool:binutils`, "the compiler for the
   machine this build runs on", so a consumer that names `tool:gcc`
   never gets a cross compiler. A cross compiler and a native one for
   the same target share the triple label and are told apart by where
   they run: in a 32-bit build, `tool:gcc:x86_64-linux-musl` is the
   cross compiler; in a 64-bit one, the native. A gcc requires the
   binutils for its own target by triple. Tools that make no code
   (bash, make) have no target, only the environment they run in.

4. **live-bootstrap's tools.** Every gcc and binutils child of the
   passes now also provides its `i686-linux-musl` label, and gcc
   requires `tool:binutils:i686-linux-musl`. A change to children
   alone, it keeps every pass's build key (§52).

Rationale: what a program runs in is a fact about the program, and
where it was built is a fact about one way of building it; keeping both,
with the first by default, lets a cross build say exactly what it is.
Naming every compiler's target, not only a cross compiler's, makes the
label say what the tool produces, and leaves where it runs to the
environment, which the resolver already matches.

Affected specs:

- `DECISIONS.md` §38 sub-decision 8 — whose environment is matched.
- `OBJECT.md` — environments of objects and targets.

---

## 54. Artifacts Keep What a Build Made: Modes, Directories, and Hard Links

Decision: an artifact keeps every file's **permission bits**, its
**directories** where they say something, and its **hard links**, not
only whether each file is executable. An artifact is Kairo's own blob
tree (§33 sub-decision 6), not a Git tree, and nothing about it needs
Git's three modes; recording only the executable bit dropped what
software installs on purpose: perl's read-only modules, setuid
programs (shadow's `passwd`, `doas`), `/tmp`'s sticky bit, `/root`'s
`0700`, empty directories a program expects, and the links between
files that are one file. The new fields are present only where they
differ from what a tree without them means, so a tree that has none of
them encodes as before.

Sub-decisions:

1. **`mode` on a file.** A file entry gains `mode`, its permission
   bits as four octal digits (`"0444"`, `"4755"`): read, write, and
   execute for owner, group, and others, and setuid, setgid, and
   sticky. It is present only when the bits are neither `0644` nor
   `0755`; without it a file is `0755` if `executable`, else `0644`, as
   before. `executable` stays, and says whether any execute bit is set;
   a tree whose `mode` and `executable` disagree is malformed.

2. **Directory entries.** An entry may be a directory:
   `{ "path", "directory": true, "mode"? }`, sorted with the rest by
   `path`. It is present only for a directory that is empty or whose
   bits aren't `0755`; every other directory is implied by the paths
   under it, as before, and is `0755`. A directory entry's path is no
   other entry's, and like any entry it lies under no link.

3. **Hard links.** A file that is another file of the same artifact,
   the same inode when the build ended, is `{ "path", "hardlink":
   "<path>" }`, naming the first of them by path; that one is a file
   entry with its blob and mode, and the others share them. A
   `hardlink` names a file entry earlier in the tree, never a link, a
   directory, or another `hardlink`. A child's slice (§41 sub-decision
   4) whose file is linked to one outside the slice holds it as a file
   entry with that file's blob and mode, since the slice stands alone.

4. **Not kept: owners, times, and special files.** The guest is root
   in its own namespace, and the bubblewrap engine maps that one id;
   every file a build makes is root's, and a build that tries another
   owner fails (live-bootstrap's tarballs had to be unpacked with
   `--no-same-owner`). Owners are left out until an engine maps more
   ids, which is when a decision can say what they mean. Times are left
   out because a build's inputs fix them (`SOURCE_DATE_EPOCH`, §52) and
   anything else would make an artifact depend on when it was built.
   A device, FIFO, or socket still fails collection.

5. **The command's umask is `022`.** Whoever runs Kairo, and whatever
   umask they run it with, a build's command starts with `022`, in
   every engine; under the executable-bit rule a umask couldn't change
   an artifact, since it never adds execute bits, but under these rules
   it would. The `linux` contract says so.

6. **A Git tree keeps what Git does.** An object that doesn't build is
   its own artifact (§50), collected from its tree as Kairo exports it
   on the host, where the host's umask decides every bit but execute and
   Git holds no directories. Such an artifact keeps only the executable
   bit, as before, and no directory or hard link entries; its id
   doesn't depend on who exported it.

7. **Applied in guests; setuid and setgid nowhere else.** Materializing
   an artifact for a guest, by copy into its root or as a cached layer,
   sets each file's and directory's bits, but never setuid or setgid:
   the guest is root, so neither means anything inside it, and on the
   host, under a store any local user can read, a setuid file owned by
   the store's owner would run as that owner for anyone. Every other
   bit is applied, a directory's once everything is placed in it, so a
   read-only one keeps no later dependency out; where dependencies
   share a directory, the first's bits stand, as its files do. Nothing
   writes an artifact out for use as a system root yet, which is where
   the two bits mean something; what does will apply them only when
   asked. Kairo makes a directory writable to its owner before removing
   it, so a read-only directory, an artifact's or one a guest left in
   its workspace, never strands a tree.

8. **Builds are collected again once.** A build key gains `collect`,
   the version of the rules its outputs are collected by: `2` from this
   decision; a key without it was collected by rules `1`. A held build
   that predates it lacks what these rules record, so its key no longer
   matches and it is built once more, as §52 sub-decision 6 had every
   build built once more; that is the price of a key that determines an
   artifact's whole content rather than most of it. Artifacts are still
   cited by id, so a consumer's key, which names a provider by its key,
   changes only with it. For that to reach dependencies, a dependency's
   build of the very snapshot, which §52 sub-decision 4 looks for
   first, is its artifact only if the plan it cites has the key the
   provider plans to now; one whose plan has another key is passed
   over, and the provider is found by key or built first. A build whose
   plan isn't held can't be told apart, and stands, as does any build of
   a provider not plannable here.

9. **Comparing two roots.** Whether one build reproduces another, as
   the 64-bit bootstrap's fixed point asks (`bootstrap/amd64/`), is a
   comparison of artifact trees, which now include modes, directories,
   and hard links; a checker inside a guest sees the installed root
   with setuid and setgid stripped (sub-decision 7) and must compare
   those bits from the tree, not from the files.

Rationale: an artifact is what a build made, and a build makes files
with modes, directories, and links between them; recording all of
what can't vary between two runs of the same build, and none of what
can, keeps an artifact's id a statement about its content while
dropping nothing software depends on. Present-only-when-unusual fields
keep every artifact that has nothing unusual exactly as it was.

Affected specs:

- `schemas/canonical/artifact-tree-v1.md` — `mode`, directory entries,
  `hardlink`, and the rules for each.
- `schemas/canonical/resolved-manifest-v1.md` — the build key's
  `collect`.
- `BUILD.md` §6 — what an artifact keeps.
- `EXECUTOR.md` §14 — collection and materialization.
- `schemas/environment/linux-v1.md` — the umask.

---

## 55. Packages Apart from the Bootstraps, a Target per Environment

Decision: software Kairo keeps as artifacts for builds to choose from,
apart from what a bootstrap needs to reach its next stage, is a
**package**: an object under `packages/`, one per release, built on a
finished root. A package built for more than one environment has **a
target for each and no environment of its own**, so each target's
artifacts run where they were built (§53 sub-decision 1). The first is
gcc 7.5.0, for x86 and x86-64, the start of the gcc versions a build
can choose its compiler from.

Sub-decisions:

1. **Where packages live.** `packages/<name>-<version>/` holds a
   package's manifest, its recipe in `src/`, and an `import.sh` that
   archives its sources and creates its object with
   `bootstrap/wrapper.sh`, as the bootstraps' stages do. A bootstrap
   stage builds what the next stage needs, in the order it needs it;
   a package is built from a root that is already finished, and nothing
   in a bootstrap depends on one.

2. **One object per release.** Each release is its own object
   (`gcc-7.5.0`), not a version tag on one object's history: two
   releases are offered side by side, chosen by the version a consumer
   asks for (§43), and each release's recipe differs from the next's in
   ways a history of one object would only hide. A fix to a release's
   recipe is a revision of its object. *Replaced by §59:* one object
   per software, its releases version tags on its history, each built
   with older ones.

3. **A target per environment.** A package built for x86 and x86-64
   names `environment:linux/x86` on one target and
   `environment:linux/x86-64` on the other, and none at the top. Both
   targets offer the same labels by `[[provides]]` entries naming their
   target; an offer is matched to a consumer by its target's
   environment (§53 sub-decision 2), so a build in x86 gets the x86 one.
   Each target builds on its environment's root as a target
   dependency, and says what it runs with in its own `requires` (§46).

4. **Beside another of its kind.** A package that the root it is used
   with also has, as a gcc is, keeps every file but its programs in a
   directory named by its version, so that in an overlay of both (§41)
   only the program names are shared, and the first-listed dependency's
   programs are the ones that run. Its recipe checks it. A gcc's runtime
   libraries go in its versioned directory
   (`--enable-version-specific-runtime-libs`), never over the root's.
   For a gcc older than the root's, the root's shared `libstdc++` and
   `libgcc_s` are newer and keep their ABI, so the package offers
   `lib:stdc++:static` and not the shared libraries, which a consumer
   takes from the root; a gcc newer than the root's will have to offer
   its own. *Amended by §56:* every gcc offers its shared runtime
   libraries, as children of their own in `/usr/lib`; only the
   compiler keeps to its versioned directory.

5. **A compiler bootstraps itself.** A packaged gcc bootstraps, its
   later stages built by its earlier ones and compared, so what it is
   doesn't depend on which gcc the root had. Its first stage is built
   as the language standards its sources were written to.

6. **The 64-bit root as a whole, for now.** The 64-bit bootstrap's root
   offers only itself, `data:amd64-bootstrap:x86-64:root`, not labels
   for binutils, musl, or the kernel's headers as live-bootstrap's
   children do. A package's x86-64 target requires that root whole
   until it does; its x86 target requires the parts by label.
   *Replaced by §56:* the root's packages are children, and the label
   for the whole is gone.

Rationale: a bootstrap's stages are ordered by what each needs to make
the next, and a package by nothing but its own release, so they are
kept apart; that keeps the bootstrap small and lets packages be added,
in any number of versions, without touching it. A target per
environment keeps one release's recipe in one place for every
architecture, as §53 already lets an object say per target where its
artifacts run.

Affected specs:

- `PROJECT_LAYOUT.md` — `packages/`.

---

## 56. No Whole Roots: Every Package a Child, Runtime Libraries Apart

Decision: nothing downstream of a bootstrap depends on a **whole
root**. A bootstrap stage that builds many packages in one build ships
each as a **child** (§41 sub-decision 4), as live-bootstrap's passes
do, and its own artifact, the children side by side, is installed
nowhere (`install = false`). A build that needs a shell, make, a
compiler, and a libc asks for each by label, and its environment is
the overlay of those packages and what they require. A gcc's shared
**runtime libraries** are children apart from its compiler, each
offered with its own version and conferring itself as a range, so a
program runs with the newest any gcc in its environment offers.

Sub-decisions:

1. **The 64-bit root as children.** The 64-bit bootstrap's stage 3
   (`bootstrap/amd64/native-root/`) ships every package into a
   directory of its own under `/kairo/package`, each a child installed
   at `/`: the kernel's headers, binutils, gcc, each of gcc's shared
   runtime libraries, musl as its headers, static, and shared parts
   (§51), the userland, zlib, libffi, perl, and python. The label for
   the whole root, `data:amd64-bootstrap:x86-64:root`, is withdrawn
   (§39 sub-decision 5: nothing built with it was published). The
   build fails if two children would hold one path, since a consumer's
   overlay would show only one of them.

2. **Each child says what it runs with.** A child's `requires` (§46)
   names what its programs and libraries load: the shared musl, for
   every dynamically linked package, which brings the merged-/usr links
   its loader is reached through; `libgcc_s` for `libstdc++`; and for
   python, the libraries its modules load. The recipe reports each
   child's `NEEDED` sonames, from which its requires are written. A
   change to children alone keeps the build key (§52), so a missed
   requirement is fixed without a rebuild.

3. **Fixed points over children.** Stage 4 builds on stage 3's
   children, each by its first label pinned to its version, and checks
   that it makes them again, together; its artifact is stage 3's, the
   same id.

4. **The compiler and its runtime libraries.** A gcc installs its
   runtime libraries and C++ headers in its versioned directory
   (`--enable-version-specific-runtime-libs`), where its driver finds
   them when it links, so a program is linked against the gcc that
   builds it. Each shared runtime library (`libstdc++`, `libgcc_s`,
   `libgomp`, `libatomic`, `libitm`, `libquadmath`) is also copied, its
   file and its soname's link, into a child of its own in `/usr/lib`,
   where the loader finds it. Each offers `lib:<name>:shared` at the
   gcc's version and confers itself as `>=` that version: these
   libraries keep one soname and their ABI across releases, so a later
   one runs what an earlier one linked, and one copy, the newest a
   consumer's confers allow, serves every program in an environment.
   Libraries whose sonames change between releases (`libgfortran`,
   `libasan`) don't satisfy one another, and will be offered as
   coexisting (§46). libcc1, which would go in `/usr/lib` under no
   version, isn't built.

5. **The compiler's requirements.** A gcc child requires binutils for
   its target, musl's headers, the kernel's headers, `one_of` musl's
   static or shared libc, chosen by what builds with it (§51), and the
   shared musl its own programs load.

6. **Packages build by label.** A package under `packages/` (§55)
   builds from the packages it names, not from a root: a shell and the
   GNU userland, binutils, a compiler and the runtime libraries its
   programs load, a libc, and headers, each by label, from
   live-bootstrap's children in x86 and the 64-bit bootstrap's in
   x86-64.

7. **What comes next.** The base packages become objects of their own
   under `packages/`, built first from the bootstraps' children and
   then from one another, so that the archive's packages depend on the
   bootstraps only as history. A **build environment** that many
   objects share will be an object holding only a list of
   requirements, which a consumer depends on instead of naming each
   package; and since an environment of many small packages is a deep
   overlay, **merged layers** will be cached rather than stacked
   (§41 sub-decision 5 notes the limit). Both are decisions of their
   own.

Rationale: an aggregate root ties every consumer to every package in
it, so no one package can be replaced or chosen by version; children
cost the bootstrap nothing, since they share its one build, and let
every consumer take exactly what it needs. Runtime libraries apart
from their compiler let one environment hold several compilers and
one copy of each library, the newest, which is what the libraries'
compatibility promises.

Affected specs:

- `DECISIONS.md` §55 sub-decisions 4 and 6.

---

## 57. An Object That Doesn't Build Brings Its Dependencies

Decision: an object that doesn't build is its own artifact (§50), and
its **dependencies are the dependencies of any build that depends on
it**. A build that depends on such an object, by label or by id, takes
that object's `[[dependencies]]` as its own requirements, as if its
manifest named each. So a **build environment** many objects share is
an object holding only a list of dependencies, as Occam had it, and a
build names that one object instead of each tool.

Sub-decisions:

1. **Taken as the build's own.** Each brought dependency is a
   requirement of the build, after the object that brought it, in that
   object's order. It is resolved, chosen, and conferred as the build's
   own are: its version requirement constrains the label, it meets a
   `one_of` obligation (§51), and a label it brings confers what that
   label's provider confers (§46). It is not a runtime requirement of
   the object that brought it: an object that doesn't build holds no
   program that needs them; the build does.

2. **The build's own labels win.** A label the build names itself, at
   any position, replaces every brought requirement on it. A build that
   wants gcc 7 names `tool:gcc` with `^7` and still takes the rest of
   its environment from the object. Between brought requirements
   nothing is replaced: two objects that bring one label both
   constrain it, as two of a build's own dependencies on one label do.

3. **Nested.** A brought dependency that is itself an object that
   doesn't build brings its own, after it. An object brought twice
   brings its dependencies once. Nesting stops at 16 levels.

4. **What can be brought.** Labels and objects. An environment label
   would change where the build runs, and a file or source where its
   tree is, which only the build's own manifest decides; an object that
   doesn't build and depends on one is refused when a build chooses it.
   A build environment therefore names no environment, and each of its
   labels resolves to the package for the environment the build runs in
   (§53): one object serves builds in x86 and in x86-64. It names
   native tools by their plain labels (`tool:binutils`), not by a
   triple.

5. **In plans.** A brought requirement records `brought_by`, the
   `object:<id>` that brought it (`resolved-manifest-v1`), present only
   then, so a plan with nothing brought is the same bytes as before.
   The build key reads only what was requested and resolved (§52), so
   naming the tools directly or through an object that brings the same
   ones differs only by the object's own requirement. `kairo build`
   shows `(brought by object:<id>)`.

6. **The first: `build-base`.** `packages/build-base/` brings what a
   typical C or C++ package builds with: a shell and the GNU userland,
   make, the archivers and compressors, m4, the native binutils and gcc
   with C++ and their shared runtime libraries, musl's headers and
   shared libc, and the kernel's headers, as `data:build-base` 1.0.0.
   gcc 7.5.0 builds with it in x86 and x86-64, naming beyond it the gcc
   15 its first stage is built by and static musl. *Amended by §61:*
   only releases satisfy its labels, so it serves builds once every
   package it names is a release; gcc 7.5.0, a first generation, builds
   with `bootstrap-base`.

Rationale: a list of what builds need is information about builds, so
it belongs in an object others depend on, versioned and signed like
any other, not copied into every manifest; and since an object that
doesn't build has no build of its own to need its dependencies, they
can only mean what its consumers need. Taking them as the build's own,
rather than as what an artifact requires at run time, keeps the build
in charge: it chooses, it confers, and it can replace any one of them.

Affected specs:

- `BUILD.md` §3 — what a target's requirements are.
- `schemas/canonical/resolved-manifest-v1.md` — `brought_by`.
- `PROJECT_LAYOUT.md` — `packages/build-base/`.

---

## 58. live-bootstrap's gcc in Parts, and What Its Children Load

Decision: live-bootstrap's gcc packages with shared runtime libraries
are split at packaging time, as its musl is (§51), into the compiler
and a child for each library, the shape §56 gave the 64-bit
bootstrap's gcc. And live-bootstrap's children say what their programs
and libraries load, from a report the packaging writes, as §56
sub-decision 2 has the 64-bit bootstrap's children do. The split is
made by the build, not by declaring a filtered child: a child stays
what the build staged under one directory (§41 sub-decision 4).

Sub-decisions:

1. **`split-gcc.sh`.** Every later pass's `package.sh` sources
   `bootstrap/live-bootstrap/split-gcc.sh`, as it does `split-musl.sh`,
   and runs it on each gcc package the pass made. For each runtime
   library, `libstdc++`, `libgcc_s`, `libgomp`, `libatomic`, `libitm`,
   and `libquadmath`, the shared file and its soname's link move into
   `<package>-lib<name>/`, laid out as in the package. What links
   against them stays with the compiler: the unversioned names
   (`libstdc++.so`, and `libgcc_s.so`, a linker script), the static
   archives, the headers, and the debugger's scripts. A gcc built
   without shared libraries, as live-bootstrap's 4.x are, is left as it
   is; one with some must have all six.

2. **Its children.** gcc 10.5.0 (pass 5) and 15.2.0 (pass 6) are each a
   compiler child providing `tool:gcc`, `tool:g++`, their
   `i686-linux-musl` labels, and `lib:stdc++:static`, and six runtime
   children providing `lib:stdc++:shared`, `lib:gcc:shared`,
   `lib:gomp:shared`, `lib:atomic:shared`, `lib:itm:shared`, and
   `lib:quadmath:shared`, each conferring itself `>=` its version. The
   compiler child no longer offers the shared labels.

3. **What each child loads.** After the splits, `package.sh` reports,
   for each child, the sonames its ELF files load (`NEEDED`) and those
   its libraries are (`SONAME`). `children.py`, given the report, has
   each child require the label of what it loads and doesn't hold, by a
   table of sonames, and stops on a soname it has no label for rather
   than guess. So pass 6's m4 requires `lib:atomic:shared`, which it
   loads, and pass 6's gcc the shared musl, `libstdc++`, and `libgcc_s`
   its programs load; pass 4's dynamically linked programs require the
   shared musl. Before, they got these only when the whole gcc child
   happened to be installed.

4. **What rebuilds.** Passes 2 to 6 share `pass/package.sh`, so all five
   rebuild, and with pass 6's artifact everything built on it: the
   64-bit bootstrap's four stages and gcc 7.5.0's x86 target. Pass 1
   keeps its own `package.sh` and its build: its packages, before
   live-bootstrap's first shared musl, load nothing.

Rationale: a child that is a filter over a finished artifact would give
children a second definition, matched against a tree rather than made
by a build, to save rebuilding; the archive is better served by one
rule, kept everywhere, and the bootstrap's outputs made as its later
packages are. Requirements read from what a child actually loads,
rather than written by hand, are complete by construction, and stop
the generation when the table is.

Affected specs:

- `DECISIONS.md` §51 (musl's split, the precedent), §56.

---

## 59. One Object per Software, Generations, and Offers by Whom

Decision: a package is **one object per piece of software**, its
releases **version tags** on its history, as Occam kept gcc. Every
tagged snapshot **offers its labels** beside the head, so older
releases stay available by label. A build may use its own object's
offers only from an **older generation**: a version-tagged snapshot
whose revision is an ancestor of the one being built. And a
requirement can say **whose offers** may satisfy it (`offered_by`), so
a first generation names the bootstrap it is built from. This replaces
§55 sub-decision 2.

Sub-decisions:

1. **One object per software.** `packages/<name>/` is one object: its
   manifest and recipe are its latest release's, and each release is a
   revision of it, tagged with the release's version (§43). An older
   release's recipe is its tagged revision, in the object's history; a
   fix to it is a revision on a branch from its tag. gcc 7.5.0 is the
   `gcc` object at tag 7.5.0, and a later gcc a later revision on it.

2. **Tagged snapshots offer their labels.** The resolver offers, for
   each held object, the labels of its head and of each of its
   creator's version-tagged snapshots whose tree the node holds. Ties
   are broken as before (§48); the highest version still wins by
   default (§43), so an older release is chosen only when a
   requirement asks for it. Search and `kairo provider show` still
   describe the head.

3. **Generations.** A generation is a tagged snapshot. A target's build
   may use an offer of its own object, or of a child of it, only when
   the offer is a generation older than the build, a version-tagged
   snapshot whose revision is an ancestor of the revision being built,
   or another target's of the same snapshot, which is a build of its
   own, as the 64-bit bootstrap's stage 4 builds on stage 3's children.
   Its own target's offers, and a newer or unrelated tag's, are never
   candidates, as Occam never built an artifact with one of its own
   generation. So the first release of an object builds with what
   others offer, and each later one may build with an older release of
   itself, as gcc 8 with gcc 7. A resolution keys the builds it plans by
   snapshot as well as object and target, so planning an older
   generation for a newer one is no cycle.

4. **`offered_by`.** A provides dependency may name the objects whose
   offers alone satisfy it:

   ```toml
   [[dependencies]]
   kind = "provides"
   provides = "tool:make"
   offered_by = ["zQm...", "zQm..."]
   ```

   An offer qualifies when its object, or for a child its parent, is
   named. The ids are bare (§4), at least one; only a provides
   dependency takes it. It is part of the manifest's canonical bytes
   and of the plan's requested requirement, and so of the build key,
   only when given.

5. **The first generation names its bootstrap.** Packages offer the
   bootstrap's labels at its versions, and win the tie as the newest
   published (§48); building one package with another of the same
   round would be a cycle, and breaking cycles by whichever was asked
   for first would make a plan depend on its caller. So the first
   generation of a package `build-base` names (§63 for the rest)
   builds with `packages/bootstrap-base/`
   (`data:bootstrap-base`), which brings what `build-base` does (§57),
   each label `offered_by` the bootstrap's objects: live-bootstrap's
   six passes and the 64-bit bootstrap's root, by their ids. Object ids
   are bound to their genesis and permanent (§2); the objects in the
   archive's canonical store are these, and other stores get them by
   bundle or federation, keeping their ids. Later releases build with
   `build-base`, whose labels resolve to the packages, and to older
   generations of themselves.

6. **What a build installs already meets what it brings.** A label the
   runtime closure asks for (§46) is met by an object the build already
   installs, a choice for its declared requirements or the closure's,
   when that object offers the label at a version every constraint
   allows; only otherwise is the label chosen anew. So the bootstrap's
   gcc, requiring `tool:binutils:x86_64-linux-musl`, is served by the
   binutils child the build took for `tool:binutils` under
   `offered_by`, not by a package of the same version that would win the
   tie, and no second package is installed where one serves.

Rationale: a piece of software's releases are one lineage, which an
object's history and tags already are; one object per release would
duplicate that lineage across objects and hide that 8.5.0 follows
7.5.0. Barring only the build's own and newer generations, rather than
the whole object, is what lets software that builds itself do so, and
naming the bootstrap by the ids it already has is what lets a first
generation exist at all without a cycle.

Affected specs:

- `DECISIONS.md` §55 sub-decision 2.
- `schemas/canonical/object-manifest-v1.md` — `offered_by`.
- `schemas/canonical/resolved-manifest-v1.md` — the requested
  requirement's `offered_by`.
- `BUILD.md` §3 — generations and `offered_by`.
- `PROJECT_LAYOUT.md` — `packages/<name>/`.

---

## 60. Libraries in a Directory Named by the Target

Decision: in each Linux environment, every library built for musl is
installed in **a directory named by the target**, not in `/usr/lib`, and
musl's loader searches that directory alone. In `linux/x86` that is the
directory live-bootstrap already uses, `/usr/lib/i686-unknown-linux-musl`;
in `linux/x86-64`, `/usr/lib/x86_64-linux-musl`. Libraries built for
another C library, as glibc's would be, have their own directory beside
it, so the two can be installed in one environment: they share sonames
and not ABIs, and each loader finds only its own. `schemas/environment/linux-v1.md`
records each environment's directory as a convention packages follow.

Sub-decisions:

1. **Why not `/usr/lib`.** A program built against musl can't load a
   library built against glibc, nor the reverse, yet both have a
   `libstdc++.so.6`. In one `/usr/lib` they would collide on the path,
   and glibc's loader searches `/usr/lib` by default. A directory per C
   library keeps them apart, as Debian's multiarch keeps architectures
   apart; musl's loader reads `/etc/ld-musl-<arch>.path` and, when it
   exists, searches only what it lists.

2. **How a package follows it.** A package configures its libraries'
   directory (`--libdir`) as the environment's; `/usr/lib` keeps only
   musl's loader, a relative link to the directory's `libc.so`, reached
   as `/lib` through the merged-/usr links, and links to its startup
   files, which a gcc running from its build tree, as one bootstrapping
   does, looks for only in `/usr/lib` and `/lib`: the 64-bit root's
   stage 4, with no other C library in `/usr/lib`, failed without them.
   musl's shared part ships the loader's search path in `etc/`. A gcc is configured with the
   same `--libdir`, so that it finds the C library's startup files and
   libraries there; on x86-64 its multilib directory for the OS is the
   directory itself (`MULTILIB_OSDIRNAMES = m64=.`), not `../lib64`,
   which it would otherwise add to every library path. An experiment
   built such a gcc and musl in `linux/x86-64` and linked and ran a
   static and a dynamic program before the 64-bit root moved.

3. **The 64-bit bootstrap moves, from stage 2.** Stage 2, the
   cross-built root, and stage 3 install into
   `/usr/lib/x86_64-linux-musl`, so that stage 3 builds in the layout
   stage 4 does. A stage 3 built on a stage 2 with its libraries in
   `/usr/lib` made a gcc whose driver differed from the one stage 4 then
   made on stage 3's children: what gcc's configure finds of the C
   library is built into its driver. Stage 4 builds stage 3 again on its
   children, `offered_by` its own object, since the packages of the same
   versions would otherwise win the tie; when it differs, it shows how
   each differing file differs, a binary by its strings.

4. **gcc's runtime children** (§56) are in the environment's
   directory, where the loader finds them: until now gcc 7's x86 ones
   were in `/usr/lib`, which x86's loader doesn't search.

5. **Headers wait for glibc.** Headers matter only to a build, which
   chooses one C library, so a second C library's headers will be a
   root of their own, each gcc configured for one C library's root,
   when glibc arrives. Libraries are installed everywhere at run time,
   so their directory is settled now.

6. **The name.** The x86-64 directory carries no vendor field, as the
   target triples in labels don't (§53); x86's is live-bootstrap's, with
   one, which the bootstrap fixed and the contract records.

Rationale: where libraries live is what every installed package and
every loader depends on at run time, and so the costliest thing to
change once many packages exist; a directory per C library is what lets
musl and glibc, or any two C libraries, share an environment.

Affected specs:

- `schemas/environment/linux-v1.md` — the conventions.
- `DECISIONS.md` §56 (where gcc's runtime children go).

---

## 61. Only Releases Are Offered by Label

Decision: a requirement by label is satisfied only by a **release**, a
snapshot its object's creator has tagged with a version (§43), unless
the requirement **names the object** whose offer it takes
(`offered_by`, §59). An object's untagged head offers nothing to a
requirement that doesn't name it. So the bootstraps' stages, which are
steps toward a base and never releases, are reached only by the builds
that name them, and the packages, which are, are what every other
build gets, whatever order the two were published in. This amends what
a candidate offers (§33 sub-decision 2, the current snapshot) and the
order offers are ranked in (§48), where at equal owner and version the
newest published broke the tie between a package and the bootstrap's
child of the same version.

Sub-decisions:

1. **What is a release.** A snapshot one of its object's creator's
   version tags names (§43). An object's head is a release when it is
   tagged; a recipe fix after a tag is a release when the owner tags
   it, re-tagging the same version if it changes no version, which
   supersedes the earlier tag. Releases are offered by label beside one
   another, older ones too (§59).

2. **Naming an object takes its head.** A requirement with
   `offered_by` is satisfied by its named objects' offers, released or
   not, as an object dependency by id or snapshot is: it says which
   object it means. The bootstraps name the stage before them so:
   live-bootstrap's passes 2 to 6 the pass before, the 64-bit
   bootstrap's stage 1 and 2 pass 6 and stage 1, stage 3 stage 2, stage
   4 stage 3, `bootstrap-base` the passes and stage 3, and the hello
   consumer the passes, each by its id in the archive's canonical store
   (§59).

3. **What a build installs already.** A label the runtime closure asks
   for is met by an object the build installs already, released or not
   (§59 sub-decision 6), since the build chose it. Otherwise the closure
   chooses among releases, as any requirement that names no object
   does.

4. **The releases so far.** stage0-posix, whose `tool:kaem` and the
   rest are upstream's 1.9.1, is tagged 1.9.1, and linux-filesystem's
   merged-/usr layout 1.0.0, so pass 1 and every requirer of the layout
   resolve as before; the packages (§59) and the environment objects
   are tagged by their `import.sh`.

5. **Until the base is released.** `build-base` names a shell, the GNU
   userland, and the rest, which only the bootstraps offer until each is
   a package. Until then it can't be resolved, and every package is a
   first generation, built with `bootstrap-base`: gcc 7.5.0 too. A
   package's later release builds with `build-base` once the base it
   names is released.

6. **Tests and fixtures.** A fixture object that offers labels is
   tagged as a release, `0.0.0`, as the objects it stands for would be.

Rationale: a label is a request for software, and software is
published as releases; a head is work in progress, and a bootstrap's
stage a means to a base. Gating offers on the owner's own statement
that something is a release settles which of two equal versions a build
gets by what they are rather than when they were published, and keeps
the bootstrap's internals out of every resolution but the ones that ask
for them.

Affected specs:

- `DECISIONS.md` §33 sub-decision 2, §48, §57 sub-decision 6, §59.
- `BUILD.md` §3 — only releases by label.

## 62. A Plan Is Reused While What It Consulted Is Unchanged

Decision: a plan is a function of the snapshot planned, the target, and
what the resolver read of the store and the host on the way: the offers
of the labels it looked up, the objects it named, the owners' ranking,
and the host. The resolver records a **fingerprint** of each of those
with the plan, in a cache beside the store, and a later resolution of
the same object, snapshot, and target whose fingerprints all still
match reuses the plan instead of planning again. What the plan found
held (artifacts, the plans their builds cite, sources) is read again
each time, and a plan that would bind differently is planned again.
Reuse never changes a plan: a reused plan is byte for byte the plan a full
resolution would make, so its hash and build key are the same, and the
cache is only ever a cost (`STORE.md` §2.3, §6).

Sub-decisions:

1. **A label's fingerprint.** When the index is built (§59, the offers
   of every label made once), each label's offers are fingerprinted:
   a hash of each offer's object, snapshot, owner, and whether it is a
   release, in the index's order (§48). The snapshot fixes everything
   its manifest says of the offer, its version, artifact, environment,
   install root, confers, and requires; the order fixes how ties fall.
   It is computed per architecture, over the offers eligible for a
   target of that architecture (§38 sub-decision 8), content offers in
   every one, so a release built for x86 alone leaves x86-64's
   fingerprints as they were.

2. **Releases only, unless named.** A label's fingerprint covers only
   its releases, the offers a requirement naming no object can take
   (§61), so a new untagged head changes no plan that didn't name its
   object. A lookup by requirements that name objects (`offered_by`,
   §59) counts those objects' offers of the label too, released or not,
   so their heads are part of it. An object dependency, by id, version,
   or snapshot, records the object's fingerprint, its creator's head
   snapshot and its version tags, and a child its parent's. The planned
   object records its version tags, which decide its own generations
   (§59), and whether its snapshot is still its head, but not where
   its head has gone since: a fix to make that isn't tagged leaves
   make 4.2.1's plan as it was.

3. **Every lookup counts.** A plan records the fingerprint of every
   label it looked up and every object it named, chosen or passed
   over: a label consulted only in a branch the resolver abandoned, or
   by the runtime closure before it converged (§46 sub-decision 7),
   decided the plan as much as one it chose from. A plan that went on
   past a provider whose planning failed, using its held artifact
   (§52), isn't recorded: the failure may depend on what was being
   planned around it.

4. **What else it records.** The **ranking**: the tier of each owner
   whose offers it ranked (§48), which `--prefer`, `[resolve] prefer`,
   the local actors, and their trust decisions at `general` scope
   decide; trusting an owner replans the plans that ranked its offers.
   A **host fingerprint**: the host's contract, the engines it offers,
   in development mode its host tools (§34 sub-decision 10), and the
   seeds accepted (§37). And the
   **resolver's version**, a number the resolver raises whenever it
   would choose differently from the same inputs; a plan recorded by
   another version is planned again. A host change or a new resolver
   invalidates every plan; each is rare.

5. **Providers are plans of their own.** A plan records the providers
   it builds first by their own object, snapshot, and target, not
   their choices; each provider's plan is recorded and checked in its
   own right. A plan is reused when its own fingerprints match and each
   provider's plan is reused too, so a new release of `tool:make`
   replans make's consumers, and their consumers, and nothing else.

6. **Bindings are read again.** A plan records what it found held,
   rather than offered: each provider's held artifact, the plan its
   held build cites, an artifact found by build key (§52), and each
   source commit (§37). On reuse each is read again, lookups by key,
   cheap beside a resolution; a plan that would find otherwise is
   planned again. They are what changes when a build finishes, which
   leaves every fingerprint as it was, so the plans that bound to the
   build are planned again and the providers' beneath them reused.

7. **Where it lives.** Under the store's `cache/plans/`, by object,
   snapshot, and target: the plan, its fingerprints and bindings, the
   providers it built on, the resolver version, and what it passed
   over, shown again when it is reused, unsigned and never federated. It is cache data:
   removing it costs the next resolution its full time and changes
   nothing else, and a record that can't be read is planned again,
   never an error. A failed resolution records nothing.

8. **Checking it.** `kairo build --replan` ignores the cache, for the
   plan and every provider's, and records what it plans. Tests hold a
   reused plan equal to a fresh one after each kind of change
   (a release of a consulted label, of an unconsulted one, an untagged
   head, a named object's head, a trust change), and the resolver
   counts the plans it reuses beside the plans it makes (§59), so a
   test and `scripts/plan-timing.sh` see when reuse stops happening.

9. **Not yet.** A plan with a changed fingerprint or binding is
   planned again in full, though its unchanged providers are reused.
   Re-choosing only the changed labels and what came through them,
   keeping the rest, and binding unchanged choices to newly held
   builds, are the next steps once full replans cost enough to matter;
   each must give the plan a full resolution would.
   Persisting the index itself, which every command now builds by
   reading every held object, waits until building it does. A
   candidate's record of being passed over, its reputation, may order
   offers to try but never excludes one: a rejection depends on what
   the requirement asked, and a later one may ask differently.

Rationale: planning is a pure function of what it reads, so a plan
stays right exactly as long as what it read stays the same, and the
cheapest correct test of that is to fingerprint what it read. A
counter each write bumps was the alternative: cheaper to compare, but
every path that writes an object, a tag, or a trust record, federation
among them, would have to bump the right ones, and a missed bump is a
stale plan that nothing catches. A fingerprint is computed from the
store as it is, by the index already built for every resolution, so
it can't fall out of step with it; and it is per label, not per
store, so a new release replans only the plans that looked at its
label.

Affected specs:

- `PLANNER.md` §19 — recorded plans and their reuse.
- `STORE.md` §4 — `cache/plans/`.
- `CLI.md` §17 — `kairo build --replan`.
- `PHASE_2.md` §8 — fingerprints and reused plans.

## 63. Packages Outside the Base Build with `build-base` from Their First Release

Decision: only the packages `build-base` names, its base, build their
first generation with `bootstrap-base` (§59 sub-decision 5). Any other
package builds with `build-base` from its first release: nothing it
offers is a label `build-base` brings, so its build can't take itself
through it, and the cycle §59 avoids can't form. zlib, libffi, perl,
and Python are the first: built with packages alone, no bootstrap's
object in their plans. This narrows §59 sub-decision 5, which said
every package's first generation names its bootstrap.

Sub-decisions:

1. **What it builds against is a package too.** A library such a
   package builds against, as Python does zlib, libffi, liblzma, and
   libbz2, is required by label beside `build-base` and resolves, as
   `build-base`'s labels do, to releases alone (§61), so to the
   packages and never to a bootstrap's child of the same software.

2. **When a package joins the base.** A package `build-base` comes to
   name, as perl or Python might once a base needs them, has its
   releases built with `build-base` by then; one built before then
   with `build-base` is an older generation of itself, which a later
   release may build with (§59), and its own target is barred from its
   own offers as any build's is.

Rationale: `bootstrap-base` exists to break the cycle between a base
package and the packages that build it. Software outside the base has
no such cycle, and building it with the packages is what every later
package will do; naming the bootstraps would tie it to objects that
are steps toward a base, never releases (§61).

Affected specs:

- `DECISIONS.md` §59 sub-decision 5.
- `packages/bootstrap-base/README.md`, `packages/build-base/README.md`.
