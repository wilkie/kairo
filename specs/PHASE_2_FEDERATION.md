# PHASE_2_FEDERATION.md

## Status

Draft, locked 2026-10-16. All six slices and the post-slice-5 fix-up
shipped; §4 lists what comes next.

Slice plan for the first federation milestone in `PHASE_2.md` §4:
pulling a published object's signed history from one node into
another. The locked shape is `DECISIONS.md` §16; this document is
the build decomposition.

References:

- `DECISIONS.md` §16 — the ten sub-decisions.
- `FEDERATION.md` — long-term design; its "Status" section scopes
  it to this plan.
- `crates/kairo-bundle/AGENTS.md` — the fixity and
  "what goes in" rules the closure builds on (and the one place
  §16 sub-decision 4 departs from them).
- `crates/kairo-store/AGENTS.md` — shard scheme and the
  derived-vs-primary state rule provenance follows.
- `THREAT_MODEL.md` — the federation-peer adversaries (withheld
  revocations, garbage floods) the client limits answer.

Out of scope: see `DECISIONS.md` §16 sub-decision 10.

---

## 1. Crate Layout

| Location | Change |
|----------|--------|
| `crates/kairo-federation-core` (new) | Wire types: `NodeDocument`, `ObjectListing`, `ActorListing`, `PublishedObjects`, `Cursor` / `PageRequest`, `FedErrorEnvelope` / `FedErrorCode`. Serde only; depends on `kairo-core` and `kairo-identity`. No I/O. |
| `crates/kairo-federation-http` (new) | `server` (closure computation, axum router), `client` (fetch + verify + ingest). Depends on `kairo-federation-core`, `kairo-store`, `kairo-statement`. `kairo-federation-http` binary. |
| `crates/kairo-store` | `published` module: `PublishedObjectStore` trait + `FilesystemStore` impl over `<store>/federation/published.json` (slice 1). `provenance` module: `ProvenanceStore` trait + `FilesystemStore` impl over `<store>/provenance/` (slice 3). Both live here, not in the federation crate: they need the store's index lock and atomic write, and the daemon reads provenance (slice 5) without depending on federation. |
| `crates/kairo-cli` | `kairo fetch --object <id> --from <url>`; `kairo federation publish \| unpublish --object <id>`, `published`, `provenance`, `status` |
| `crates/kairo-config` (new, slice 5) | `<store>/config.toml` parsing (`DECISIONS.md` §17) |
| `crates/kairo-daemon` | Slice 5 only: `GET /api/v1/provenance/{record}` |
| `frontend/` | Slice 5 only: "Fetched from" badge |

`kairo-federation-dht` and `kairo-federation-service` from
`PROJECT_LAYOUT.md` are not created in this plan.

---

## 2. Cross-cutting Conventions

### 2.1 Endpoints

| Method | Path | Response |
|--------|------|----------|
| `GET` | `/.well-known/kairo-node` | `NodeDocument` |
| `GET` | `/federation/v1/objects` | `Page<ObjectId>` of published objects |
| `GET` | `/federation/v1/objects/{id}` | `ObjectListing` (paged) |
| `GET` | `/federation/v1/objects/{id}/genesis` | `ObjectGenesis` JSON, exactly as stored |
| `GET` | `/federation/v1/objects/{id}/git` | Git pack, streamed, `application/x-git-packed-objects` |
| `GET` | `/federation/v1/actors/{id}` | `ActorListing` |
| `GET` | `/federation/v1/statements/{id}` | statement JSON, exactly as stored |
| `GET` | `/federation/v1/blobs/{id}` | blob bytes, `application/octet-stream` |

Every other method or path is `405` / `404`. `{id}` is a bare id.
An id outside every published closure is `404 fed_not_found`,
whether or not the store holds it.

### 2.2 Shapes

