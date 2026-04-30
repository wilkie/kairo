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

See `CORE_LIBRARY.md`.

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
