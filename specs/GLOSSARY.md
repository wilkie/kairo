# GLOSSARY.md

## Object

An `Object` is a stable logical archival unit in Kairo. It may represent source
code, a binary, dataset, disk image, toolchain, library, emulator, application,
runtime, or environment provider.

An Object is versioned over time by signed statements. A specific state of an
Object is a Snapshot.

See `OBJECT.md` and `OBJECT_STORE.md`.

## Snapshot

A `Snapshot` is a selected state of an Object, defined by an Object ID and a
statement frontier. Snapshots are the primary unit of validation, build planning,
run planning, and reproduction.

See `CORE_LIBRARY.md` and `OBJECT.md` §2.3.

## Frontier

A `frontier` is the set of currently-selected `StatementId`s whose canonical
contributions, taken together, define an Object's effective Kairo state.

A frontier is **selected**, not computed: it is named directly as a finite set
of statement IDs rather than derived from graph topology. The selection rule
in the MVP is "follow the actor's `ObjectBranch` for `(object, name)` to its
`ObjectRevision`," producing a single-element frontier. Future statement types
(`Provides`, `Build`, `Observation`, ...) join the frontier per facet, with at
most one currently-active statement per facet.

A frontier is **the basis for snapshot identity**: two nodes with the same
frontier compute the same `SnapshotId`, regardless of which actor's branch
they followed to get there or what local trust they assign. The frontier
itself does not record the resolution path.

What a frontier is not:

- Not "the head." `head` is one named `ObjectBranch`; a frontier may compose
  contributions from multiple statement types and (eventually) multiple
  branches.
- Not "all statements about the Object." That is history; the frontier is a
  current cut.
- Not "the leaves of the parent DAG." Leaves come from graph structure;
  frontiers come from explicit selection. A stale leaf no one's branch points
  at is not in the frontier.

See `OBJECT.md` §2.3 and `schemas/canonical/snapshot-v1.md`.

## Fixity

A check that a record's content-addressed identity matches its bytes:
re-derive the id from the canonical bytes and confirm it equals the id
the record is *claimed* to have (its filename, its store key, the entry
in a manifest, the field on a wrapping envelope). For Kairo this means
recomputing an `ActorId`, `ObjectId`, `StatementId`, or `BlobId` from
the canonical encoding and refusing the record if the derivation
disagrees.

Fixity is a property of the data, not of the storage system. Because
every Kairo identifier is `multihash(domain || canonical_bytes)`,
identity *is* the bytes — there is no "later version of `zQm…`."
A mismatch can only mean tampering, corruption, the wrong domain
separator, or a bug in canonical encoding. None of those are silently
recoverable: every fixity failure is surfaced to the caller (e.g.
`StoreError::Corrupt { reason: HashMismatch { expected, actual } }`,
`BundleError::FixityMismatch`, `BundleError::BlobHashMismatch`) rather
than papered over with a "best effort" repair.

Where fixity is enforced today:

- **Store reads** — `get_actor`, `get_object_genesis`,
  `get_object_revision`, `get_object_branch`, `get_object_version_tag`,
  and `get_actor_trust` all re-derive the id from the parsed body and
  reject records whose derivation does not match the requested id.
- **Bundle import** — every actor/object/statement file is parsed and
  its id re-derived; every blob's bytes are re-hashed against
  `OBJECT_MANIFEST_DOMAIN` and compared to the filename. Re-importing
  the same bundle is idempotent because identical bytes produce
  identical ids.
- **Keystore reads** — the stored `actor_id` and `key_id` fields must
  agree with the recomputed values for the loaded secret key.
- **`kairo verify object`** — the genesis-derives-id check is part of
  the aggregate verdict; a fixity failure on the genesis record is
  reported as `INVALID`.