```json
{
  "schema": "kairo.fed.node.v1",
  "node_actor": "z6MkNode...",
  "actor_genesis": { "...": "ActorGenesis JSON as stored" },
  "base_url": "https://archive.example.org",
  "protocol_versions": ["kairo-fed/1"],
  "supports": ["object-pull"]
}
```

```json
{
  "schema": "kairo.fed.object-listing.v1",
  "object": "z6MkObject...",
  "statements": ["z6MkStatement..."],
  "authority": ["z6MkStatement..."],
  "actors": ["z6MkActor..."],
  "blobs": ["z6MkBlob..."],
  "git": { "available": true },
  "next_cursor": null
}
```

`statements` holds the object-bound envelope kinds: every
`ObjectBranch` and `ObjectVersionTag` chain entry (not just heads), and
the `ObjectRevision`s those refs and the genesis's initial revision
reach along parent links (`DECISIONS.md` §20). `ObjectGenesis` is not among them: it has
no statement id, because its body derives the object id, so it is
fetched from `/objects/{id}/genesis`. `authority` holds
object-scoped capability grants and revocations of them
(§16 sub-decision 4). Paging walks the four id lists in that order,
so a cursor is a position in their concatenation; `git` appears on
the first page only.

`ActorListing` is `{ schema: "kairo.fed.actor-listing.v1", actor,
genesis, key_events: [statement ids] }`, unpaged (key-event
histories are short). Only actors in some published closure are
served.

### 2.3 Error codes

| Status | Code | Meaning |
|--------|------|---------|
| 404 | `fed_not_found` | missing *or* unpublished; deliberately indistinguishable |
| 400 | `fed_bad_id` | path segment isn't a valid id of that type |
| 400 | `fed_bad_cursor` | cursor didn't decode |
| 400 | `fed_bad_limit` | `limit` isn't an integer from 1 to 1000 (refused, not clamped) |
| 405 | `fed_method_not_allowed` | anything but `GET` / `HEAD` |
| 500 | `fed_store_error` | store read failed (message is generic; details logged) |
| 503 | `fed_busy` | pack generation at capacity; `Retry-After` says when to retry (`DECISIONS.md` §21) |

Envelope is `API.md` §7.2.

### 2.4 Client limits

Defaults, overridable per invocation and later in `[federation]`
config: 1 MiB per statement or listing page, 256 MiB per blob,
1 GiB per Git pack, 100 000 records per pull, 30 s per request
(30 min for the Git pack request).
Exceeding one fails the pull with what was stored so far kept.

### 2.5 Test harness

- Server: in-process axum router against a seeded
  `FilesystemStore` in a tempdir (`kairo-test-support` builders).
- Client: two tempdir stores; server bound to `127.0.0.1:0`; pull
  into the second; assert the result with the same resolvers
  `kairo verify object` uses.
- Adversarial: a test-only server that serves tampered bytes,
  wrong-id records, oversize bodies, and withheld key events.

---

## 3. Slice Sequence

### Slice 1 — Wire types and publish list *(shipped)*

- `kairo-federation-core` with the §2.2 types. Ids parse into
  their `kairo-core` newtypes, every top-level document is
  `deny_unknown_fields`, and each `schema` field accepts only its
  exact string. `NodeDocument::validate` checks the base URL,
  `kairo-fed/1`, that `node_actor` derives from `actor_genesis`,
  and kind `node` (`ActorKind::node()` added to
  `kairo-identity`). `ActorListing::validate` does the
  derivation check. Unknown error codes from newer nodes parse as
  `Unknown`.
- `kairo-store` `published` module: read/modify/write of
  `<store>/federation/published.json` (`[{ object_id,
  published_at }]`, sorted by id) under the store's index lock.
  It is primary state, which `rebuild_indexes` leaves alone.
  Publishing requires the object's genesis locally and is
  idempotent; the first `published_at` is kept.
