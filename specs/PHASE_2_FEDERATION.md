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
| `GET` | `/federation/v1/objects/{id}/git/info/refs?service=git-upload-pack` | Git protocol v2 capability advertisement (`DECISIONS.md` §22) |
| `POST` | `/federation/v1/objects/{id}/git/git-upload-pack` | Git protocol v2 `ls-refs` / `fetch`, filtered to published commits (§22) |
| `GET` | `/federation/v1/actors/{id}` | `ActorListing` |
| `GET` | `/federation/v1/statements/{id}` | statement JSON, exactly as stored |
| `POST` | `/federation/v1/statement-batch` | up to 256 statements, each exactly as stored, in request order; unserved ids left out (`DECISIONS.md` §24) |
| `GET` | `/federation/v1/blobs/{id}` | blob bytes, `application/octet-stream` |
| `GET` | `/federation/v1/inventory` | `{ schema: "kairo.fed.inventory.v1", node_actor, base_url, objects, signed_at, signature }`: the published objects, strictly ascending, signed by the node actor when the view is built (`DECISIONS.md` §29; signed bytes in `schemas/canonical/inventory-v1.md`) |
| `GET` | `/federation/v1/trust` | `{ schema: "kairo.fed.published-trust.v1", chains: [{ by, scope }] }`: published trust chains (`DECISIONS.md` §27) |
| `GET` | `/federation/v1/trust/{by}?scope={scope}` | `{ schema: "kairo.fed.trust-listing.v1", by, scope, statements }`: one published chain, withdrawals included; the statements and the truster's actor listing are served |
| `POST` | `/federation/v1/push/plan` | `KairoNode1`-signed `{ schema: "kairo.fed.push-plan-request.v1", object, inventory }`; `200` `{ schema: "kairo.fed.push-plan.v1", genesis, actors, statements, blobs, have_commits }`: what this node lacks (`DECISIONS.md` §26 sub-decision 6) |
| `POST` | `/federation/v1/push?object={id}` | `KairoNode1`-signed closure bundle tar (§26 sub-decision 8), at most `max_push_bytes`; `202` `{ schema: "kairo.fed.push-accepted.v1", entry }` |
| `POST` | `/federation/v1/announce` | `KairoNode1`-signed `{ schema: "kairo.fed.announce.v1", base_url, objects }` (at most 256 objects); `202` `{ schema: "kairo.fed.announce-accepted.v1", entry }` once it is in the inbox (`DECISIONS.md` §26) |

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
  "supports": ["object-pull", "statement-batch"],
  "signed_at": "2026-10-18T12:00:00Z",
  "signature": { "key_id": "z...", "algorithm": "ed25519", "bytes": "<base64>" }
}
```

The node signs the document at startup (`DECISIONS.md` §25 sub-decision
1; signed bytes in `schemas/canonical/node-document-v1.md`). A pull
refuses a document whose `base_url` isn't the URL pulled from, fetches
the node actor's listing and key events, and refuses a signature that
isn't by the node actor's key active at `signed_at`.

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
served, plus the node actor, which is always served with its key
events so a peer can check the node document (`DECISIONS.md` §25
sub-decision 2).

### 2.3 Error codes

| Status | Code | Meaning |
|--------|------|---------|
| 404 | `fed_not_found` | missing *or* unpublished; deliberately indistinguishable |
| 400 | `fed_bad_id` | path segment isn't a valid id of that type |
| 400 | `fed_bad_cursor` | cursor didn't decode |
| 400 | `fed_bad_limit` | `limit` isn't an integer from 1 to 1000 (refused, not clamped) |
| 400 | `fed_bad_batch` | statement batch request that doesn't parse, or names no ids or more than 256 (`DECISIONS.md` §24) |
| 400 | `fed_bad_git_request` | Git request the node won't serve: not protocol v2, a command other than `ls-refs` / `fetch`, an unpublished `want`, a malformed body (`DECISIONS.md` §22) |
| 400 | `fed_bad_announcement` | an authenticated announcement that doesn't parse, or names no objects, more than 256, an object twice, or an invalid `base_url` (`DECISIONS.md` §26) |
| 400 | `fed_bad_push` | a plan request that doesn't parse, or whose inventory is over 100 000 ids, names one twice, or names a commit that isn't a `git:` revision; an upload without `?object=` or whose body stream fails (`DECISIONS.md` §26) |
| 413 | `fed_too_large` | an upload over this node's `max_push_bytes`; nothing is kept |
| 403 | `fed_not_trusted` | the receiving node actor has no `trusted` `federation:push` opinion of the announcer (§26 sub-decision 1) |
| 403 | `fed_not_accepted` | no `[[federation.accept]]` entry covers an announced object for this announcer; the message names them (§26 sub-decision 2) |
| 401 | `fed_unauthenticated` | an endpoint that acts on who is asking got no valid `KairoNode1` signature: missing, malformed, outside ±300 s, a replayed nonce, an unknown or non-`node` signer, a key not active or revoked at the request's `timestamp`, or a signature that doesn't verify over this node's base URL (`DECISIONS.md` §25 sub-decision 3). Read endpoints never send it |
| 405 | `fed_method_not_allowed` | anything but `GET` / `HEAD`, except the `git-upload-pack`, `statement-batch`, `announce`, `push/plan`, and `push` `POST`s |
| 500 | `fed_store_error` | store read failed (message is generic; details logged) |
| 503 | `fed_busy` | concurrent Git fetches at capacity, or an announcer's inbox share (`max_inbox_entries`) is full; `Retry-After` says when to retry (`DECISIONS.md` §21, §22, §26) |

Envelope is `API.md` §7.2.

### 2.4 Client limits

Defaults, overridable per invocation and later in `[federation]`
config: 1 MiB per statement or listing page, 32 MiB per statement
batch response (`DECISIONS.md` §24), 256 MiB per blob,
1 GiB per Git fetch (measured in the fetch's quarantine), 100 000
records per pull, 30 s per request (30 min for the Git fetch).
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
  `http://<bind>`), and `--view-ttl`; `--keys` (default
  `<store>/keys`, the node actor's signing key) came with node
  authentication (`DECISIONS.md` §25). A non-loopback bind logs a
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

