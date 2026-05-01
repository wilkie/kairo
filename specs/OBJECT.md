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

It is computed over:

- Object ID
- active statement frontier
- canonicalized effective metadata
- artifact path mappings
- referenced revision and blob IDs
- purpose-specific closure metadata where required by `CORE_LIBRARY.md`

It explicitly excludes:

- build artifacts
- federation metadata
- availability or trust information

Two snapshots with identical Object ID, active statement frontier, and canonical
effective state MUST have identical Snapshot IDs.

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
