# Kairo Federation Spec

This document describes the basic federation model for Kairo.

Federation is the part of Kairo that allows independent nodes to discover Objects, Providers, statements, build results, environment plans, and other archival knowledge without relying on a single central service.

Identifier examples in this document must follow `IDENTIFIERS.md`. External
federation references use the `kairo:` namespace, such as
`kairo:object:<id>`.

This document should be read alongside `PROJECT_LAYOUT.md`, especially the sections describing:

- `kairo-federation-core/`
- `kairo-federation-http/`
- `kairo-federation-dht/`
- `kairo-federation-service/`
- `kairo-statement/`
- `kairo-store/`
- `kairo-trust/`

## Status

The first implemented milestone is **object pull** between nodes,
locked in `DECISIONS.md` §16 and decomposed in
`PHASE_2_FEDERATION.md`. That decision overrides this document
where they differ:

- A node is an actor (`actor_kind = "node"`), referenced as
  `kairo:actor:<id>`. There is no separate node identifier.
- The MVP transfer surface is the `/federation/v1/` endpoint set
  in "HTTP Endpoints" below. The query protocol, token
  advertisements, token indexes, routing, gossip, and DHT sections
  describe the later discovery phase. They depend on statement
  types (`ProvidesCapability`, `ResolutionSucceeded`,
  `TokenAdvertisement`) that arrive with `PHASE_2.md` §7 and §8,
  and their examples are illustrative until then.
- Statements travel in the `kairo-statement` JSON wire shape
  (`type`, `version`, `actor`, `subject`, `created_at`, `body`,
  `signature`), not a separate federation envelope.
- Git history travels over Git's own smart HTTP protocol v2
  (`DECISIONS.md` §22), filtered to the commits of published
  revisions, so object content needs no Kairo packfile format.

---

## Goals

Kairo federation exists to answer questions such as:

```text
What Objects are known by this name?
Who provides environment:msdos/x86?
Has anyone successfully built this Object revision?
Which nodes hold this statement or blob?
Which Actors have made claims about this Object?
What version mappings are known for this Object?
What build/run failures have been observed?
```

Federation should support:

- decentralized discovery
- signed claims
- content-addressed transfer
- partial indexes
- trust-aware local policy
- efficient provider and resolution lookup
- reproducible build/run knowledge sharing
- interoperability between hosted archives, personal nodes, and institutional nodes

---

## Non-Goals

Federation is not responsible for deciding global truth.

Kairo federation does **not** require:

- global consensus
- a single canonical index
- all nodes agreeing on trust
- all nodes storing the same Objects
- all nodes accepting the same Providers
- all statements being trusted
- gossip being perfectly complete

Federation spreads information. Local trust policy decides what a node acts on.

---

## Core Principle

The federation exchanges **signed statements** and **content-addressed blobs**.

A node should not trust a remote database row merely because another node returned it.

Instead, a remote node provides:

```text
signed statements
statement hashes
blob hashes
holder hints
token index summaries
node advertisements
```

The receiving node verifies signatures, hashes, and local trust policy before using anything.

---

## Important Terms

### Node

A Kairo node is a network-visible participant in the federation.

A node may be:

- a personal workstation
- an institutional archive
- a public portal
- a CI/build node
- a federation index node
- a mirror
- a specialized Provider node

A node is identified by its node actor (`actor_kind = "node"`, see "Status") and may have one or more network endpoints.

### Actor

An Actor is a cryptographic identity that signs statements.

Actors are distinct from nodes.

A single Actor may operate from multiple nodes. A node may host or relay statements from many Actors.

### Statement

A statement is a signed fact.

Examples:

```text
VersionTag
ProvidesCapability
ResolutionSucceeded
ResolutionFailed
BuildSucceeded
BuildFailed
RunSucceeded
RunFailed
OwnershipClaim
Delegation
Revocation
TokenAdvertisement
```

Statements are identified by content hash.

### Blob

A blob is content addressed by hash.

Examples:

```text
Object packfile
Build Artifact metadata
Environment Plan
Resolution Record
Build log
Run log
Output archive
```

