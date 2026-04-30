# PLANNER.md

## 1. Overview

The Kairo planner is responsible for turning object metadata into executable or buildable plans.

The planner does not decide truth globally. It makes local, policy-driven decisions using:

- object metadata from `kairo.toml`
- object snapshot hashes
- artifact snapshot hashes
- capability declarations
- signed statements
- local trust policy
- available host capabilities

A planner resolves abstract requirements into concrete object snapshots, artifact snapshots, run targets, and build targets.

Core rule:

```text
metadata expresses requirements
planner resolves requirements
statements record exact resolutions
```

---

## 2. Inputs to Planning

A planning request names an intended operation.

Examples:

```text
build object O at snapshot S using target release
run object O at snapshot S using target main
view object O using a compatible viewer
materialize data object O into an environment
```

The planner may use:

```text
source object metadata
build target metadata
run target metadata
capability requirements
available artifacts
available environment providers
signed statements
local trust policy
```

---

## 3. Capabilities

Capabilities are conventional labels with the form:

```text
kind:specifier[:detail]
```

Core capability kinds:

```text
data
environment
tool
lib
```

Examples:

```text
data:exe:mz
data:program:dos
data:doom-wad
environment:dos/x86
environment:linux/x86-64
environment:web
environment:web:chrome:109
tool:make
tool:c_compiler:gcc
lib:zlib:static
lib:zlib:shared
```

Capabilities are not global truth. They are claims or inferred facts weighted by evidence.

---

## 4. Resolution Model

The planner resolves requirements in two phases:

### 4.1 Candidate Discovery

Given a requirement such as:

```text
provides = "tool:make"
version = ">=4 <5"
```

the planner finds candidate objects, artifacts, or outputs that may satisfy it.

Candidate evidence may include:

- `provides` declarations in `kairo.toml`
- signed provides statements
- successful build statements where the candidate was used for that capability
- inference statements
- observation statements
- local cache availability

### 4.2 Candidate Selection

The planner ranks candidates using local policy.

Signals may include:

```text
trusted actor explicitly claimed capability
trusted actor successfully used candidate
multiple actors independently used candidate
candidate produced reproducible artifacts
candidate artifact is already available locally
candidate version satisfies requested range
candidate is preferred by local policy
```

The planner records the exact result in a resolved manifest.

---

## 5. Exact Matching and Compatibility

By default, capability matching is exact.

For example:

```text
environment:docker
```

does not automatically satisfy:

```text
environment:oci
```

unless compatibility evidence exists.

Compatibility may be established through:

- signed compatibility statements
- local policy
- successful observed use
- explicit planner configuration

A more specific capability may satisfy a broader one only if policy allows it.

Example:

```text
environment:web:chrome:109
```

may satisfy:

```text
environment:web
```

if local policy accepts that relationship.

---

## 6. Resolved Manifests

A resolved manifest records the exact choices made by the planner.

Example:

```json
{
  "type": "kairo.resolved-manifest.v1",
  "requirements": [
    {
      "requested": {
        "kind": "provides",
        "provides": "tool:make",
        "version": ">=4 <5"
      },
      "resolved": {
        "object": "z6MkMakeObject...",
        "snapshot": "z6MkMakeSnapshot...",
        "target": "release",
        "output": "make",
        "artifact": "z6MkMakeArtifact..."
      }
    }
  ]
}
```

The manifest itself is canonicalized and hashed.

Build and run statements refer to this manifest hash.

---

## 7. Build Planning

To plan a build, the planner:

```text
1. reads the selected build target
2. resolves build dependencies
3. resolves required build environment
4. prepares the build workspace
5. prepares dependency mounts or paths
6. invokes the build target commands
7. collects declared outputs
8. constructs an artifact tree
9. computes the artifact snapshot hash
10. emits a signed build statement
```

The planner does not place build outputs back into the source object.

Artifacts are separate snapshot-addressed content.

---

## 8. Run Planning

To plan a run, the planner:

```text
1. reads the selected run target
2. resolves required input capabilities
3. resolves required environment capabilities
4. resolves environment providers if needed
5. constructs a provider chain
6. binds inputs to provider-specific hosting rules
7. prepares the execution plan
8. optionally emits a signed run statement
```

A run target declares what it requires, not how every possible provider executes it.

---

## 9. Environment Capabilities

