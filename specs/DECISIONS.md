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