Fixity is distinct from **signature validity** (does the actor's
private key vouch for these bytes?), **trust** (do I, the local
caller, accept this actor's claims?), and **content-layer
verification** (does the storage commit named by an `ObjectRevision`
actually exist in Git, with the declared parents?). All four are
independent dimensions of the verification model in `STATEMENTS.md`
§6 and `ACTORS.md` §6.2; fixity is the prerequisite that the bytes
in front of you really are the record you asked for.

See `crates/kairo-store/src/error.rs` (`CorruptReason::HashMismatch`)
and `crates/kairo-bundle/src/error.rs`
(`FixityMismatch`, `BlobHashMismatch`).

## Statement

A `Statement` is an immutable signed claim made by an Actor. Statements describe
Object creation, revisions, ownership, delegation, version tags, dependencies,
build results, runtime observations, provider capabilities, and federation
advertisements.

See `STATEMENTS.md`.

## Version Tag

A `VersionTag` is an actor-scoped, mutable pointer that binds a strict
semver string (`1.2.3`, `1.2.3-rc.1`, `1.2.3+build.5`) to a specific
`ObjectRevision` statement, or withdraws a previously published binding.
Encoded as the `ObjectVersionTag` statement type and resolved
latest-wins on `(actor, object, version)`, like `ObjectBranch`. Every
non-genesis tag carries an explicit `supersedes` pointer so the
rebind / revoke chain is reconstructable as audit history.

Tags are mutable: an actor may rebind `1.2.3` to a different revision
(e.g. to revoke a bad release and replace it). The version string alone
is therefore not a stable handle for build reproducibility — consumers
that need stability must pin to the resolved `StatementId` (or
`SnapshotId`), the way Cargo.lock and package-lock.json do.

See `STATEMENTS.md` §4.2b and `schemas/canonical/object-version-tag-v1.md`.

## Trust

A first-person opinion that one actor (the *truster*, `by_actor`) holds
about another actor (the *trusted actor*, `trusted_actor`). Encoded as
the `ActorTrust` statement type with three observable values: `Trusted`,
`Untrusted`, or withdrawn (chain-leaf decision is `null`). Resolution
is per-truster and chain-precedence: the head for `(by_actor,
trusted_actor)` is the leaf of the supersedes chain. Cross-actor
`supersedes` is invalid — only the truster who signed `S` may publish
its successor.

Trust is **informational**: it never makes a cryptographically valid
statement invalid, and it never validates an invalid one.
`evaluate_trust` folds the chain leaf into one of `Trusted | Untrusted
| Unknown` (withdrawal collapses to `Unknown`); a caller that supplied
no truster sees `Unevaluated`.

See `STATEMENTS.md` §4.2c and `schemas/canonical/actor-trust-v1.md`.

## Capability

A `Capability` is a structured, signed authorization in which one actor
(the *grantor*) delegates to another actor (the *grantee*) the authority
to issue specific statement kinds on a scoped target — typically an
object or the grantor's own actor surface. A capability names a scope,
the statement kinds it covers, and optional constraints (expiration,
delegation depth, opt-in pinning to a specific signing key). Encoded as
the `ActorCapabilityGrant` statement type, retracted via
`ActorCapabilityRevocation`, sharded per-grantor.

This is the distributed-systems sense of "capability": a transferable,
unforgeable token of authority in the statement graph. Capability
validity is **semantic** and computed from grants, root authority, and
constraints; local trust policy may still refuse to act on a valid
capability. Capabilities make cross-actor authority claims (e.g.
`ObjectVersionTag` cross-actor `supersedes`) load-bearing; without
them, every authority claim is bounded by the actor that signed it.

Not to be confused with **runtime sandbox capability** (filesystem,
network, GPU access granted to an executing artifact). That is an
unrelated concept; see `SANDBOX.md`.

See `CAPABILITIES.md`.

## Actor

An `Actor` is a cryptographic identity that can issue signed statements.
Actors are portable across federation nodes; local trust policy determines which
Actor statements a node accepts.

See `ACTORS.md`.

## Core Provider Trait

A core provider trait is a Rust interface used by the core library to request
Objects, Statements, Blobs, or Snapshot closures from a store, daemon, package,
or federation-backed implementation.

See `CORE_LIBRARY.md` and `STORE.md`.

## Provider Object

A `Provider Object` is an Object that declares it can provide a capability,
tool, library, runtime, operating system, architecture, emulator, browser
environment, container, or virtual machine environment.

See `PLANNER.md` and `ENVIRONMENTS.md`.

## Daemon

The daemon is the long-running local service that coordinates store access,
federation, policy, task scheduling, execution dispatch, streaming logs, and API
serving.

See `DAEMON.md` and `API.md`.

## CLI

The Kairo command-line interface. The v1 binary name is `kairo`.

See `CLI.md`.

## Web Client

The TypeScript/React client that communicates with the daemon API to browse,
inspect, fetch, build, run, and manage Kairo data.

See `WEB_CLIENT.md`.