Environments are first-class capabilities.

Examples:

```text
environment:linux/x86-64
environment:dos/x86
environment:web
environment:oci
environment:docker
environment:wasm
```

An object may require an environment:

```toml
[[run.targets.requires]]
kind = "provides"
provides = "environment:dos/x86"
```

Another object may provide that environment:

```toml
[[run.targets.provides]]
provides = "environment:dos/x86"
```

The planner connects them.

---

## 10. Provider Chains

A provider chain is a recursive resolution of environment requirements.

Example:

```text
DOS program
  requires environment:dos/x86

DOSBox
  provides environment:dos/x86
  requires environment:linux/x86-64

Host
  provides environment:linux/x86-64
```

The resolved chain is:

```text
host environment
  → runs DOSBox provider
    → hosts DOS program
```

Provider chains may contain multiple layers:

```text
host linux
  → container runtime
    → emulator
      → guest program
```

Each layer is resolved using the same capability mechanism.

---

## 11. Environment Providers

An environment provider is a run target that provides an environment capability.

Example:

```toml
[[run.targets]]
name = "provide-dos"
kind = "environment-provider"
artifact = "release"
output = "dosbox"
command = ["bin/dosbox", "-conf", "/kairo/generated/dosbox.conf"]

[[run.targets.provides]]
provides = "environment:dos/x86"

[[run.targets.requires]]
kind = "provides"
provides = "environment:linux/x86-64"

[[run.targets.accepts]]
kind = "run-target"
requires = "environment:dos/x86"
role = "guest"
```

This means:

```text
this target can run on linux/x86-64
and can host another run target requiring dos/x86
```

The guest object does not need to know about this provider.

---

## 12. Hosting Rules

A provider may describe how accepted guests are materialized.

Example:

```toml
[run.targets.hosting]
guest_root = "/kairo/guest"
generated_config = "/kairo/generated/dosbox.conf"

[[run.targets.hosting.mounts]]
source = "guest.tree"
target = "C:\"

[[run.targets.hosting.commands]]
template = '''
[autoexec]
mount C /kairo/guest
C:
cd ${guest.workdir}
${guest.command}
'''
```

Hosting rules belong to the provider, not the guest.

This preserves separation:

```text
guest object:
  declares environment requirement

provider object:
  declares how to host guests for that environment

planner:
  binds guest to provider
```

---

## 13. DOS Provider Example

### 13.1 Guest Object

```toml
[name]
canonical = "Commander Keen Episode 1"
short = "keen1"

[content]
kind = "tree"

[[content.entries]]
path = "KEEN1.EXE"
provides = ["data:exe:mz"]

[[provides]]
provides = "data:program:dos"

[run]
default = "main"

[[run.targets]]
name = "main"
kind = "program"
command = ["KEEN1.EXE"]
workdir = "."

[[run.targets.requires]]
kind = "provides"
provides = "environment:dos/x86"
```

The guest declares only that it needs DOS/x86.

### 13.2 Provider Object

```toml
[name]
canonical = "DOSBox"

[[build.targets]]
name = "release"
command = ["make"]

[[build.targets.outputs]]
name = "dosbox"
path = "src/dosbox"
artifact_path = "bin/dosbox"
kind = "file"
provides = ["tool:dosbox"]

[[run.targets]]
name = "provide-dos"
kind = "environment-provider"
artifact = "release"
output = "dosbox"
command = ["bin/dosbox", "-conf", "/kairo/generated/dosbox.conf"]

[[run.targets.provides]]
provides = "environment:dos/x86"

[[run.targets.requires]]
kind = "provides"
provides = "environment:linux/x86-64"

[[run.targets.accepts]]
kind = "run-target"
requires = "environment:dos/x86"
role = "guest"
```

### 13.3 Resolved Plan

```json
{
  "type": "kairo.plan.run.v1",
  "subject": {
    "object": "z6MkKeenObject...",
    "snapshot": "z6MkKeenSnapshot...",
    "runTarget": "main"
  },
  "environmentChain": [
    {
      "provides": "environment:linux/x86-64",
      "source": "host"
    },
    {
      "provides": "environment:dos/x86",
      "provider": {
        "object": "z6MkDosboxObject...",
        "snapshot": "z6MkDosboxSnapshot...",
        "runTarget": "provide-dos",
        "artifact": "z6MkDosboxArtifact..."
      }
    }
  ],
  "guest": {
    "object": "z6MkKeenObject...",
    "runTarget": "main",
    "command": ["KEEN1.EXE"],
    "workdir": "."
  }
}
```

