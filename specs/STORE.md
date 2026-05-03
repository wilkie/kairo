# STORE.md

## Status

Draft specification.

This document defines the Kairo local object store. The store provides durable
persistence and indexing for objects, statements, blobs, and snapshot closures.
It implements provider traits required by the core library.

---

## 1. Purpose

The Kairo Store is responsible for:

1. Persisting object records.
2. Persisting statement logs.
3. Persisting blobs/artifacts.
4. Caching and serving snapshot closures.
5. Providing efficient lookup and indexing.
6. Implementing core provider traits.

The store is not responsible for:

1. Statement interpretation (core responsibility)
2. Federation protocols (federation layer)
3. Execution (daemon/runtime)
4. User interfaces

---

## 2. Design Principles

### 2.1 Content-addressed storage

All stored entities SHOULD be content-addressed where possible:

- Objects
- Statements
- Blobs
- Snapshots (derived)

### 2.2 Immutability

Stored records are immutable:

- Objects are append-only via statements
- Statements are immutable
- Blobs are immutable

### 2.3 Separation of durable vs cache

The store MUST distinguish:

- Durable data (authoritative local copy)
- Cache data (re-fetchable or derivable)

---

## 3. Data Categories

### 3.1 Objects

Object records as defined in `OBJECT.md`.

### 3.2 Statements

Statement logs as defined in `STATEMENTS.md`.

### 3.3 Blobs

Binary or structured content referenced by artifacts.

### 3.4 Snapshot Closures

Derived collections used for validation/build/run.

---

## 4. Storage Layout (Conceptual)

Implementation-specific, but conceptually:

```text
store/
  objects/
    <object_id>/
      object.toml
  statements/
    <object_id>/
      <statement_id>.stmt
  blobs/
    <hash_prefix>/<hash>
  snapshots/
    <snapshot_id>.closure
  index/
    ...
```

Exact layout is not mandated but must preserve semantics.

#### MVP layout

The MVP `FilesystemStore` (`crates/kairo-store/src/lib.rs`) deliberately
**does not group statements by object**. Each record type lives under
its own top-level directory and is sharded two levels deep using
characters from the record id's base58 payload:

```text
~/.kairo/
  version.txt                            # store schema version
  actors/<XX>/<YY>/<actor-id>.json       # ActorGenesis bodies
  objects/<XX>/<YY>/<object-id>.json     # signed ObjectGenesis
  statements/<XX>/<YY>/<statement-id>.json
                                         # signed ObjectRevision /
                                         # ObjectBranch / ObjectVersionTag /
                                         # ActorTrust
  branches/<XX>/<YY>/<object-id>.json    # per-object materialized
                                         # branch tip index
  version_tags/<XX>/<YY>/<object-id>.json
                                         # per-object materialized
                                         # version-tag head index
  trust/<XX>/<YY>/<trusted-actor-id>.json
                                         # per-trusted-actor materialized
                                         # trust-opinion index, keyed
                                         # internally by truster
  blobs/<XX>/<YY>/<blob-id>              # raw blob bytes
  keys/<actor-id>.json                   # FilesystemKeystore (mode 0600
                                         # on Unix; not part of the store
                                         # crate proper)
```

Sharding (2 levels of 2 base58 chars from positions 3–4 and 5–6 of the
id) yields up to ~11.3M sparse leaf directories per record type. The
materialized indices (`branches/`, `version_tags/`, `trust/`) are
strict materializations of the underlying signed statements: rebuild
from `statements/` is correct but not yet implemented; for now the
write paths (`put_object_branch`, `put_object_version_tag`,
`put_actor_trust`) keep them consistent.

Snapshots are not stored in the MVP — `kairo snapshot compute` derives
a `SnapshotId` on demand from the resolved frontier; closure caching
is future work.

---

## 5. Provider Trait Implementation

The store MUST implement:

### 5.1 ObjectProvider

Returns object records.

### 5.2 StatementProvider

Returns statement sets.

Must support:

- Full object statement retrieval
- Partial retrieval (by frontier or dependency)

### 5.3 BlobProvider

Returns blob content or metadata.

### 5.4 SnapshotProvider (optional but recommended)

Returns snapshot closures.

---

## 6. Indexing

The store SHOULD maintain indexes for:

- ObjectId → object record
- ObjectId → statement IDs
- StatementId → statement
- BlobId → blob location
- ActorId → statements
- Dependency relationships
- Snapshot closures

Indexes must not affect semantics, only performance.

---

## 7. Snapshot Closure Caching

The store MAY cache snapshot closures.

Cached closures must:

1. Be associated with snapshot ID + purpose
2. Be invalidated if underlying data changes
3. Be recomputable from stored data

---

## 8. Consistency

The store must ensure:

- No partial writes visible
- Atomic persistence of records
- Integrity checks on read (optional but recommended)

---

## 9. Validation Responsibilities

The store:

- MUST NOT interpret statements
- MUST NOT enforce authority rules
- MUST NOT resolve effective state

The store MAY:

- Perform integrity checks (hashes, structure)
- Reject malformed records

---

## 10. Interaction with Core

The store provides data through provider traits.

Core:

- Validates
- Resolves
- Plans

The store:

- Supplies raw or cached data

---

## 11. Interaction with Federation

Federation layer:

- Writes to store
- Reads from store

Store must support:

- Ingestion of remote data
- Partial updates
- Conflict-tolerant storage (multiple branches)

---

## 12. Garbage Collection

The store MAY implement GC.

GC must:

- Not remove reachable data
- Not break snapshot closures
- Respect pinned snapshots or objects

---

## 13. Versioning

Store must track:

- Schema version
- Data format version

Must support migration or fail safely.

---

## 14. Security

The store must:

- Treat all incoming data as untrusted
- Verify hashes when possible
- Avoid path traversal issues
- Avoid executing stored content

---

## 15. Implementation Checklist

A conforming store should provide:

1. Durable storage for objects
2. Durable storage for statements
3. Blob storage
4. Provider trait implementations
5. Indexing layer
6. Snapshot closure cache (optional)
7. Integrity checks
8. Migration support

---

End of STORE.md