### Token

A token is a normalized routing key used for discovery.

Examples:

```text
provider:environment:msdos/x86
provider:environment:linux/x86_64
provider:tool:make
object:z6MkObject...
actor:z6MkActor...
resolution-success:object:z6MkObject...:revision:git:sha256:abc123
kairo:object:z6MkObject...
kairo:actor:z6MkActor...
kairo:object:z6MkObject...:revision:git:sha256:abc123
tag:gcc
namespace:gnu
collection:dos-games
```

Tokens are hashable and can be used with DHTs, delegated routing services, direct peer queries, or local indexes.

---

## Federation Layers

Kairo federation is divided into several layers.

```text
Application query layer
  FindProviders, FindStatements, FindSuccessfulResolutions

Token index layer
  token → statement hashes, holders, advertisers

Routing layer
  token → nodes likely to have token indexes

Transfer layer
  statement hash → statement
  blob hash → blob
  object id/revision → object content
```

The routing layer should stay dumb. It should not need to understand all Kairo semantics.

The application layer may perform rich filtering, ranking, and trust-aware interpretation.

---

## Node Discovery

A node SHOULD expose a well-known document:

```text
GET /.well-known/kairo-node
```

Example:

```json
{
  "schema": "kairo.fed.node.v1",
  "node_actor": "z6MkNode...",
  "actor_genesis": { "...": "the node actor's ActorGenesis JSON" },
  "base_url": "https://archive.example.org",
  "protocol_versions": ["kairo-fed/1"],
  "supports": ["object-pull"]
}
```

`node_actor` is the actor the node speaks as. Its keys and
rotation history come from that actor's ordinary key events, so
the document carries no key list of its own. Later phases add
`supports` values (`token-indexes`, `holder-discovery`,
`provider-discovery`) as those endpoints exist.

The well-known document is a discovery aid. It is not itself a trust root unless local policy says so. In the MVP it is unsigned, so `node_actor` is a claim. Node authentication arrives with push (`DECISIONS.md` §16 sub-decision 9).

---

## Transport Strategy

The initial transport SHOULD be HTTPS.

HTTPS is preferred for the first implementation because it is:

- easy to deploy
- easy to debug
- compatible with existing infrastructure
- cache-friendly
- firewall-friendly
- suitable for institutional archives

Future transports MAY include:

- libp2p
- DHT/delegated routing
- ActivityPub-like delivery
- static mirror indexes
- offline bundle exchange

The protocol should be designed so that transport can evolve without changing the statement model.

---

## HTTP Endpoints

The MVP node serves only published objects' closures
(`PHASE_2_FEDERATION.md` §2.1):

```text
GET  /.well-known/kairo-node
GET  /federation/v1/objects
GET  /federation/v1/objects/{objectId}
GET  /federation/v1/objects/{objectId}/genesis
GET  /federation/v1/objects/{objectId}/git/info/refs?service=git-upload-pack
POST /federation/v1/objects/{objectId}/git/git-upload-pack
GET  /federation/v1/actors/{actorId}
GET  /federation/v1/statements/{statementId}
POST /federation/v1/statement-batch
GET  /federation/v1/blobs/{blobId}
```

The discovery phase adds, under the same `/federation/v1/` prefix:

```text
POST /federation/v1/query
POST /federation/v1/announce
GET  /federation/v1/tokens/{encodedToken}
GET  /federation/v1/holders/{id}
GET  /federation/v1/advertisers/{encodedToken}
```

Path segments carry bare ids.

---

## Signed Statements

All semantically meaningful federation data SHOULD be represented as signed statements.

A statement envelope should include:

```json
{
  "type": "ProvidesCapability",
  "version": 1,
  "actor": "z6MkAlice...",
  "subject": "object:z6MkDosbox...",
  "created_at": "2026-04-29T12:00:00Z",
  "body": {
    "capability": "provider:environment:msdos/x86",
    "object": "z6MkDosbox...",
    "revision": "git:sha256:def456..."
  },
  "signature": {
    "actor": "z6MkAlice...",
    "key_id": "...",
    "algorithm": "ed25519",
    "bytes": "..."
  }
}
```

