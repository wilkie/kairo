# Environment Contract: `linux-root` v1

## Label

```text
environment:linux-root/<arch>
```

`<arch>` is the architecture in label spelling: `x86-64`, `x86`,
`aarch64`, ... An engine implements the contract only for an
architecture the host runs natively. An x86-64 kernel with IA32
emulation runs `x86` natively, and bubblewrap offers `x86` there only
when its probe runs a static i386 program in a sandbox
(`specs/DECISIONS.md` §38 sub-decision 7).

## Status

Implemented by the `bubblewrap/1` engine (`specs/DECISIONS.md` §38
sub-decision 5). A change that could alter build output is a new
contract label, never a revision of this one (§34 sub-decision 1).

## Purpose

Some software assumes it owns a filesystem hierarchy rooted at `/`:
live-bootstrap's scripts run from `/steps`, install to `/usr`, and bake
those paths into what they build (tcc embeds its prefix). Under this
contract the build tree **is** the guest's root. It is a separate label
from `linux` because where the guest finds things is part of the promise
and outputs record it, so results under the two must not be confused.

## Promises

A guest command running under this contract sees exactly:

| Path | Contents | Access |
|---|---|---|
| `/` | the snapshot's tree, with every dependency placed in it (below); the command runs in `/<workdir>`, and declared outputs are collected relative to it | read-write |
| `/<path>` | each dependency's `path` placement: a source at its pinned commit, a file source's bytes, a provider's artifact or tree, optionally only its `from` subtree, and with `overlay` filling in around what the tree already holds | read-write |
| `/tmp` | empty, mounted over the tree's own | read-write |
| `/proc` | the sandbox's own processes, mounted over the tree's own | — |
| `/dev` | bubblewrap's minimal set (as in `linux-v1.md`), mounted over the tree's own | — |

Nothing else: no `/kairo` paths and no host paths. **Every dependency
must be placed**, since there is nowhere else for it to appear; a plan
with an unplaced source, artifact, or content dependency is refused
before it runs.

And, as `linux` v1 except for identity:

- **Kernel**: the Linux system-call interface of the running kernel.
- **Network**: a fresh network namespace with only loopback.
- **Processes and IPC**: fresh PID, IPC, UTS, and cgroup namespaces.
- **Identity**: hostname `kairo`; uid and gid **0**, with every
  capability, inside the guest's own user namespace (`DECISIONS.md`
  §39 sub-decision 4). On the host the guest is still the user who runs
  Kairo. So files can be chown'd to 0, checks for root pass, and tmpfs
  or proc can be mounted inside the guest's own mount namespace; device
  nodes can't be created (`mknod` of a device fails), no id but 0 can
  own a file, and there is no `/sys`. Software needing those belongs to
  a contract a VM engine implements.
- **Environment**, and nothing else:
  - `HOME=/tmp`, `LANG=C`, `TZ=UTC`;
  - `SOURCE_DATE_EPOCH`: the revision's `created_at`, in epoch seconds;
  - `KAIRO_SRC=/`;
  - `KAIRO_PROVIDER_<LABEL>`: each placed dependency's path in the
    guest (`/AMD64/bin`), named as in `linux` v1; file dependencies have
    none;
  - `PATH`: the `bin/` directories of the placed artifacts and trees
    that have one, in placement order.

The mount points `/proc`, `/dev`, and `/tmp` are created in the tree if
it lacks them, and remain there, empty, after the run.

## Not promised

- **The clock is not frozen**, as in `linux` v1.
- Resource limits beyond the wall-clock timeout.
- Anything about the host beyond the kernel interface.

## Equivalence

Engines that implement this label are interchangeable, as for `linux`
v1. An artifact built under `linux-root` is never a substitute for one
built under `linux`, or the reverse.
