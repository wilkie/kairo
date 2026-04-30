# BUILD.md

## 1. Overview

The BUILD specification defines how Objects produce artifacts.

A **build** is a deterministic transformation:

(source object snapshot + resolved dependencies) → artifact snapshot

Builds are:

- declarative (described in `kairo.toml`)
- executed by planners
- recorded via signed build statements
- reproducible when inputs are identical

---

## 2. Build Targets

Builds are organized into **targets**.

Each target defines a specific transformation.

Example:

[[build.targets]]
name = "release"
command = ["make", "release"]
workdir = "."

A build target includes:

- name (stable identifier)
- command(s) to execute
- working directory
- dependencies
- outputs

---

## 3. Build Dependencies

Dependencies specify requirements for building.

### 3.1 Capability-based dependencies

[[build.targets.dependencies]]
kind = "provides"
provides = "tool:make"
version = ">=4 <5"

### 3.2 Object dependencies

[[build.targets.dependencies]]
kind = "object"
object = "z6MkObject..."
version = "^1.0.0"

Rules:

- dependencies express requirements only
- resolution is performed by the planner
- exact resolution is recorded in build statements

---

## 4. Build Execution Model

A build proceeds as:

1. Materialize source object at snapshot
2. Resolve dependencies
3. Prepare execution environment
4. Execute build commands
5. Collect outputs
6. Construct artifact tree
7. Compute artifact snapshot hash
8. Emit build statement

---

## 5. Outputs

Outputs define what is included in the artifact.

Example:

[[build.targets.outputs]]
name = "cli"
path = "dist/app"
artifact_path = "bin/app"
kind = "file"

Fields:

- name: logical identifier
- path: path in build workspace
- artifact_path: path inside artifact
- kind: file or directory

Outputs define the boundary of the artifact.

---

## 6. Artifact Model

A build produces a single **artifact snapshot**.

Example:

artifact
├─ bin/app
├─ lib/libapp.a
└─ share/data

Properties:

- content-addressed
- immutable
- reproducible
- independent of source object

Outputs are named projections of this artifact.

---

## 7. Referencing Outputs

Other objects may depend on specific outputs:

(object, target, output)

Resolved form:

artifact snapshot + subpath

---

## 8. Build Statements

Successful builds produce signed statements.

Example:

{
  "type": "kairo.statement.build.v1",
  "subject": {
    "object": "z6MkApp...",
    "snapshot": "z6MkSnapshot..."
  },
  "target": "release",
  "resolvedManifest": {
    "hash": "z6MkResolvedManifest..."
  },
  "artifact": {
    "snapshot": "z6MkArtifactSnapshot..."
  },
  "result": "success"
}

Statements:

- bind inputs to outputs
- record dependency resolution
- enable reproducibility
- provide trust evidence

---

## 9. Artifact Identity

Artifacts are identified by snapshot hash.

Two builds with identical inputs MUST produce identical artifact hashes.

---

## 10. Caching

Builds may be reused if:

source snapshot + resolved dependencies + target are identical

Cache key:

(source snapshot, resolved manifest, target)

---

## 11. Determinism

Builds SHOULD be deterministic.

Non-deterministic builds reduce reproducibility and trust.

---

## 12. Relationship to Objects

Artifacts:

- are not automatically objects
- may be promoted to objects
- may be consumed by other builds

---

## 13. Relationship to Federation

Build statements are signed and shared.

Trust is derived from:

- actor identity
- reproducibility
- repeated successful use

See FEDERATION.md.

---

## 14. Design Principles

- builds are declarative
- outputs define artifact boundaries
- artifacts are immutable
- dependency resolution is external
- reproducibility is primary
- trust is derived from signed evidence