### Slice 4 — Git pack transfer *(shipped; transport replaced by `DECISIONS.md` §22)*

The single-pack endpoint below was replaced by Git's smart HTTP
protocol v2 (§22): `git fetch` with negotiation, so a re-pull moves
only new objects. The listing's `git.available`, the outcome
reporting, and the pinning under `refs/kairo/imported/` are
unchanged.


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

In rough order, each needing its own decision. Items 1, 2, 6's
inventory step, and 7 come from comparing this design with Radicle
(§4.2).

0. ~~**Incremental fetch**~~ — done by `DECISIONS.md` §22 (Git smart
   HTTP v2). Next on the content plane: reachability bitmaps, then
   bundle or packfile URIs for static hosting, both Git
   configuration now.
1. ~~**Actor chains** (signed statement heads)~~, *decided in
   `DECISIONS.md` §23, shipped*. The statement envelope gains `previous`, the
   signer's tips in the statement's chain (per object, per capability
   target, or the actor's key events). Every chained statement is its
   own head, so there are no extra records. Withholding inside a chain
   becomes a detected, attributable gap (`THREAT_MODEL.md` §5.15), and
   each chain is causally ordered. Freshness still needs witnesses
   (§23 sub-decision 9). Implementation steps:
   1. ~~the envelope field and every schema doc's envelope section~~
      (shipped);
   2. ~~tips and completeness in the store~~ (shipped);
   3. ~~every write path signs with the tips, and the daemon API
      exposes them~~ (shipped);
   4. ~~ingest order and the pull report~~ (shipped);
   5. ~~verification and the daemon API's completeness~~ (shipped).
2. ~~**Batch record fetch.**~~ *Shipped as statement batches,
   `DECISIONS.md` §24.* A pull fetches statements 256 at a time
   instead of one request each, still verifying every statement on
   its own. Absence stays ambiguous, and a statement a batch leaves
   out is fetched per record. Blobs stay per record.
3. ~~**Push**~~ *(shipped)* with node authentication, *decided in `DECISIONS.md` §25
   (node authentication) and §26 (push)*. Slices, in order:
   1. ~~**Scoped `ActorTrust`**~~ (§18; shipped): the `scope` field,
      the chain key `(truster, trusted, scope)`, and `kairo trust
      --scope`.
   2. ~~**Signed node document** and the always-served node actor
      listing~~ (§25 sub-decisions 1–2; shipped). Pulls report
      authenticated peers.
   3. ~~**`KairoNode1` request signing and verification**~~ (§25
      sub-decision 3), with the nonce cache (shipped). The wire format
      is in `kairo-federation-core`; `Node::authenticate_request`
      verifies, for slice 4's endpoints to call. A receiver
      authenticates only node actors whose genesis and key events it
      holds, so slice 4 has to say how it learns a sender's (for
      example, from the sender's node document and always-served
      listing, as a pull does).
   4. ~~**Accept list, inbox, and announce**~~ (§26 sub-decisions 1–3,
      7; shipped), plus `kairo federation announce`, `receive`, and
      `learn` for announcements. The server reads the accept list and
      push limits from `<store>/config.toml` at startup.
   5. ~~**Upload**~~ (`kairo push`, `POST /federation/v1/push/plan`
      and `/push`; shipped), sending only the delta, with closure
      bundles ingested by `kairo federation receive` through the pull
      verifier (§26 sub-decisions 4, 6, 8).
   - ~~A receiver must hold a pushing node's actor before it can
     check the upload's signature~~ *(shipped)*: a node that is never
     reachable is learned from `kairo actor export`, with `kairo
     federation learn --file` (§26 sub-decision 7).