---

## 14. Web Environments

Web execution is modeled using the same environment capability system.

A web application may require:

```text
environment:web
```

or a more specific environment:

```text
environment:web:chrome:109
```

A browser or web runtime provider may provide:

```text
environment:web
```

A web viewer may consume data capabilities.

Example:

```toml
[[build.targets]]
name = "web-release"
command = ["npm", "run", "build"]

[[build.targets.outputs]]
name = "site"
path = "dist"
artifact_path = "."
kind = "directory"
provides = ["tool:web-viewer:data:doom-wad"]

[[run.targets]]
name = "view"
kind = "web-app"
artifact = "web-release"
output = "site"
entry = "index.html"

[[run.targets.requires]]
kind = "provides"
provides = "environment:web"

[[run.targets.inputs]]
name = "wad"
kind = "provides"
provides = "data:doom-wad"
role = "subject"
```

This means:

```text
the object is a web app
it runs in a web environment
it accepts Doom WAD data as input
```

The planner may use it when asked to view an object providing:

```text
data:doom-wad
```

---

## 15. Viewer Selection

A viewer is a run target that accepts data and provides a human-facing interaction.

To view an object, the planner:

```text
1. determines or infers the data capability of the selected object/path
2. finds run targets accepting that capability
3. resolves the viewer's environment requirements
4. constructs any required provider chain
5. binds the data object/path into the viewer input
```

Example:

```text
data object provides data:doom-wad
viewer accepts data:doom-wad
viewer requires environment:web
host provides environment:web
planner runs viewer with data object mounted as input
```

If the host lacks `environment:web`, the planner may resolve a browser provider if one exists.

---

## 16. Host Capabilities

A host may locally provide capabilities.

Examples:

```text
environment:linux/x86-64
environment:web
environment:oci
tool:network-fetch
```

Host capabilities are local facts, not archive objects.

A planner may terminate a provider chain when the current host satisfies the required environment.

---

## 17. Evidence and Social Learning

Planning uses evidence.

A candidate may become preferred because:

```text
trusted actors used it successfully
builds using it reproduced
discovery agents observed successful resolutions
content scanners inferred expected data types
```

Discovery agents may emit observation statements.

Example:

```json
{
  "type": "kairo.statement.observation.v1",
  "observedStatement": "z6MkBuildStatement...",
  "notes": [
    {
      "kind": "resolution-evidence",
      "requested": {
        "provides": "tool:make"
      },
      "resolved": {
        "object": "z6MkMakeObject...",
        "snapshot": "z6MkMakeSnapshot..."
      },
      "result": "success"
    }
  ]
}
```

Such observations influence future planner decisions according to local policy.

---

## 18. Failure Handling

Planning may fail if:

```text
no candidate satisfies a capability
no trusted candidate satisfies policy
provider chain cannot terminate on the host
required artifacts are unavailable
input/output constraints cannot be bound
version constraints conflict
```

A failed plan should report:

```text
unresolved requirement
candidate set considered
policy reason for rejection
missing artifacts or providers
```

---

## 19. Plan Records

Plans may be recorded as unsigned local records or signed statements.

Plans are useful for:

```text
debugging
replaying
explaining decisions
building trust
feeding discovery agents
```

Executed builds and runs should emit signed statements when their results are intended to be shared.

---

## 20. Relationship to Other Specs

`OBJECT.md` defines object structure, metadata, capabilities, and run interfaces.

`BUILD.md` defines build targets, artifacts, and build statements.

`STATEMENTS.md` defines signed claims and canonical statement hashing.

`OBJECT_STORE.md` defines storage and retrieval of objects, blobs, and artifacts.

`FEDERATION.md` defines exchange, trust, and social inference across nodes.

---

## 21. Design Principles

```text
Planning is local and policy-driven.
Capabilities are requirements, not truth.
Provider chains are recursive capability resolutions.
Environments are first-class capabilities.
Guests do not know their providers.
Providers describe how to host guests.
Builds and runs record exact resolutions.
Successful use becomes evidence for future planning.
```
