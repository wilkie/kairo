# Snapshot v1 Canonical Encoding

## Type

```text
Snapshot
```

## Version

```text
1
```

## Domain Separator

```text
kairo.snapshot.v1
```

## Derived ID

```text
SnapshotId = z<base58btc(multihash_sha2_256(domain || canonical_snapshot_bytes))>
```

A `Snapshot` is not a signed statement; it is a deterministic, content-
addressed picture of an object's effective state at a chosen statement
frontier. Two parties with the same statements derive the same
`SnapshotId`. There is no signature on a snapshot — its truth is the
content-addressed integrity of the inputs plus the signatures on the
underlying frontier statements.

## Purpose

Statements record claims; snapshots record state. A `SnapshotId` is what
you cite when you want to point at "the effective state of object X at
this moment" in a way that any other node can recompute and verify, given
the same frontier.

A **frontier** is the set of currently-selected `StatementId`s whose
canonical contributions define the Object's effective state — selected
explicitly, not derived from graph topology. See `GLOSSARY.md` (under
"Frontier") and `OBJECT.md` §2.3 for the full definition.

In the MVP the only contributing statement type is `ObjectRevision`, so
the frontier is a single `StatementId` and the effective state is the
`(revision, manifest_hash)` carried by that statement. As more statement
types land (Builds, Provides, Observations, ...) they join the frontier
alongside without changing this encoding shape.

## Resolution Rule

The default snapshot for `(actor, object, name)` is computed by:

1. Resolve the latest `ObjectBranch` statement for `(actor, object, name)`
   via `BranchResolver::latest_branch`.
2. Load the `ObjectRevision` statement that branch points at.
3. Build a `Snapshot` from that revision and compute the `SnapshotId`.

A caller may instead pin the frontier explicitly by passing a specific
`ObjectRevision` `StatementId`, bypassing branch resolution.

## Example (logical)

```text
object         = zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk
frontier       = [zQm... (statement id of the chosen ObjectRevision)]
revision       = git:sha256:abc...
manifest_hash  = zQmfBE2w2UqKJhGZAxK4ZWb4JuCrvxnN9P4YKuvzfbPSvD5
```

Snapshots are not transmitted as JSON envelopes today; they are derived
locally from underlying statements. The CLI prints the resolved fields
plus the derived `SnapshotId`.

## Canonical Snapshot Fields

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| type `"Snapshot"` | `string` |
| version `1` | `u8` |
| `object` | `ObjectId` payload as `string` |
| `frontier` | `list<string>`, lexicographically sorted by payload |
| `revision` | storage revision as `string` |
| `manifest_hash` | `BlobId` payload as `string` |

`frontier` is sorted at canonicalization time so the `SnapshotId` is
independent of the order the caller assembled it. In the MVP the list
always has a single element; sorting future-proofs.

## Excluded Fields

The following must not be included in `SnapshotId` canonical bytes:

- build artifacts
- federation metadata
- availability or trust information
- which actor's branch was used to resolve the frontier (snapshot identity
  is over the *frontier*, not the resolution path)
- timestamps of resolution

The actor whose branch a caller followed is part of the *snapshot lookup*,
not part of the snapshot itself. Two callers following different actors'
branches end up at potentially different snapshots, but a snapshot does
not record which actor's view produced it.

## Rust-Equivalent Pseudocode

```text
canonical_snapshot =
  string("Snapshot") ||
  u8(1) ||
  string(object) ||
  list(sorted(frontier), string) ||
  string(revision) ||
  string(manifest_hash)

snapshot_id =
  sha2_256_multihash_base58btc(
    "kairo.snapshot.v1" || canonical_snapshot
  )
```

## Notes

- `object` is the bare `ObjectId` payload.
- `frontier` entries are bare `StatementId` payloads, sorted lexicographically.
- `revision` is a storage revision id (e.g. `git:sha256:<commit>`),
  derived from the frontier's `ObjectRevision` body — encoded directly so
  a snapshot is self-describing without re-fetching its frontier
  statement.
- `manifest_hash` is a `BlobId` payload, also derived from the frontier
  body for the same reason.
- A snapshot whose frontier `ObjectRevision` does not bind to the snapshot's
  `object` is a configuration error and must not be constructed; the Rust
  implementation enforces this with `Snapshot::from_object_revision`.
- Bootstrap (`ObjectGenesis.initial_revision` set, no `ObjectBranch`,
  no `ObjectRevision`) does **not** produce a snapshot — there is no
  manifest hash to bind. Sign at least one `ObjectRevision` and (optionally)
  point a `Branch` at it before computing a snapshot.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
