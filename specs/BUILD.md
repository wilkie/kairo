# BUILD.md

## 1. Overview

The BUILD specification defines how Objects produce artifacts.

A **build** is a deterministic transformation:

(source object snapshot + resolved dependencies) → artifact snapshot

Builds are:

- declarative (described in `kairo.toml`, pinned by the manifest hash)
- planned by the resolver and run by an executor (`DECISIONS.md` §33)
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

- name (stable identifier, `[a-z][a-z0-9-]*`, unique)
- command: one program and its arguments, run without a shell
- working directory (relative to the tree root; default `"."`)
- dependencies
- outputs (at least one)

---

## 3. Build Dependencies

Dependencies specify requirements for building.

### 3.1 Capability-based dependencies

[[build.targets.dependencies]]
kind = "provides"
provides = "tool:make"
version = ">=4, <5"

### 3.2 Object dependencies

[[build.targets.dependencies]]
kind = "object"
object = "zQmObject..."
version = "^1.0.0"

Rules:

- a target's requirements are the manifest's top-level `[[dependencies]]`
  plus its own
- an object that doesn't build, chosen for one of them, brings its own
  dependencies, which the target takes as its own requirements, after
  it; a label the target names itself replaces a brought requirement on
  it (`DECISIONS.md` §57)
- a target's build never uses its own object's offers except an older
  generation's: a version-tagged snapshot whose revision is an ancestor
  of the one being built (`DECISIONS.md` §59)
- a provides dependency may name the objects whose offers alone satisfy
  it, `offered_by` (`DECISIONS.md` §59)
- version requirements use Cargo's syntax
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
- path: path relative to the target's working directory
- artifact_path: path inside artifact (unique within the target)
- kind: file or directory

The artifact is what the build stages under `/kairo/package`, laid out as
it should be installed, together with each output, copied there at its
`artifact_path` (`DECISIONS.md` §41): nothing else the build touched is
collected. `[build.targets.artifact]` names a subdirectory of the
package as the artifact (`root`) and where it lands in consumers'
environments (`install`, `/` by default, or `false`).

Outputs define the boundary of the artifact. Symbolic links inside an
output are kept as links, never followed: a relative target must stay
inside the artifact, and an absolute one is kept as the guest's path; a
`file` output may itself be a link
(`DECISIONS.md` §39, `schemas/canonical/artifact-tree-v1.md`).

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

An artifact keeps each file's contents and permission bits, its
symbolic and hard links, and directories that are empty or not `0755`;
not owners or times (`DECISIONS.md` §54,
`schemas/canonical/artifact-tree-v1.md`).

---

## 7. Referencing Outputs

Other objects may depend on specific outputs:

(object, target, output)

Resolved form:

artifact snapshot + subpath

---

## 8. Build Statements

> Superseded by `DECISIONS.md` §33 sub-decision 8 (`BuildResult`).

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

## Runtime requirements

A target's artifact may need other labels beside it wherever it is
installed (`requires`), and the labels it was built with may confer
requirements on it (`confers`); both are declared in `kairo.toml` and
gathered by the planner (`DECISIONS.md` §46, `PLANNER.md` §8.1). The
executor installs a plan's `runtime` section after its declared
requirements, in the same way, and refuses a build in which two
providers of one label that claim to co-exist put a file at the same
path.