This is the `kairo-statement` JSON wire shape every statement
type uses; `ProvidesCapability` itself is a future type (§8). A
node serves a statement as exactly these bytes as stored.

Statement IDs are content hashes over the canonical signed representation.

Nodes MUST verify statement hashes and signatures before accepting statements into their local store.

Trust policy determines whether the statement may influence resolution, planning, build reuse, or execution.

---

## Token Advertisements

A token advertisement tells the federation that a node maintains knowledge relevant to a token.

Example token:

```text
provider:environment:msdos/x86
```

Example advertisement:

```json
{
  "type": "TokenAdvertisement",
  "version": 1,
  "actor": "z6MkNode...",
  "subject": "actor:z6MkNode...",
  "created_at": "2026-04-29T12:00:00Z",
  "body": {
    "token": "provider:environment:msdos/x86",
    "endpoint": "https://archive.example.org/federation/v1/tokens/provider%3Aenvironment%3Amsdos%2Fx86",
    "index_root": "z6MkIndexRoot...",
    "entry_count": 42,
    "watermark": "2026-04-29T12:00:00Z"
  },
  "signature": {
    "actor": "z6MkNode...",
    "key_id": "...",
    "algorithm": "ed25519",
    "bytes": "..."
  }
}
```

A token advertisement does not prove that the statements are true. It only claims that the node has indexed data relevant to the token.

---

## Token Indexes

A token index maps a token to statement/item hashes and holder hints.

Example:

```json
{
  "token": "provider:environment:msdos/x86",
  "index_root": "z6MkIndexRoot...",
  "entries": [
    {
      "statement": "kairo:statement:z6MkStmtA...",
      "statement_type": "ProvidesCapability",
      "subject": "kairo:object:z6MkDosbox...",
      "holders": [
        "kairo:actor:z6MkNodeA...",
        "kairo:actor:z6MkNodeB..."
      ],
      "advertisers": [
        "kairo:actor:z6MkNodeA..."
      ],
      "first_seen": "2026-04-20T12:00:00Z",
      "last_seen": "2026-04-29T12:00:00Z"
    }
  ],
  "more": null
}
```

Token indexes are partial observed ledgers.

Different nodes may have different token indexes for the same token. There is no required global agreement.

---

## Advertisers, Holders, and Indexers

The federation distinguishes between three roles.

### Advertiser

A node that claims relevance for a token.

Example:

```text
This node advertises provider:environment:msdos/x86.
```

### Holder

A node that can serve a specific statement or blob.

Example:

```text
This node can serve kairo:statement:z6MkStmtA....
```

### Indexer

A node that knows about statements or holders, but may not possess all referenced blobs.

Example:

```text
This node knows that kairo:statement:z6MkStmtA... exists and is held by node A.
```

A single node may be an advertiser, holder, and indexer, but the protocol should not require this.

---

## Query Protocol

The main query endpoint is:

```text
POST /federation/v1/query
```

A query request should include:

```json
{
  "query_type": "FindProviders",
  "query_version": 1,
  "body": {},
  "limit": 50,
  "cursor": null
}
```

A response should include:

```json
{
  "results": [],
  "statements": [],
  "holders": [],
  "blobs": [],
  "cursor": null,
  "warnings": []
}
```

Responses may include full statements or only statement hashes. Clients should be prepared to fetch missing statements by hash.

---

## Basic Query Types

### `FindStatements`

Find signed statements matching a structured predicate.

Example request:

```json
{
  "query_type": "FindStatements",
  "body": {
    "statement_type": "ProvidesCapability",
    "subject": "kairo:object:z6MkDosbox..."
  },
  "limit": 20
}
```

---

### `FindProviders`

Find Objects or Build Artifacts that provide a capability.

Example request:

```json
{
  "query_type": "FindProviders",
  "body": {
    "token": "provider:environment:msdos/x86",
    "constraints": {
      "requires_environment": "provider:environment:linux/x86_64"
    }
  },
  "limit": 20
}
```

