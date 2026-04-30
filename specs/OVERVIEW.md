# OVERVIEW.md

Kairo is a federated software archival system for preserving, discovering,
building, running, and verifying software artifacts over long periods of time.
It is designed to archive both newly created software and already-obsolete
software whose original repositories, package feeds, operating systems, or
build tools may no longer be available.

The core archival unit is an **Object**. An Object is a stable logical lineage:
for example a source tree, binary release, dataset, disk image, toolchain,
library, emulator, operating system image, or environment provider. Object
identity is stable across revisions and is separate from any particular content
revision. A specific Object state is represented as a **Snapshot**, and the
content bytes behind a source revision may be backed by Git or another
content-addressed storage mechanism.

Kairo uses signed **Statements** to describe Object creation, revisions,
version tags, ownership, delegation, dependency declarations, build results,
runtime observations, provider capabilities, and federation advertisements.
Statements are made by **Actors**, which are cryptographic identities that can
operate on any node in the federation. Other nodes verify statement signatures
and apply local trust policy before acting on them.

Kairo does not depend on external package repositories or live networks when
building or running archived software. Dependencies, compilers, tools, runtime
libraries, emulators, and environment providers are themselves archived as
Objects. A build or run request resolves abstract requirements such as
`tool:gcc`, `lib:zlib`, or `environment:dos/x86` to concrete Object snapshots and
artifacts.

Builds produce **Build Artifacts**: cached outputs with exact provenance,
dependency selections, environment information, and runtime metadata. Build
artifacts are not automatically Objects, but they may be promoted into Objects
when they need independent archival identity.

Some Objects are **Provider Objects**. A Provider Object declares that it can
provide a capability, tool, runtime, operating system, architecture, emulator,
container, browser environment, or similar execution context. Provider chains
let Kairo map an old required environment onto something available locally, such
as a DOS emulator running inside a container on the host.

The **core library** is the authority for deterministic Object, Statement,
Snapshot, validation, and planning semantics. It validates snapshot closures and
produces build/run plans, but it does not execute code, manage networking, or
make local policy decisions.

The **daemon** owns long-running local state: store access, background tasks,
federation coordination, policy checks, execution dispatch, streaming logs, and
runtime sessions. The **CLI** binary is `kairo`; it communicates with the daemon
by default and may use direct/local mode for safe offline validation,
bootstrapping, and diagnostics. The **web client** uses the same daemon API and
must not reimplement Kairo validation semantics.

The **federation** exchanges signed statements, content-holder advertisements,
search indexes, provider advertisements, and build/run observations. Federation
helps nodes discover data; it does not decide truth. Every receiving node
validates hashes, signatures, closure completeness, and local trust policy.

Version names such as `4.1.2` are represented by signed version/tag statements
that map human-readable names to concrete Object revisions or snapshots. They
are not embedded into content hashes and are not trusted merely because a remote
node advertises them.

Kairo's main implementation decision is to keep semantics shared and typed:
Rust crates define the trusted model, storage and federation provide data to
that model, and the CLI/web client present daemon/core results rather than
inventing independent interpretations.
