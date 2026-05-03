# OBJECT.md

## 1. Overview

An **Object** is the fundamental unit of content in the Kairo archive system.

An Object:

- is **immutable per version**
- contains **versioned data and metadata**
- may reference **external content-addressed blobs**
- may declare **capabilities** it provides
- may define **run interfaces**
- may define **build interfaces** (see `BUILD.md`)

An Object may represent:

- source code
- compiled binaries
- datasets
- disk images
- tools, libraries, or applications
- environments or emulators

Objects have **stable lineage identity** and are **versioned over time**.
Specific Object states are identified by Snapshot IDs.

---

## 2. Object Identity

An Object has four related identity layers:

- object id - stable identity of the object lineage
- revision id - storage-level content revision, such as a Git commit
- snapshot id - deterministic identity of a selected Object state
- blob id - content-addressed identity for external bytes

### 2.1 Object ID

The Object ID identifies the logical Object lineage. It is not a content hash of
the current source tree and does not change when a new revision is added.

The Object ID is established by Object creation or an equivalent root statement
defined by `STATEMENTS.md` and `OBJECT_STORE.md`.

### 2.2 Revision ID

A revision ID identifies the stored content revision. For Git-backed Objects this
is a Git object ID such as `git:sha256:<commit>`.

Git revisions verify bytes and history under Git's object model. They do not
establish Kairo authority unless signed Kairo statements bind them to an Object.

### 2.3 Snapshot ID

A Snapshot ID uniquely identifies the **effective Kairo state** of an Object for
a selected statement frontier.

#### Frontier

A **frontier** is the set of currently-selected `StatementId`s whose canonical
contributions, taken together, define the Object's effective state. A
frontier is *selected*, not derived: it is named directly as a finite set of
statement IDs rather than computed from graph topology. The MVP selection
rule is "follow the actor's `ObjectBranch` for `(object, name)` to its
`ObjectRevision`," producing a single-element frontier; future statement
types (`Provides`, `Build`, `Observation`, ...) join the frontier per facet,
at most one currently-active statement per facet.

The frontier is the basis for snapshot identity. Two nodes with the same
frontier compute the same `SnapshotId`, regardless of which actor's branch
they followed to get there or what local trust they assign.

A frontier is **not** the head — `head` is one `ObjectBranch` name and a
frontier may compose across multiple statement types and branches. Nor is it
"all statements about the Object" (that is history, not a current cut). Nor
is it "the leaves of the parent DAG" — leaves are structural; frontiers are
explicit. See `GLOSSARY.md` for the canonical definition.

#### Snapshot identity inputs

A `SnapshotId` is computed over:

- Object ID
- active statement frontier (sorted `StatementId`s)
- referenced revision and manifest hash, derived from the frontier so the
  snapshot is self-describing without re-fetching the underlying statements
- (future) artifact path mappings, capability metadata, and any other
  effective-state fields contributed by additional statement types as they
  land

It explicitly excludes:

- build artifacts
- federation metadata
- availability or trust information
- which actor's branch was used to resolve the frontier — snapshot identity
  is over the frontier itself, not the resolution path

Two snapshots with identical Object ID, active statement frontier, and canonical
effective state MUST have identical Snapshot IDs.

Canonical bytes and full derivation rules: `schemas/canonical/snapshot-v1.md`.

#### MVP frontier

In the MVP the only contributing statement type is `ObjectRevision`, so the
frontier is a single `StatementId`. The default snapshot for an object is
computed by:

1. Resolving the latest `ObjectBranch` for `(actor, object, "head")` —
   defaulting `actor` to `ObjectGenesis.created_by`.
2. Following that branch to its `ObjectRevision` statement.
3. Building a snapshot from `(object_id, [statement_id], revision_id, manifest_hash)`.

Callers may bypass branch resolution by pinning a specific `ObjectRevision`
`StatementId` directly. There is no implicit bootstrap from
`ObjectGenesis.initial_revision` — without an `ObjectRevision` there is no
manifest hash to bind, so no snapshot is yet defined.

#### Tags vs branches

`ObjectVersionTag` shares `ObjectBranch`'s actor-scoped, latest-wins
shape but uses a strict semver name and carries an explicit
`supersedes` chain. Branches are intended for in-flight pointers (`head`,
`audit`, `release`); tags are intended for published version names that
the future dependency resolver will consume (`1.2.3`, `1.2.3-rc.1`).

Both pointer types are mutable. Snapshot identity is over the
*frontier*, not over which actor's branch or tag was followed to
resolve it — so two callers following different actors' tags for
`1.2.3` may end up at different snapshots, but each snapshot is
independently verifiable. Consumers that need build reproducibility
must pin to the resolved `StatementId` (or `SnapshotId`), not to the
tag's version string.

