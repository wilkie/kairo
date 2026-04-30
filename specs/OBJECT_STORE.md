# Kairo Object Store Spec

This document describes Kairo Object identity, Object revision storage, signed provenance, clone behavior, and local Object store layout.

It should be read alongside:

- `PROJECT_LAYOUT.md`
- `FEDERATION.md`

The Object store is responsible for preserving software artifact contents and the signed facts needed to verify their identity, provenance, ownership, revisions, and history.

---

## Goals

The Object store should provide:

- globally unique Object identifiers
- stable Object identity across revisions
- verifiable Object creation
- ownership and delegation that can change over time
- efficient revision tracking
- Git-compatible source history storage
- support for legacy Git repositories
- hash agility for Kairo-native content
- efficient clone and deduplication behavior
- append-only signed provenance logs
- shallow verification proofs when a full log is unnecessary

---

## Non-Goals

The Object store does not decide global truth.

It does not require:

- global consensus
- globally unique human-readable names
- Git branches to be trusted
- every Git commit to have a Kairo statement
- every clone to fetch every known statement immediately
- every node to agree on current ownership or trust

The Object store stores content and signed evidence. Local trust policy determines what to accept and act on.

---

## Core Model

Kairo separates Object identity, revision content, ownership, and provenance.

```text
Object identity
  immutable, derived from genesis statement

Revisions
  content-addressed Git commits

Ownership
  append-only signed statements

Version names / refs
  append-only signed statements

Build/run/provenance knowledge
  append-only signed statements
```

The central invariant is:

```text
Object identity is immutable.
Ownership is mutable but append-only.
Revisions are content-addressed.
```

---

## Object Identifiers

A Kairo Object identifier is derived from or cryptographically bound to the
Object's genesis statement.

```text
ObjectId payload = z<base58btc(multihash_sha2_256(canonical(ObjectGenesis)))>
Object reference = object:<id>
External Object reference = kairo:object:<id>
```

Example:

```text
zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk
object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk
kairo:object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk
```

The Object ID is not a Git commit hash and is not derived from a mutable name.

This gives Kairo:

- stable identity
- verifiable creation
- rename support
- ownership-transfer support
- fork support
- independence from revision changes

---

## Object Genesis Statement

Every Object begins with an `ObjectGenesis` statement.

The genesis statement is immutable and defines the Object's identity.

Example:

```json
{
  "statementType": "ObjectGenesis",
  "statementVersion": 1,
  "objectKind": "software",
  "createdBy": "kairo:actor:z6Mka...",
  "createdAt": "2026-04-29T12:00:00Z",
  "nonce": "base64-random-256-bit-value",
  "initialRevision": "git:sha256:abc123...",
  "signature": {
    "algorithm": "ed25519",
    "keyId": "...",
    "value": "..."
  }
}
```

The genesis statement SHOULD be minimal.

It SHOULD contain:

- statement type and version
- Object kind
- creating Actor
- creation timestamp
- random nonce
- optional initial revision
- signature

It SHOULD NOT contain mutable descriptive fields such as:

- display name
- description
- tags
- categories
- current owner
- version names

Those belong in separate signed statements.

---

## Domain-Separated Hashing

Object ID hashing SHOULD use a domain separator.

Conceptually:

```text
ObjectId = multibase_base58btc(multihash_sha2_256("kairo.object.genesis.v1" || canonical_genesis_bytes))
```

The exact canonicalization and hashing rules must be specified by the statement layer.

Domain separation prevents the same canonical bytes from accidentally being interpreted as another Kairo content type.

---

## Hash Format

Kairo-native stable identifiers MUST use SHA-256 multihash payloads encoded with
multibase base58btc as defined by `IDENTIFIERS.md`.

Typed fields store bare payloads. Standalone references use the typed reference
forms defined by `IDENTIFIERS.md`.

Examples:

```text
object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk
statement:zQmaFrbQVbbdb1HSDQFdPjhtB4XPZMhmyazswWUp57qpwr9
blob:zQmfBE2w2UqKJhGZAxK4ZWb4JuCrvxnN9P4YKuvzfbPSvD5
kairo:object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk
kairo:statement:zQmaFrbQVbbdb1HSDQFdPjhtB4XPZMhmyazswWUp57qpwr9
```

Other multihash algorithms are not valid for canonical Kairo IDs in v1.

Git revisions SHOULD use explicit Git-flavored identifiers:

```text
git:sha1:<commit>
git:sha256:<commit>
```

Kairo MUST NOT infer hash algorithm from digest length.

A Git SHA-256 object ID is not the same thing as a generic SHA-256 digest of arbitrary bytes. Git object IDs hash Git's own object serialization.

---

## Revisions

Object revisions are identified by Git commit hashes.

Examples:

```text
object:z6Mk...:revision:git:sha256:abc123...
object:z6Mk...:revision:git:sha1:def456...
kairo:object:z6Mk...:revision:git:sha256:abc123...
```

New Kairo-managed repositories SHOULD prefer Git SHA-256.

Legacy Git repositories MAY preserve Git SHA-1 object IDs when imported.

Kairo should support archived legacy repositories without rewriting their history by default.

---

## Object Repository Format

A Kairo Object repository is Git-compatible and contains the artifact contents.

At minimum, a Kairo Object source tree SHOULD contain:

```text
kairo.toml
```

Example repository:

```text
kairo.toml
src/
README.md
build.sh
```

Example `kairo.toml`:

```toml
[kairo]
schema = 1
kind = "software"
name = "gnu-make"
summary = "GNU Make"

[provides.tool.make]
version = "3.81"

[requires.environment]
os = "linux"
arch = "x86"

[build]
commands = ["./configure", "make"]

[run]
commands = ["./make"]
```

The `kairo.toml` file describes declared Object metadata at that revision.

The Object ID, ownership, version mappings, revision attestations, build results, and federation advertisements are represented as signed statements outside the Git tree.

---

## Object ID Declaration in Manifest

The `kairo.toml` manifest SHOULD declare the Object ID once the Object has been initialized.

Example:

```toml
[kairo]
schema = 1
object = "z6Mk..."
kind = "software"
name = "gnu-make"
```

This declaration is a consistency check, not the source of authority.

The authoritative Object identity comes from the `ObjectGenesis` statement.

A malicious repository may include a false Object ID in `kairo.toml`. A node MUST only accept a revision for an Object if a trusted signed statement binds that Object to the revision.

---

## Signed Revision Binding

A Git commit is accepted as a revision of an Object only when an authorized Actor signs a statement binding the Object to that commit.

Example `ObjectRevision` statement:

```json
{
  "statementType": "ObjectRevision",
  "statementVersion": 1,
  "subject": "kairo:object:z6Mk...",
  "actor": "kairo:actor:z6MkAlice...",
  "issuedAt": "2026-04-29T12:00:00Z",
  "body": {
    "revision": "git:sha256:abc123...",
    "parents": ["git:sha256:def456..."],
    "manifestHash": "kairo:blob:z6MkManifest...",
    "message": "Import GNU Make 3.81 source",
    "attestsReachableHistory": true
  },
  "signature": {
    "algorithm": "ed25519",
    "keyId": "...",
    "value": "..."
  }
}
```

The statement means:

```text
The signing Actor claims this Git commit is a revision of this Kairo Object.
```

Whether that claim is accepted depends on local trust policy and Object ownership/delegation statements.

---

## Git Signatures

Git's built-in commit and tag signatures MAY be preserved and MAY be recorded as evidence.

They are not the canonical Kairo authority.

Kairo's rule is:

```text
Git signatures are evidence.
Kairo statements are authority.
```

A Git signature proves that a Git identity signed a commit or tag.

A Kairo `ObjectRevision` statement proves that a Kairo Actor claims a commit belongs to a particular Kairo Object.

Kairo MAY record Git signature observations in signed statements, but validation of Object provenance should rely on Kairo statements.

---

## Reachable History Attestation

Kairo does not require a signed statement for every Git commit.

A trusted signed update to a recent revision can act as an attestation over the reachable Git history behind that commit.

```text
Authorized Actor signs Object O → commit C
Git proves C → parent → parent → earlier history
Therefore the Actor attests to the reachable history behind C
```

This is the default efficient validation model.

It does not prove that the Actor originally authored every historical commit.

It proves that the Actor endorsed the reachable history as belonging to the Object.

For large imports, Kairo MAY use an `ObjectHistoryImport` statement.

Example:

```json
{
  "statementType": "ObjectHistoryImport",
  "statementVersion": 1,
  "subject": "kairo:object:z6Mk...",
  "actor": "kairo:actor:curator",
  "issuedAt": "2026-04-29T12:00:00Z",
  "body": {
    "head": "git:sha256:abc123...",
    "historyMode": "reachable-from-head",
    "sourceDescription": "Imported from upstream Git repository",
    "commitCount": 12453,
    "commitSetRoot": "kairo:blob:z6MkCommitSetRoot..."
  },
  "signature": {
    "algorithm": "ed25519",
    "keyId": "...",
    "value": "..."
  }
}
```

---

## Branches, Refs, and Version Tags

Remote Git branches are transport hints. They are not trusted Kairo authority.

Kairo refs and versions are signed statements.

Example `ObjectRef`:

```json
{
  "statementType": "ObjectRef",
  "statementVersion": 1,
  "subject": "kairo:object:z6Mk...",
  "actor": "kairo:actor:maintainer",
  "body": {
    "name": "main",
    "revision": "git:sha256:abc123..."
  },
  "signature": { "algorithm": "ed25519", "keyId": "...", "value": "..." }
}
```

Example `VersionTag`:

```json
{
  "statementType": "VersionTag",
  "statementVersion": 1,
  "subject": "kairo:object:z6Mk...",
  "actor": "kairo:actor:maintainer",
  "body": {
    "name": "4.1.2",
    "revision": "git:sha256:abc123...",
    "tagKind": "release"
  },
  "signature": { "algorithm": "ed25519", "keyId": "...", "value": "..." }
}
```

Kairo version statements avoid the self-reference problem of committing version metadata into the source tree.

---

## Ownership and Delegation

Object ownership is represented by append-only signed statements.

Ownership is not encoded into the Object ID.

This allows Object stewardship to change without changing Object identity.

Relevant statement types include:

```text
OwnershipClaim
OwnershipTransfer
Delegation
Revocation
```

Example ownership chain:

```text
ObjectGenesis
  createdBy Alice

OwnershipClaim
  Alice claims initial ownership

Delegation
  Alice delegates release signing to Bob

OwnershipTransfer
  Alice transfers ownership to Carol

Revocation
  Carol revokes Bob's release-signing authority
```

Local trust policy determines how these statements are interpreted.

---

## Validation Model

A node validating a revision should verify:

```text
1. ObjectGenesis statement exists.
2. ObjectId equals `z<base58btc(multihash_sha2_256(canonical ObjectGenesis))>`.
3. ObjectGenesis signature is valid.
4. Ownership/delegation/revocation statements establish authorized Actors.
5. ObjectRevision/ObjectRef/VersionTag statement binds ObjectId to revision.
6. Binding statement is signed by an authorized Actor.
7. Git commit hash verifies.
8. Git tree and blobs verify under Git's object model.
9. kairo.toml, if present, declares the expected ObjectId.
10. Local trust policy accepts the result.
```

The repository alone does not prove ownership.

The signed statement log authorizes the revision. Git verifies the bytes.

---

## Statement Logs

Each Object has an append-only observed statement log.

Important statement categories include:

```text
ObjectGenesis
OwnershipClaim
OwnershipTransfer
Delegation
Revocation
ObjectRevision
ObjectRef
VersionTag
ObjectHistoryImport
ObjectName
ObjectDescription
CurationNote
ProvidesCapability
ResolutionSucceeded
ResolutionFailed
BuildSucceeded
BuildFailed
RunSucceeded
RunFailed
```

Nodes SHOULD preserve the full Object statement log when operating as archival mirrors.

Nodes MAY perform shallow fetches when only a specific proof is needed.

The canonical log should not be truncated.

---

## Full Clone

A full Object clone should fetch:

```text
1. ObjectGenesis statement
2. Object-relevant signed statement log
3. Git content needed for known revisions
4. Kairo manifests for fetched revisions
5. Associated blobs referenced by required statements
```

A full archival mirror may also fetch:

```text
all known Object revisions
all known version/ref statements
all known build and run statements
all known resolution and environment-plan statements
all associated Build Artifacts and logs
```

---

## Shallow Clone and Trust Closure

Validation and trust verification do not always require the full statement log.

A node MAY request a minimal trust closure for a target operation.

Example target:

```text
Validate kairo:object:z6Mk...:revision:git:sha256:abc123...
```

The required closure should include:

```text
ObjectGenesis
the statement binding the Object to the target revision
ownership/delegation chain authorizing the signer
revocations affecting those Actors or statements
superseding ownership transfers that affect interpretation
policy-relevant warnings or tombstones
an explicit frontier marker
```

