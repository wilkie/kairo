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

## Statement

A `Statement` is an immutable signed claim made by an Actor. Statements describe
Object creation, revisions, ownership, delegation, version tags, dependencies,
build results, runtime observations, provider capabilities, and federation
advertisements.

See `STATEMENTS.md`.

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