- `kairo federation publish --object <id>` (the object must exist
  locally), `unpublish --object <id>` (prints "peers that already
  fetched this object keep their copy"), and `published`. These
  use `--object` like the other v1 subtrees, not `CLI.md` §25's
  positional `<object-ref>`.

### Slice 2 — Serving *(shipped)*

- Closure computation is `FilesystemStore::object_closure` in
  `kairo-store`, since it reads the store's private indexes:
  revisions by directory walk (there is no revision index yet),
  every branch, tag, and object-scoped grant entry from the
  per-object indexes, and those grants' revocations from each
  grantor's index. `actor_key_event_ids` lists an actor's key
  events; `get_object_genesis_bytes` serves the genesis raw. The
  trust index is never read.
- The server builds a *published view* (every published closure,
  plus the key events of every actor those closures name), and
  each per-record endpoint checks membership in it before reading
  the store. The view is reused for `--view-ttl` seconds (default
  5). That bounds the cost of a pull's many per-record requests,
  and also how long an `unpublish` takes to stop serving.
- Listing cursors are `<section>.<last id>` (`st`, `au`, `ac`,
  `bl`), so page boundaries survive records being added between
  requests. `git` is `{ available: false }` until slice 4.
- `kairo-federation-http` binary with `--store`, `--node-actor`,
  `--bind` (default `127.0.0.1:7880`), `--base-url` (default
  `http://<bind>`), and `--view-ttl`. A non-loopback bind logs a
  warning naming the reverse-proxy expectation. Every non-`GET` /
  `HEAD` request, including to unknown paths, gets `405
  fed_method_not_allowed` with `Allow: GET, HEAD`. Unknown paths
  get `404 fed_not_found`.
- `--node-actor` must be in the store with kind `node` (refused
  otherwise). The server warns, without refusing, if the node actor
  created objects or signed object or capability statements.
  Nothing indexes grants by grantee across objects, so "holds no
  grant" is checked from what the actor signed.
- Tests (`crates/kairo-federation-http/tests/serve.rs`, plus store
  unit tests) cover:
  - every endpoint, and that the closure matches the seeded set
    (no `ActorTrust`, no other object's statements or grants, a
    grantee who signed a branch listed, a grantor's revocation
    included);
  - paged listings reassembling to the whole;
  - unpublished-but-present records returning a body
    byte-identical to a missing record's;
  - statement, genesis, and blob bytes equal to the stored
    files;
  - `unpublish` taking effect, malformed ids, limits, and
    cursors, the method guard, and node-actor refusals.

### Slice 3 — Pull with verification and provenance *(shipped)*

- `kairo_federation_http::client::pull_object` runs: well-known
  (validated) → listing (all pages, deduplicated) → each actor
  (genesis derives its id; missing key events ingested in
  `created_at` order) → genesis → authority (grants, then
  revocations) → object statements (revisions, then branches and
  tags) → blobs. Each record is fetched only if absent, then
  checked and stored via `put_*`. The checks are: schema; fixity
  (the re-derived id equals the requested one); section (the
  record belongs where it was listed, and `ActorTrust` is always
  refused); and signature, through the same
  `verify_envelope_statement` / `verify_envelope_multi_statement`
  calls the daemon uses, plus the genesis check against
  `created_by`'s active key.
- A statement signed by a key revoked at its `created_at` is
  stored and flagged in the report, and the pull continues
  (§16 sub-decision 7, amended by the fix-up below; it was
  skipped at first).
- Transport is `ureq` 3 with rustls (ring, webpki roots). It
  follows no redirects, so an `https://` peer can't bounce the
  client to plain HTTP, and applies the §2.4 limits per request.
  The crate splits into `server` and `client` features, and the
  CLI takes `client` only.
- `kairo-store` `provenance` module: `ProvenanceStore` over
  `<store>/provenance/`, keyed by typed reference
  (`statement:`, `object:`, `actor:`, `blob:`). Entries are
  appended under the index lock, one per distinct `peer_url`, and
  the first `fetched_at` is kept.
- CLI: `kairo fetch --object <id> --from <url> [--json]` reports
  fetched / already-held / flagged counts.
  `kairo federation provenance --ref <typed-ref>` prints the peers,
  or says the record is local.
- Tests (`tests/pull.rs`, real server on a loopback port) cover:
  - a round trip (branch heads and key events resolve the same on
    both stores; nothing outside the closure arrives);
  - re-pull is a no-op that keeps the first provenance;
  - a second peer appends;
  - revoked-key statements are stored and flagged.

  Every adversarial case fails the pull without storing the bad
  record: tampered bytes, a substituted statement, withheld key
  events, smuggled `ActorTrust`, a node document claiming another
  actor, an oversize blob (earlier records kept), the record
  limit, an unpublished object, and plain HTTP to a remote host.
  Store tests cover provenance dedup, mismatched records,
  snapshot refs, and `rebuild-indexes` leaving it alone.

### Slice 4 — Git pack transfer *(shipped)*

- `GET /federation/v1/objects/{id}/git` streams the same pack
  `kairo bundle export --include-git` produces
  (`GitCache::pack_for_object_to`). `pack-objects` runs on the
  blocking pool and writes into a bounded channel the response body
  drains, so the pack never sits in memory and a slow peer applies
  backpressure. A failure partway through aborts the body; the
  receiver's `index-pack` then rejects the truncated pack.
- The listing's `git.available` is true when the managed cache
  (`<store>/git`) has a per-object repo for the object. The server
  checks this without opening the cache, so it never initializes
  the pool. An unpublished object's pack is `404` like every other
  record.
- After blobs, the client gathers the `git:sha256:` commits of
  every pulled revision. It fetches the pack only if some commit is
  missing locally, and streams it straight into
  `GitCache::ingest_pack_from`. Then it ensures the object's repo
  exists, checks that every commit resolves, and pins each newly
  ingested one under `refs/kairo/imported/<oid>` (as bundle
  import does), so the node can serve the pack onward. The outcome
  is reported as `git = no git revisions | already held | ingested
  | unavailable from peer`:
  - A peer without a pack still yields a successful pull, with
    content verification left indeterminate.
  - A pack lacking a signed commit fails the pull
    (`GitIncomplete`), keeping the statements already stored.
  - The `git` binary is only needed when a pack is actually
    ingested.
- Tests (`tests/git.rs`, skipped without `git`) cover:
  - ingest and pin, re-pull (`already held`), and relaying the
    ingested pack onward;
  - a peer without a pack (`unavailable`, no local cache
    created);
  - streamed bytes equal the cached pack, and unpublish hides it;
  - a pack from an unrelated repo (`GitIncomplete`);
  - an oversize pack.

  The CLI test `fetched_object_verifies_from_the_cache_alone` runs
  the slice's acceptance check: after `kairo fetch`, `kairo verify
  object --no-cwd-repo` on the second store is VALID with `commit
  lookup: cache`.

### Slice 5 — Policy and visibility *(shipped)*

- `<store>/config.toml` with a strict `[federation]` section
  (`DECISIONS.md` §17), parsed by the new `kairo-config` crate:
  `enabled`, `[federation.limits]` overrides, and `[[federation.peers]]`
  entries with a `url` and an optional `node_actor` pin.
  `kairo config path | validate`.
- `kairo fetch` checks policy before any network I/O. `--offline`
  and `enabled = false` refuse. An unlisted peer needs
  `--allow-peer` (one run, config untouched). A pin goes to the
  pull as `PullOptions::expected_node_actor`, which refuses a
  mismatched claim before fetching anything
  (`PullError::NodeActorMismatch`). The output says whether the
  peer was pinned, unpinned (with the line to pin it), or
  unlisted. Exit codes are 7 for policy refusals and 11 for pull
  failures.
- `kairo-store` `pulls` module: the last successful pull per peer
  at `<store>/federation/pulls.json`. `kairo federation status`
  shows enablement, the config in effect, the publish list,
  configured peers with pins and last pull, and any unlisted peers
  pulled with `--allow-peer`.
- Provenance reaches the inspector through a new read endpoint,
  `GET /api/v1/provenance/{record}` (typed reference; empty
  `entries` for local records). This replaces the planned
  `provenance` field on read DTOs: the statement, object, and
  actor detail responses are the signed records' JSON, and
  provenance is local state that shouldn't be mixed into them.
- Inspector: `<ProvenanceBadge />` in `@kairo/ui` ("Fetched from
  <peer> [+N]"; the tooltip lists every peer and time and marks
  the node actor as claimed), plus a `useProvenance` hook. The
  badge shows on the statement, object, and actor detail pages and
  renders nothing for local records. MSW seeds provenance for Beta
  and one Alpha revision.
- Tests:
  - `kairo-config` parsing (strictness, validation, defaults);
  - the pull log;
  - the pin refusing before anything is stored;
  - CLI policy refusals and exit codes, status, and config
    commands;
  - the end-to-end fetch, now through a pinned config entry;
  - the daemon provenance endpoint;
  - badge unit tests and an MSW Playwright spec.

### Fix-up after slice 5 *(shipped)*

A review of slices 1–5 found gaps, fixed before slice 6 so the
two-node check runs against the corrected behavior:

- **Git object formats** (`DECISIONS.md` §19). Revisions are
  `git:sha1:` / `git:sha256:`, parsed strictly. Each object has one
  format, there are per-format pools, gix is upgraded to 0.89 for
  SHA-256 reads, bundles name typed revisions, and `kairo git fetch`
  takes `--object-format`. The pull ingests a pack into the pool
  for its commits' format; an object whose revisions mix formats is
  refused.
- **Pack completeness.** After ingest, `missing_objects` checks
  every commit's full closure. The object's repo is created and
  commits pinned only when it passes, and a failure leaves no repo
  behind. The server's `git.available` now means the repo has refs,
  not just that it exists.
- **Revoked-key statements are stored and flagged**, not skipped, so
  two nodes don't diverge (§16 sub-decision 7).
- **Honest node identity.** A matching pin prints "claim matches,
  node not authenticated", and node authentication is now a
  prerequisite of discovery as well as push (§16 sub-decision 9,
  §18).
- **Honest key-event completeness.** A withheld revocation can't be
  detected from one peer; §16 sub-decision 4 says so and names the
  defenses (re-pull, multiple peers).
- **Peer URLs** are normalized (lowercase scheme and host) on both
  the config side and the `--from` side, and `kairo config
  validate` checks them.
- Tests:
  - a SHA-256 history federating into its own pool;
  - a failed Git step leaving no repo;
  - a commit-only pack caught by `missing_objects`;
  - wrong-format packs and repos refused;
  - strict ids at `revision create`, including a mislabeled SHA-1
    id and a mixed-format history;
  - `git fetch` asking for a format;
  - SHA-256 reads in `kairo-git`.

  Fixtures now use valid-shaped ids, with real fixture repos labeled
  `git:sha1:`.

### Slice 6 — Two-node real-stack check *(shipped)*

- `scripts/federation-smoke.sh [TARGET_STORE]` builds an origin node
  with real binaries:
  - a node actor;
  - a person actor whose object has a revision bound to a real Git
    commit (`OBJECT_FORMAT=sha1|sha256`);
  - that commit in the origin's managed cache;
  - the object published.

  It serves the origin with `kairo-federation-http`, writes the
  target's `config.toml` with the peer and its pinned node actor,
  and runs `kairo fetch`. It then checks:
  - the peer is pinned and the Git pack is ingested;
  - `kairo verify object --no-cwd-repo` is VALID with `commit
    lookup: cache`;
  - provenance names the origin;
  - a re-fetch fetches nothing and finds Git already held.

  It writes `federation-ids.json` for the e2e suite, and refuses a
  target that already has a `config.toml`.
- CI's `e2e-real` job runs the script for SHA-1 and SHA-256 before
  Playwright.
- The real-stack global setup runs it into the e2e store after
  `smoke-seed.sh`, so the inspector has a pulled object.
  `e2e-real/federation.spec.ts` checks:
  - the pulled object's page shows "Fetched from <origin>", with the
    node actor marked as claimed;
  - so does the pulled revision's statement page;
  - a locally created object shows no badge.

---

## 4. After This Plan

In rough order, each needing its own decision:

0. **Incremental fetch** (`DECISIONS.md` §21 sub-decision 6): the
   client names the commits it already has and the server packs only
   the difference, as Git's negotiation does. Then reachability
   bitmaps, and bundle or packfile URIs for static hosting.
1. **Push** with node authentication (signed node document,
   `KairoActor1` on requests) and a receiver acceptance policy.
2. **Trust transport** (opt-in, never auto-trusted) and `forget`.
3. **Daemon `FederationService`**: background sync, on-demand
   fetch of missing closure data (`DAEMON.md` §9).
4. **Discovery** — token advertisements, indexes, query protocol
   — once `PHASE_2.md` §7 / §8 statement types exist. Also needs
   scoped trust (`DECISIONS.md` §18), trust transport (item 2), and
   node authentication (`DECISIONS.md` §16 sub-decision 9), which
   push (item 1) brings.

### 4.1 Bootstrap peers, learned peers, node trust

Decided in `DECISIONS.md` §18:

- bootstrap nodes and allowed peers are separate config roles;
- learned peers are state recorded with who advertised them, and
  are never permissions on their own;
- node-to-node trust is `ActorTrust` signed by the node actor;
- `ActorTrust` gains a single scope per chain (`general` for
  today's opinions; `federation:index` and `federation:push`
  defined by the features that use them), with no fallback
  between scopes;
- propagation is opt-in and depth-limited.

Sequencing consequence: scoped `ActorTrust` lands before discovery,
which needs `federation:index` to decide which learned peers to
follow. Push needs `federation:push`.

## 5. Risk Notes

- **Publishing is irreversible.** Opt-in and absence ambiguity
  limit accidents; nothing can recall a fetched copy.
- **Closure completeness is a correctness property.** A server
  bug that omits a revocation makes peers accept statements the
  origin wouldn't. Slice 2's closure tests and slice 3's
  "withheld key events" case are the guard; the resolver's
  `Indeterminate` handling is the backstop only when the gap is
  detectable.
- **Unauthenticated node identity.** Until push, `peer_actor` is a
  claim. Slice 5's pinning turns it into trust-on-first-use; it is
  never used for authorization in this plan.
- **First TLS dependency.** rustls enters through the client only;
  keep it out of core crates and the daemon.
- **Signature verification on ingest is new.** Bundle import does
  not verify signatures, so the store has two entry points with
  different guarantees. Routing bundle import through the pull's
  verifier is tracked in `PHASE_2.md` §11.
- ~~**Serving a pack holds the object's lock for the whole
  transfer.**~~ Resolved by `DECISIONS.md` §21: lock-free snapshot
  packing, a pack cache, per-object single-flight, a generation cap
  with `503 fed_busy`, and a transfer deadline. The pulling side's pool
  lock during download is gone too (quarantined ingest).
- ~~**Rebuilding the published view walks the store.**~~ Resolved
  by `DECISIONS.md` §20: closures walk each object's reachable
  history through the per-object revision index, so a rebuild costs
  the published histories, not the store.
- ~~**Pool junk from failed pulls.**~~ Resolved by `DECISIONS.md`
  §21: incoming packs are quarantined, and only a complete pack moves
  into the pool.
- **Strict config parsing on downgrade.** An older `kairo` rejects a
  config written for a newer one (`DECISIONS.md` §17). That is
  acceptable before 1.0.

---

End of PHASE_2_FEDERATION.md