4. ~~**Trust transport** (opt-in, never auto-trusted) and `forget`~~
   *(shipped; `DECISIONS.md` §27)*. A node serves only (truster,
   scope) chains its operator publishes; `kairo trust fetch` pulls one
   chain through the pull verifier; fetched opinions stay the
   truster's; `kairo trust forget` deletes an imported truster's
   opinions.
5. ~~**Daemon `FederationService`**~~ *(shipped)*, *decided in `DECISIONS.md` §28*:
   follows and background sync; on-demand fetch (`DAEMON.md` §9)
   waits for discovery. Slices, in order:
   1. ~~**Follows and the engine.**~~ *(shipped)* `kairo-federation-service` with
      `follows.json`, `kairo federation follow | unfollow`, and *sync
      once* over every listed peer, with per-follow results;
      `kairo federation sync` and `status` use it. `trust forget`
      drops matching follows.
   2. ~~**Inbox in the engine.**~~ *(shipped)* `kairo federation receive` moves into
      `kairo-federation-service` unchanged in behavior.
   3. ~~**The daemon drives it.**~~ *(shipped)* `[federation.sync]` config, interval
      and inbox timers with single-flight and a concurrency cap,
      `POST /api/v1/federation/sync`, `GET
      /api/v1/federation/status`, and an in-memory run record.
6. **Discovery**, in two steps; the first is *decided in
   `DECISIONS.md` §29*, with slices:
   1. ~~**Signed inventory.**~~ *(shipped)* `kairo.fed.inventory.v1` and its signing
      bytes; served at `GET /federation/v1/inventory`, signed when the
      published view is rebuilt.
   2. ~~**Hints.**~~ *(shipped)* `hints.json`, `[[federation.bootstrap]]` and
      `[federation.discovery]`, fetching and verifying direct
      inventories (`kairo federation discover`), `kairo federation
      holders`.
   3. **Relay.** `GET /federation/v1/inventories` with actor exports,
      one hop under `federation:index` trust.
   4. **Use.** The daemon fetches inventories on the schedule, and
      `use_hints` lets sync pull followed objects from trusted hinted
      holders.

   The two steps:
   - **Inventory announcements first.** A node signs and gossips
     which published objects it holds, timestamped, and peers
     record them as learned-peer hints (`DECISIONS.md` §18). That
     answers "who has object X?" with no index, as Radicle's
     inventory gossip does.
   - **Then** token advertisements, indexes, and the query protocol,
     once `PHASE_2.md` §7 / §8 statement types exist.

   Both need scoped trust (`DECISIONS.md` §18), and the second needs
   trust transport (item 4). Both need node authentication
   (`DECISIONS.md` §16 sub-decision 9), which push (item 3) brings.
7. **`git-remote-kairo`.** A Git remote helper so `git clone
   kairo://<node>/<object>` fetches over §22 and verifies every
   commit against the object's signed revisions, the way
   `git-remote-rad` makes Radicle usable with ordinary Git.

Also, as a review rather than a feature: wherever statement order
decides correctness rather than display, prefer explicit predecessor
references over signer-asserted `created_at`, as Radicle's
collaborative objects order operations by their parents. Branch
supersession already works this way; key-event ordering on ingest
and the revision log's heap order by `created_at` today.

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

