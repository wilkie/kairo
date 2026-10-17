# PHASE_2_FEDERATION.md

## Status

Draft, locked 2026-10-16. No slices shipped yet.

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
| `crates/kairo-federation-core` (new) | Wire types: `NodeDocument`, `ObjectListing`, `ActorListing`, `PublishedObjects`, `Page<T>` / cursor, `FedErrorCode`. Serde only; depends on `kairo-core` and `kairo-identity`. No I/O. |
| `crates/kairo-federation-http` (new) | `server` (closure computation, axum router), `client` (fetch + verify + ingest), `publish` (the `published.json` list). Depends on `kairo-federation-core`, `kairo-store`, `kairo-statement`. `kairo-federation-http` binary. |
| `crates/kairo-store` | `provenance` module: `ProvenanceStore` trait + `FilesystemStore` impl over `<store>/provenance/` (slice 3). Lives here, not in the federation crate, so the daemon can read it (slice 5) without depending on federation. |
| `crates/kairo-cli` | `kairo fetch <object-ref> --from <url>`; `kairo federation publish \| unpublish \| published \| provenance \| status` |
| `crates/kairo-daemon` | Slice 5 only: provenance on read DTOs |
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
| `GET` | `/federation/v1/objects/{id}/git` | Git pack bytes (slice 4) |
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

`statements` holds the object-bound kinds; `authority` holds
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
| 405 | `fed_method_not_allowed` | anything but `GET` / `HEAD` |
| 500 | `fed_store_error` | store read failed (message is generic; details logged) |

Envelope is `API.md` §7.2.

### 2.4 Client limits

Defaults, overridable per invocation and later in `[federation]`
config: 1 MiB per statement or listing page, 256 MiB per blob,
1 GiB per Git pack, 100 000 records per pull, 30 s per request.
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

### Slice 1 — Wire types and publish list

- `kairo-federation-core` with the §2.2 types, round-trip serde
  tests, and `deny_unknown_fields` on every top-level type.
- `publish` module: read/modify/write
  `<store>/federation/published.json` (sorted object ids) under
  the store's index lock.
- `kairo federation publish <object-ref>` (object must exist
  locally), `unpublish` (prints "peers that already fetched it
  keep their copy"), `published`.

### Slice 2 — Serving

- Closure computation: given an object id, walk the store's
  object and capability indexes for the §16 sub-decision 4 set.
  Unit-tested on its own, including: `ActorTrust` never appears;
  grants scoped to *other* objects don't appear; a grantee who
  signed a branch appears with key events.
- axum router for §2.1 (except `/git`), `kairo-federation-http`
  binary with `--store`, `--bind` (default `127.0.0.1:7880`),
  `--base-url`, `--node-actor`. Non-loopback bind logs a warning
  naming the reverse-proxy expectation.
- `--node-actor` must name an actor whose genesis kind is `node`
  (refuse otherwise) and must not hold any capability grant
  (warn).
- Tests: every endpoint; absence ambiguity (an unpublished but
  present statement returns the same body as a nonexistent one);
  served statement bytes equal the stored file.

### Slice 3 — Pull with verification and provenance

- `client` module: well-known → listing (all pages) → actors and
  key events → genesis → statements and authority → blobs, each
  fetched only if absent, each fixity- and signature-checked, each
  stored via `put_*`.
- `kairo-store` `provenance` module: shard-path writes per §16
  sub-decision 8, appended under the store's index lock;
  `kairo federation provenance <id>` prints it.
- `kairo fetch <object-ref> --from <url>` (`CLI.md` §15) reporting
  fetched / already-held / rejected counts.
- Tests: two-store round trip, re-pull is a no-op that appends
  no duplicate provenance, second peer appends a provenance entry,
  and every adversarial case from §2.5 rejects without storing the
  bad record. `rebuild-indexes` leaves `provenance/` untouched.

### Slice 4 — Git pack transfer

- `GET /federation/v1/objects/{id}/git`: the same pack `kairo
  bundle export --include-git` produces from the managed cache.
- Client ingests through the managed-cache path bundle import
  uses, then checks every pulled `ObjectRevision`'s commit resolves.
- Test: after a pull, `kairo verify object` on the second store
  resolves revisions without a cwd repo (`--no-cwd-repo`).

### Slice 5 — Policy and visibility

- `[federation]` config: peer allow list by URL (pull refuses
  others unless `--allow-peer`), pinned `node_actor` per URL
  (pull fails if the document's claim changes), the §2.4 limits.
- `kairo federation status`: publish list, configured peers, last
  pull per peer.
- Daemon read DTOs gain optional `provenance`; the inspector
  shows a "Fetched from <peer_url>" badge on statements that have
  it.

### Slice 6 — Two-node real-stack check

- `scripts/federation-smoke.sh`: two stores, publish, serve,
  pull, verify. Runs in CI next to `e2e-real`.
- One `e2e-real` Playwright spec: inspector over the pulled store
  shows the object and the provenance badge.

---

## 4. After This Plan

In rough order, each needing its own decision:

1. **Push** with node authentication (signed node document,
   `KairoActor1` on requests) and a receiver acceptance policy.
2. **Trust transport** (opt-in, never auto-trusted) and `forget`.
3. **Daemon `FederationService`**: background sync, on-demand
   fetch of missing closure data (`DAEMON.md` §9).
4. **Discovery** — token advertisements, indexes, query protocol
   — once `PHASE_2.md` §7 / §8 statement types exist.

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
  not verify signatures; once slice 3 lands, consider routing
  bundle import through the same verifier.

---

End of PHASE_2_FEDERATION.md
