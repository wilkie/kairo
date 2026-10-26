# Environment Contract: `linux` v1

## Label

```text
environment:linux/<arch>
```

`<arch>` is the architecture in label spelling: `x86-64`, `x86`,
`aarch64`, ... An engine implements the contract only for an
architecture the host runs natively. An x86-64 kernel with IA32
emulation runs `x86` natively, and bubblewrap offers `x86` there only
when its probe runs a static i386 program in a sandbox
(`specs/DECISIONS.md` §38 sub-decision 7).

## Status

Implemented by the `bubblewrap/1` engine (`specs/DECISIONS.md` §35).
Amended by §41 to the layout below, absorbing the former `linux-root`
contract; nothing built under the earlier forms was published. From
here on, a change that could alter build output is a new contract
label, never a revision of this one (§34 sub-decision 1).

## Promises

A guest command running under this contract sees exactly:

| Path | Contents | Access |
|---|---|---|
| `/` | the dependencies' artifacts at their install roots, each copied (or overlaid, §41 sub-decision 5) in requirement order, the first listed winning where two provide the same path; and absolute placements; over a writable layer of the build's own | read-write |
| `/kairo/src` | the snapshot's tree without its `kairo.toml` (`DECISIONS.md` §52), with relative placements in it; the command runs in `/kairo/src/<workdir>` | read-write |
| `/kairo/package` | empty; the build stages its artifact here, laid out as it wants to be installed | read-write |
| `/kairo/artifact/<artifact id>` | each artifact whose build target declares `install = false`, unless placed | read-only |
| `/kairo/content/<snapshot id>` | each content provider's tree (the part under its `[content] root`), unless placed, or installed at its `[content] install` root (`DECISIONS.md` §50), as an artifact is | read-only |
| `/kairo/source/<source id>/<oid>` | each source dependency without a `path`, at its pinned commit | read-only |
| `/tmp` | empty | read-write |
| `/proc` | the sandbox's own processes | — |
| `/dev` | bubblewrap's minimal set: `null`, `zero`, `full`, `random`, `urandom`, `tty`, `ptmx`, `pts/`, `shm/`, and the `fd`, `stdin`, `stdout`, `stderr` links | — |
| `/sys` | empty (§39 sub-decision 6) | read-only |

Nothing from the host: no host `/usr`, `/lib`, `/bin`, or `/etc`. Every
program, the shell included, comes from a dependency.

A placement (§37 sub-decision 7, §38 sub-decision 4) with an absolute
`path` lands at that guest path, in `/`; a relative one lands in
`/kairo/src`. The artifact is what is under `/kairo/package` (from the
target's `[build.targets.artifact] root`) after the command exits,
together with each declared output, copied there at its
`artifact_path`; nothing else in the guest is collected.

And:

- **Kernel**: the Linux system-call interface of the running kernel.
  Its release is recorded in the execution record, not promised.
- **Network**: a fresh network namespace with only loopback.
- **Processes and IPC**: fresh PID, IPC, UTS, and cgroup namespaces.
- **Identity**: hostname `kairo`; uid and gid **0**, with every
  capability, inside the guest's own user namespace (§39 sub-decision
  4). On the host the guest is still the user who runs Kairo. So files
  can be chown'd to 0, checks for root pass, and tmpfs or proc can be
  mounted inside the guest's own mount namespace; device nodes can't be
  created (`mknod` of a device fails), and no id but 0 can own a file.
  Software needing those belongs to a contract a VM engine implements.
- **Environment**, and nothing else:
  - `HOME=/tmp`, `LANG=C`, `TZ=UTC`;
  - `SOURCE_DATE_EPOCH`: the plan's, the target's `source_date_epoch`
    or 315532800 (1980-01-01T00:00:00Z), in epoch seconds
    (`DECISIONS.md` §52; it was the revision's `created_at`);
  - `KAIRO_SRC=/kairo/src`, `KAIRO_PACKAGE=/kairo/package`;
  - `KAIRO_PROVIDER_<LABEL>`: each requirement's location in the guest,
    with the label upper-cased and every other character as `_`
    (`tool:make` → `KAIRO_PROVIDER_TOOL_MAKE`): its install root, its
    placement, or its `/kairo/...` path; a source dependency's label is
    `source:<last URL segment>`; file dependencies have none;
  - `PATH`: the `bin/` (and `usr/bin/`) directories of the dependencies
    at `/kairo/...` paths and of placed ones that have them, in
    requirement order, then `/usr/local/bin:/usr/bin:/bin`.

The mount points `/proc`, `/dev`, `/tmp`, `/sys`, and `/kairo` are
created in `/` and remain there, empty, after the run; they are not
part of any artifact.

## Not promised

- **The clock is not frozen.** Wall-clock reads differ between runs;
  execution records list `wall clock not frozen` under known
  nondeterminism. A contract that freezes time will have a new label.
- Resource limits beyond the wall-clock timeout.
- Anything about the host beyond the kernel interface. In particular, a
  guest that mounts sysfs (or another kernel filesystem) itself, which
  root in its namespaces can, sees what the host kernel shows; the
  contract's empty `/sys` only keeps scripts that mount what isn't
  mounted from doing so.

## Equivalence

Engines that implement this label are interchangeable: a plan's hash
names the contract, not the engine, so an artifact built under one is a
valid input under any other, and agreeing results from different
engines are evidence for both (§34 sub-decision 4). Copying and
overlaying dependencies give the guest the same tree, so they are one
engine behaviour, not two.