Open question for push and discovery: whether statements should
also travel as Git objects. Radicle's per-peer ref namespaces and
signed refs are the closest prior art. `DECISIONS.md` §22 deferred it,
because statements form an independently appended set, not one
history, their ids are not Git ids, and moving them changes the
store backend.

Current assessment: keep statements on Kairo's own sync. Actor chains
(`DECISIONS.md` §23) and batch fetch (§4 items 1 and 2) bring the two
main gains a Git-driven statement sync would, withholding detection
and few round trips, and chain tips allow "everything after these
tips".
Git would add little beyond those:
- **It adds transport, not trust.** Git refs are unsigned, and Git
  commit signing knows nothing of Kairo keys, so Kairo-signed chains
  are needed either way.
- **Relayed views need their own namespaces.** A relay's merged view
  of an actor can't carry that actor's signature, which forces
  per-peer namespaces.
- **Publish filtering moves to per-actor-per-object refs.**
- **Per-record rejection becomes all-or-nothing per tree.**
- **The ids differ.** Git blob ids aren't statement ids.
- **`forget` fights append-only history.**
- **The `git` binary becomes mandatory** even for pulls with no Git
  content.

What Git would uniquely add is relaying through ordinary Git hosts
and Git bundles for offline transfer. §23 keeps that open: an actor
chain is a hash-linked DAG whose `previous` edges map onto Git commit
parents (one commit per statement, adding its file), so a Git
projection of statements could be generated later without a new
commitment format.

### 4.2 Comparison with Radicle

Radicle (Heartwood) is the closest prior art: peer-to-peer Git
replication with per-peer ref namespaces, signed refs, a
delegate-quorum identity document per repository, and collaborative
objects (issues, patches) stored as signed operation DAGs in Git.
Its goal is narrower: code collaboration where a Git repository is
the unit. Kairo's statements also cover builds, runs, capabilities,
and trust, and Git is one content backend.

Taken from Radicle (§4 items 1, 2, 3, 5, 6, 7):

- signed commitments to a whole set, so withholding is detectable;
- node identity proven by the node key on every exchange;
- simple seeding and follow policies as the operator surface;
- inventory gossip as the first discovery step;
- syncing in few round trips;
- plain Git workflows through a remote helper;
- causal ordering by predecessor references.

Kept deliberately different:

- **Identity lifecycle.** A Radicle identity is its key. Kairo actors
  separate genesis from operational keys and support rotation,
  attestation keys with a threshold, emergency revocation, and
  time-aware revocation (revoked-key statements stored and flagged).
- **Authority.** Kairo has capability grants scoped by object and
  statement kind, with revocation and a policy resolver, rather than
  delegates plus a threshold per repository.
- **Trust.** Kairo's `ActorTrust` is first-person and scoped, with no
  fallback between scopes and opt-in, depth-limited propagation,
  rather than binary follows.
- **Verification outcomes.** Kairo distinguishes `Valid`, `Invalid`,
  and `Indeterminate` (data missing), which is what makes partial
  federation safe to reason about.
- **Content independent of Git.** Object ids don't derive from Git,
  revisions may name SHA-1 or SHA-256 commits, and manifests and
  blobs are first-class.
- **Opt-in publishing.** Only a published object's closure is served,
  "missing" and "unpublished" look the same, and the §22 filter
  keeps the shared pool from leaking unpublished objects.
- **Deployment.** Plain HTTPS behind a reverse proxy and standard Git
  smart HTTP, rather than a dedicated peer-to-peer port and protocol.
  Radicle's network is more censorship-resistant; Kairo's is easier
  to run inside ordinary infrastructure.
- **Provenance.** Kairo records which peer every record came from.

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
  packing, a generation cap with `503 fed_busy`, and a stall timeout
  (§22 replaced the pack cache and the size-based deadline). The
  pulling side's pool lock during download is gone too (quarantined
  ingest).
- **The Git endpoint fronts a shared pool.** Protocol v2
  `upload-pack` serves any object it can read, and the pool holds
  unpublished objects' commits. `DECISIONS.md` §22's request filter
  is the only thing scoping it, so a change to that filter needs the
  pooled-secret tests in `tests/git.rs` to keep passing.
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