Example response item:

```json
{
  "statement": "kairo:statement:z6MkStmtA...",
  "statement_type": "ProvidesCapability",
  "subject": "kairo:object:z6MkDosbox...",
  "holders": ["kairo:actor:z6MkNodeA..."]
}
```

---

### `FindSuccessfulResolutions`

Find known successful resolutions for an Object revision.

Example request:

```json
{
  "query_type": "FindSuccessfulResolutions",
  "body": {
    "object": "kairo:object:z6MkObject...",
    "revision": "git:sha256:abc123...",
    "native_environment": "provider:environment:linux/x86_64"
  },
  "limit": 20
}
```

Example response item:

```json
{
  "statement": "kairo:statement:z6MkStmtB...",
  "statement_type": "ResolutionSucceeded",
  "subject": "kairo:object:z6MkObject...:revision:git:sha256:abc123...",
  "resolution": "kairo:blob:z6MkResolution...",
  "environment_plan": "kairo:blob:z6MkPlan...",
  "build_artifact": "kairo:blob:z6MkBuild...",
  "holders": ["kairo:actor:z6MkNodeB..."]
}
```

---

### `FindStatementHolders`

Find nodes that can serve a statement.

Example request:

```json
{
  "query_type": "FindStatementHolders",
  "body": {
    "statement": "kairo:statement:z6MkStmtA..."
  }
}
```

---

### `FindBlobHolders`

Find nodes that can serve a content-addressed blob.

Example request:

```json
{
  "query_type": "FindBlobHolders",
  "body": {
    "blob": "kairo:blob:z6MkBlob..."
  }
}
```

---

### `FindAdvertisers`

Find nodes advertising a token.

Example request:

```json
{
  "query_type": "FindAdvertisers",
  "body": {
    "token": "provider:environment:msdos/x86"
  }
}
```

---

### `FindObjectsByName`

Find Objects associated with a human-readable name.

Example request:

```json
{
  "query_type": "FindObjectsByName",
  "body": {
    "name": "gcc"
  },
  "limit": 20
}
```

The response should return signed statements supporting the name association, not just raw database records.

---

## Provider Discovery Flow

Example goal:

```text
Find Providers for environment:msdos/x86.
```

Suggested flow:

```text
1. Check local token index for provider:environment:msdos/x86.
2. Query configured peers directly.
3. Query known federation index nodes.
4. Query routing layer/DHT for token advertisers.
5. Fetch token indexes from advertiser nodes.
6. Fetch missing statements by hash.
7. Verify statement hashes and signatures.
8. Apply local trust policy.
9. Rank Provider candidates.
```

The DHT or routing layer should answer:

```text
Who advertises this token?
```

The token index answers:

```text
Which statement hashes are indexed under this token?
```

The holder lookup answers:

```text
Who can serve this statement or blob?
```

---

## Successful Resolution Discovery Flow

Example goal:

```text
Find a known successful build plan for object:z6MkFoo...:revision:git:sha256:abc123.
```

Suggested tokens:

```text
object:z6MkFoo...
revision:object:z6MkFoo...:revision:git:sha256:abc123
resolution-success:object:z6MkFoo...:revision:git:sha256:abc123
provider:environment:linux/x86_64
```

Suggested flow:

```text
1. Check local ResolutionSucceeded statements.
2. Query trusted peers.
3. Query token index for resolution-success:object:z6MkFoo...:revision:git:sha256:abc123.
4. Fetch candidate ResolutionSucceeded statements.
5. Fetch referenced Resolution Record and Environment Plan.
6. Fetch referenced Build Artifact if desired.
7. Verify hashes and signatures.
8. Apply local trust policy.
9. Reuse, adapt, or ignore the plan.
```

Successful remote plans are advisory. A node may reject them due to:

- insufficient trust
- missing referenced Objects
- unsupported native runtime
- incompatible policy
- stale or revoked statements
- local security restrictions

---

## Token Routing

Tokens should be normalized before indexing or routing.

Example exact token:

```text
provider:environment:msdos/x86
```

A node MAY also publish broader tokens:

```text
provider
provider:environment
provider:environment:msdos
provider:environment:msdos/x86
```

This supports both broad discovery and exact matching.

For tools:

```text
provider:tool
provider:tool:make
provider:tool:make:semver:3
provider:tool:make:semver:3.81
```

Token hierarchy should be deterministic and documented per token kind.

---

## Token Index Synchronization

Token indexes SHOULD support incremental synchronization.

Example request:

```text
GET /federation/v1/tokens/provider%3Aenvironment%3Amsdos%2Fx86?since=z6MkOldRoot...
```

Example response:

```json
{
  "token": "provider:environment:msdos/x86",
  "previous_root": "z6MkOldRoot...",
  "current_root": "z6MkNewRoot...",
  "added": [
    {
      "statement": "kairo:statement:z6MkStmtA...",
      "holders": ["kairo:actor:z6MkNodeA..."]
    }
  ],
  "removed": [],
  "cursor": null
}
```

A node may refuse incremental sync and instead return a full page.

Index roots should be computed over a canonical representation of token index entries.

---

## Gossip and Pub/Sub

Gossip and pub/sub are optional acceleration layers.

They are useful for:

- new Provider announcements
- new successful builds
- revocations
- security warnings
- popular Object updates
- hot token updates

They should not be the only source of truth.

Durable discovery should still work through:

```text
token advertisements
token indexes
statement hashes
holder lookup
blob transfer
```

---

## DHT / Delegated Routing

A DHT, if used, should route from token to advertisements or nodes.

Recommended DHT value:

```text
token hash → signed TokenAdvertisement records
```

The DHT should not be required to return full Provider data, full statements, or trust decisions.

The client should still:

```text
1. discover advertisers through routing
2. fetch token indexes from advertisers
3. fetch statements by hash
4. verify signatures
5. apply trust policy
```

This keeps the DHT simple and limits spam damage.

---

## Object Transfer

Object source content may be transferred through Git-compatible mechanisms when appropriate.

Recommended options:

```text
Git over HTTPS
Git bundle blobs
content-addressed packfiles
static mirrors
```

Federation should distinguish Object content transfer from statement query.

Git is useful for Object revisions. It is not the primary statement query protocol.

---

## Blob Transfer

Content-addressed blobs should be fetchable by hash.

Example:

```text
GET /federation/v1/blobs/z6MkBlob...
```

The response should include:

```text
Content-Type
Content-Length
Digest
```

Clients MUST verify the received blob hash by recomputing the
Blob ID from the bytes; the `Digest` header is a hint, never the
check.

Blobs may include:

- Environment Plans
- Resolution Records
- Build Artifact metadata
- build logs
- run logs
- output archives
- Object packfiles

---

## Trust Boundary

Federation does not determine trust.

A node may receive many statements but only use some of them.

The local trust layer decides:

```text
which Actors are trusted
which statement types they are trusted for
which capability scopes they are trusted for
whether delegation is accepted
whether threshold agreement is required
whether revocations apply
whether remote build artifacts may be reused
```

A node SHOULD preserve enough provenance to explain why a result was accepted or rejected.

Example:

```text
Accepted because:
  ResolutionSucceeded signed by kairo:actor:alice
  alice is trusted for provider:environment:msdos/x86
  referenced Environment Plan hash verified
  referenced Build Artifact hash verified
```

---

## Partial Ledgers

Nodes should not attempt to maintain a canonical global ledger for a token.

Each node maintains a partial observed ledger:

```text
node A's view of provider:environment:msdos/x86
node B's view of provider:environment:msdos/x86
node C's merged view of provider:environment:msdos/x86
```

Queries may return overlapping, stale, or conflicting information.

Clients should deduplicate by statement hash and then apply trust policy.

---

## Conflict Handling

Conflicting statements are expected.

Examples:

```text
Two Actors assign different version tags.
Two Providers claim the same capability.
One Actor says a build succeeded; another says it failed.
A revocation targets a previously accepted statement.
```