### 2.4 What revision fields prove

A signed `ObjectRevision` statement carries three identity-bearing fields,
each with a precise meaning at the statement layer. None of them, by
itself, says the revision is correct end-to-end — that requires the
companion content (Git) layer.

- **`object`** — the lineage the revision claims to belong to. Statement-
  layer validation checks that the resolved `ObjectGenesis` derives this
  same `ObjectId`.
- **`revision`** — the storage revision id (e.g. `git:sha256:<commit>`).
  The statement layer takes it as opaque; the content layer (`kairo verify
  object` with a Git repo) confirms the commit exists in the supplied
  repository.
- **`parents`** — claimed predecessors of `revision`. The statement layer
  records their presence; the content layer compares them to the Git
  commit's actual parents (set-equality — parent ordering is not
  enforced).
- **`manifest_hash`** — the canonical `kairo.toml` hash the actor pinned
  at signing time. Statement-layer validation re-derives the hash from a
  parsed manifest and compares. With a Git repo available, the verifier
  reads the manifest from the commit's tree (`kairo.toml` at the root),
  removing a class of "verifying with the wrong manifest" mistakes.

The structured `ObjectRevisionValidationReport` (see `STATEMENTS.md` §6.2)
reports these checks independently. The `content` field carries the
result of the Git lookup: `Verified` (commit found, parents agree),
`ParentMismatch`, `CommitNotFound`, or `Indeterminate` (no repo
supplied or non-Git revision scheme).

---

## 3. Object Structure

Object
├─ versioned tree
│  ├─ kairo.toml
│  └─ files
├─ external blob references
└─ associated statements (external)

### 3.1 Versioned Tree

The versioned tree contains files, directories, and `kairo.toml`.

### 3.2 External Blobs

Objects may reference external blobs:

[[mounts]]
path = "disk.img"
digest = "sha256:..."

Rules:

- blob bytes are stored by content-addressed Blob ID
- references are versioned
- changing a Blob ID changes the Snapshot ID

---

## 4. `kairo.toml`

Defines object metadata and interfaces.

The normalized canonical form of `kairo.toml` is documented in:

```text
schemas/canonical/object-manifest-v1.md
```

`ObjectRevision.manifest_hash` refers to the `BlobId` derived from that
canonical manifest form, not to raw TOML bytes.

When validating an `ObjectRevision` against revision content, Kairo MUST parse
the revision's `kairo.toml`, compute the canonical manifest hash, and require it
to equal `ObjectRevision.manifest_hash`. If `[kairo].object` is present, it MUST
match the `ObjectRevision.object` field.

### 4.1 Kairo Metadata

```toml
[kairo]
schema = 1
object = "z6MkObject..." # optional consistency check once initialized
kind = "software"
name = "Example"
summary = "Example object."
```

The `object` field is optional during bootstrapping. When present, it is a
consistency check against the signed `ObjectRevision` binding, not the source of
authority for Object identity.

### 4.2 Content

```toml
[content]
kind = "tree"
```

### 4.3 Capabilities

```toml
[[provides]]
provides = "data:exe:mz"
version = "1.0.0"
```

### 4.4 Dependencies

```toml
[[dependencies]]
kind = "provides"
provides = "lib:zlib:static"

[[dependencies]]
kind = "object"
object = "z6MkObject..."
version = "^4.1.0"

[[dependencies]]
kind = "object"
object = "z6MkObject..."
snapshot = "z6MkSnapshot..."
```

Object dependencies MUST specify exactly one selector:

- `version` for a version requirement resolved by the planner
- `snapshot` for an exact Object snapshot

In `kairo.toml`, fields such as `object` and `snapshot` use bare ID payloads
because the field names provide type context. Standalone references use typed
forms such as `object:z6MkObject...` or
`kairo:object:z6MkObject...:snapshot:z6MkSnapshot...`.

### 4.5 Run Targets

[[run.targets]]
name = "main"
command = ["PROGRAM.EXE"]

[[run.targets.requires]]
provides = "environment:dos/x86"

---

## 5. Capabilities Model

Capabilities describe:

- data:*
- environment:*
- tool:*
- lib:*

They are non-authoritative and strengthened by usage.

---

## 6. Relationship to Builds

Objects may define builds (see BUILD.md).

---

## 7. Design Principles

- Objects have stable lineage identity
- Snapshots are immutable selected states
- Blob identity is content-based
- Capabilities are claims
- Execution is external