A shallow proof response must not merely be "some relevant statements."

It should be:

```text
a complete proof relative to a declared statement-log frontier
```

Example response shape:

```json
{
  "object": "kairo:object:z6Mk...",
  "target": "git:sha256:abc123...",
  "proofKind": "revision-trust-closure",
  "statements": [
    "kairo:statement:sha256:genesis...",
    "kairo:statement:sha256:ownership...",
    "kairo:statement:sha256:delegation...",
    "kairo:statement:sha256:revision..."
  ],
  "frontier": {
    "objectLogRoot": "kairo:blob:z6MkLogRoot...",
    "asOf": "2026-04-29T12:00:00Z"
  }
}
```

A frontier is not a global guarantee forever. It says the proof is complete relative to a specific observed log root.

Other peers may have newer or conflicting statements.

---

## Log Checkpoints

Nodes MAY publish checkpoints for Object statement logs.

A checkpoint summarizes derived state at a particular log root.

Example checkpoint contents:

```text
current accepted owners
current delegated maintainers
known refs
known versions
known revoked statements
latest accepted heads
```

Checkpoints are optimization aids. They do not replace the underlying statement log.

A node should be able to replay the signed statements to verify or reconstruct derived state.

---

## Forks

A fork is a new Object.

Two Objects may share Git history or even the same initial revision, but they have different genesis statements and therefore different Object IDs.

Example:

```text
Original:
  kairo:object:AAA...

Fork:
  kairo:object:BBB...

Both may reference:
  git:sha256:abc123...
```

Fork lineage, equivalence, and upstream/downstream relationships should be represented as signed statements, not encoded into Object IDs.

Possible future statement types:

```text
ForkedFrom
DerivedFrom
EquivalentProject
CanonicalUpstreamClaim
```

---

## Names and Search

Human-readable names are not globally unique and are not Object identity.

Names should be represented as signed statements and indexed for search.

Example:

```json
{
  "statementType": "ObjectName",
  "subject": "kairo:object:z6Mk...",
  "actor": "kairo:actor:curator",
  "body": {
    "name": "GNU Make",
    "slug": "gnu-make"
  },
  "signature": { "algorithm": "ed25519", "keyId": "...", "value": "..." }
}
```

Multiple Objects may share the same name. Multiple names may refer to the same Object.

Search results should include supporting statements and trust/provenance information.

---

## Local Store Layout

A local Kairo node should store Object content, statements, blobs, and derived indexes separately.

Suggested layout:

```text
~/.kairo/
  objects/
    by-id/
      z6MkObject.../
        object.json
        refs/
        worktrees/
        checkpoints/
    git/
      object-pool/
        objects/
        packs/
  statements/
    by-hash/
      sha256/
        ab/
          cd...
  blobs/
    by-hash/
      sha256/
        ab/
          cd...
  indexes/
    objects.sqlite
    statements.sqlite
    tokens.sqlite
  trust/
    policy.toml
  federation/
    peers.toml
    token-indexes/
```

This layout is illustrative. Implementations may choose a different physical layout while preserving the same logical model.

---

## Git Object Deduplication

The Object store should deduplicate Git content where practical.

Possible strategies:

```text
shared bare Git object pool
Git alternates
packfile reuse
partial clone
promisor remotes
Git bundles
```

Kairo should avoid storing a complete independent Git repository for every Object when histories or blobs overlap.

For initial implementations, a per-Object bare repo is acceptable if the logical model allows migration to a shared object pool.

---

## Blob Store

Non-source artifacts should use a Kairo content-addressed blob store rather than being forced into Git.

Examples:

```text
Environment Plans
Resolution Records
Build Artifact metadata
build logs
run logs
output archives
VM manifests
statement-log checkpoints
large generated files
```

Blob identifiers should use Kairo-native SHA-256 multihash IDs:

```text
kairo:blob:z<base58btc(multihash_sha2_256(blob bytes))>
```

Clients MUST verify blob hashes after transfer.

---

## Transfer Protocol

Object transfer should separate discovery from content transfer.

Discovery queries return signed statements and holder hints.

Content transfer fetches Git objects, statements, and blobs by identifier.

Suggested endpoints:

```text
GET /objects/{objectId}/refs
GET /objects/{objectId}/git/bundle?revision={revision}
GET /objects/{objectId}/git/pack?want={revision}
GET /statements/{statementHash}
GET /blobs/{blobHash}
POST /federation/query
```

A simple first implementation may use Git bundles over HTTPS.

A later implementation may support Git smart HTTP, partial clone, or packfile negotiation.

---

## Clone Flow

A typical clone of a specific revision:

```text
1. Resolve ObjectId or name through federation/local search.
2. Fetch ObjectGenesis statement.
3. Verify ObjectId from genesis.
4. Fetch trust closure or full Object statement log.
5. Verify ownership/delegation/revision statements.
6. Fetch Git content for requested revision.
7. Verify Git commit hash.
8. Read and validate kairo.toml.
9. Store statements and Git content locally.
10. Update local indexes.
```

---

## Pull Flow

A later pull should fetch:

```text
new statements since known log frontier
new refs/version mappings
new accepted revision bindings
new Git content for desired revisions
new associated blobs as requested
```

The node should preserve old statements even if superseded or revoked.

Revocation changes interpretation. It should not delete history.

---

## Object Creation Flow

Creating a new Object:

```text
1. Create or select Actor identity.
2. Create initial Git repository or source tree.
3. Add kairo.toml.
4. Commit initial revision.
5. Create ObjectGenesis statement.
6. Compute ObjectId from ObjectGenesis.
7. Update kairo.toml to include ObjectId if desired.
8. Commit ObjectId declaration revision if needed.
9. Create ObjectRevision statement for accepted initial/current revision.
10. Optionally create ObjectName and OwnershipClaim statements.
11. Announce relevant statements to federation.
```

There is a subtle bootstrapping issue if the initial revision wants to include the ObjectId in `kairo.toml`.

Recommended approach:

```text
Genesis may reference an initial pre-ObjectId revision.
A follow-up ObjectRevision records the first revision whose kairo.toml declares the ObjectId.
```

This avoids a circular hash dependency.

---

## Validation Against Malicious Repositories

A malicious node may provide:

```text
a repo with false branches
a repo with extra commits
a repo whose kairo.toml claims the wrong ObjectId
a repo missing relevant history
a repo with valid Git objects but no trusted Kairo revision statement
```

Kairo should reject or quarantine such content unless trusted signed statements bind the Object to the requested revision.

The repository itself is never the authority.

The authority is:

```text
trusted signed statement → Git commit hash → verified Git object graph
```

---

## Relationship to Federation

Federation exchanges:

```text
ObjectGenesis statements
ObjectRevision statements
ObjectRef statements
VersionTag statements
Ownership/delegation/revocation statements
holder records
Object content transfer hints
```

The federation may help discover which nodes hold an Object or revision, but the receiving node verifies all hashes and signatures locally.

Useful federation tokens include:

```text
object:z6Mk...
revision:object:z6Mk...:revision:git:sha256:abc123...
actor:z6MkActor...
kairo:object:z6Mk...
kairo:object:z6Mk...:revision:git:sha256:abc123...
kairo:actor:z6MkActor...
name:gnu-make
provider:tool:make
provider:environment:msdos/x86
```

---

## Minimal Viable Object Store

The first implementation should support:

```text
1. ObjectGenesis statements
2. ObjectId payload = z<base58btc(multihash_sha2_256(canonical ObjectGenesis))>
3. Git SHA-256 revisions for new Objects
4. Git SHA-1 revisions for legacy imports
5. ObjectRevision statements
6. VersionTag statements
7. Ownership/delegation/revocation statements
8. Per-Object statement storage
9. Git bundle transfer
10. Basic trust-closure fetch
11. Local SQLite indexes
```

It does not need initially:

```text
shared global Git object pool
full DHT integration
per-commit Kairo statements
advanced checkpoint proofs
automatic equivalence detection
```

---

## Summary

Kairo Objects use immutable genesis-based identity and content-addressed revisions.

The fundamental model is:

```text
ObjectId payload = z<base58btc(multihash_sha2_256(canonical ObjectGenesis))>
Object reference = object:<id>
External Object reference = kairo:object:<id>
Revision = git:<hash-algorithm>:<commit>
Object version/ref = signed statement binding ObjectId to Revision
Ownership = append-only signed statements
Repository content = verified by Git object hashes
```

The repository proves file integrity.

The statement log proves Object provenance and authorization.

The local trust policy decides which signed claims are accepted.