The federation layer should store and expose conflicts. It should not erase them.

The resolver, planner, and trust layer decide what to use.

---

## Pagination and Limits

All query endpoints should support limits and cursors.

Nodes MAY impose:

- maximum page sizes
- rate limits
- authentication requirements
- proof-of-work or cost controls in hostile environments
- token-specific query restrictions

Example:

```json
{
  "query_type": "FindProviders",
  "body": {
    "token": "provider:tool:make"
  },
  "limit": 50,
  "cursor": "opaque-cursor"
}
```

---

## Privacy and Access Control

Not every statement or Object needs to be public.

Nodes may support:

- private Objects
- restricted statements
- authenticated federation peers
- organization-only indexes
- public metadata with private blobs
- private build logs

Federation responses should make absence ambiguous where privacy matters. A node may simply return no results rather than disclose restricted content.

---

## Security Considerations

Federation nodes must assume remote data may be malicious.

Required protections:

- verify all hashes
- verify all signatures
- validate all schemas
- apply size limits
- apply recursion limits
- avoid automatic execution of untrusted plans
- sandbox build/run operations
- treat remote indexes as hints
- deduplicate by hash
- rate-limit remote peers
- preserve provenance

Remote Environment Plans must not be executed merely because they were discovered. They must pass local trust and policy checks.

---

## Minimal Viable Federation

`DECISIONS.md` §16 narrows the first milestone to items 1, 3, 4,
and 9 below, applied to object pull. Items 2 and 5–8 are the
discovery phase.

The full minimal federation should include:

```text
1. HTTPS node discovery
2. Direct peer query
3. Signed statement exchange
4. Content-addressed blob fetch
5. Token advertisements
6. Token index fetch
7. Provider discovery
8. Successful resolution discovery
9. Local trust filtering
```

The first implementation does not need:

```text
full DHT
mandatory gossip
global indexes
complex reputation
automatic remote execution
```

---

## Suggested Implementation Order

Steps 1 and 3 are done (`kairo-statement`, `kairo-store`).
`PHASE_2_FEDERATION.md` covers step 2 for transfer types, plus
transfer endpoints that precede step 4. Steps 5 onward wait for
the discovery phase.

1. Define statement envelope and canonical hashing.
2. Define federation query/response types in `kairo-federation-core`.
3. Implement local statement store in `kairo-store`.
4. Implement HTTP query endpoint in `kairo-federation-http`.
5. Implement token advertisement statements.
6. Implement local token indexes.
7. Implement token index fetch and sync.
8. Implement provider discovery queries.
9. Implement successful resolution discovery queries.
10. Add holder discovery.
11. Add trust-aware filtering.
12. Add optional delegated routing or DHT support.
13. Add optional gossip/pub-sub acceleration.

---

## Example End-to-End Scenario

A user runs:

```sh
kairo providers provider:environment:msdos/x86
```

The local daemon asks the federation service.

The federation service:

```text
1. checks the local token index
2. queries configured peers
3. asks index nodes for advertisers
4. optionally queries routing/DHT for token advertisements
5. fetches token indexes from advertiser nodes
6. fetches missing ProvidesCapability statements
7. verifies statement signatures
8. applies local trust policy
9. returns ranked Provider candidates
```

The CLI displays:

```text
Provider candidates for provider:environment:msdos/x86:

1. DOSBox-X
   Object: kairo:object:z6MkDosbox...
   Revision: git:sha256:def456...
   Signed by: kairo:actor:z6MkDosboxMaintainer...
   Trust: accepted
   Known successful runs: 128

2. PCem
   Object: kairo:object:z6MkPcem...
   Revision: git:sha256:abc999...
   Signed by: kairo:actor:z6MkRetroCurator...
   Trust: manual review required
   Known successful runs: 12
```

---

## Summary

Kairo federation is a signed-statement discovery and transfer system.

The network does not provide truth. It provides evidence.

The durable federation model is:

```text
token → advertisers/indexes → statement hashes → signed statements → content-addressed blobs
```

Local policy turns that evidence into action.
