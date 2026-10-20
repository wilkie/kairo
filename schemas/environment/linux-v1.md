# Environment Contract: `linux` v1

## Label

```text
environment:linux/<arch>
```

`<arch>` is the architecture in label spelling: `x86-64`, `aarch64`, ...
An engine implements the contract only for the architecture it runs
natively.

## Status

Implemented by the `bubblewrap/1` engine (`specs/DECISIONS.md` §35).
A change that could alter build output is a new contract label, never a
revision of this one (§34 sub-decision 1).

## Promises

A guest command running under this contract sees exactly:

| Path | Contents | Access |
|---|---|---|
| `/kairo/src` | the snapshot's tree; the command runs in `/kairo/src/<workdir>`, and declared outputs are collected relative to it | read-write |
| `/kairo/artifact/<artifact id>` | each artifact provider's artifact tree | read-only |
| `/kairo/content/<snapshot id>` | each content provider's tree | read-only |
| `/kairo/source/<source id>/<oid>` | each source dependency without a `path`, at its pinned commit | read-only |
| `/kairo/src/<path>` | each source dependency with a `path`, bound over an empty directory | read-only |
| `/kairo/home` | empty | read-write |
| `/tmp` | empty | read-write |
| `/proc` | the sandbox's own processes | — |
| `/dev` | bubblewrap's minimal set: `null`, `zero`, `full`, `random`, `urandom`, `tty`, `ptmx`, `pts/`, `shm/`, and the `fd`, `stdin`, `stdout`, `stderr` links | — |

Nothing else: no host `/usr`, `/lib`, `/bin`, or `/etc`. Every program,
the shell included, comes from a provider.

And:

- **Kernel**: the Linux system-call interface of the running kernel.
  Its release is recorded in the execution record, not promised.
- **Network**: a fresh network namespace with only loopback.
- **Processes and IPC**: fresh PID, IPC, UTS, and cgroup namespaces.
- **Identity**: hostname `kairo`; uid and gid 1000.
- **Environment**, and nothing else:
  - `HOME=/kairo/home`, `LANG=C`, `TZ=UTC`;
  - `SOURCE_DATE_EPOCH`: the revision's `created_at`, in epoch seconds;
  - `KAIRO_SRC=/kairo/src`;
  - `KAIRO_PROVIDER_<LABEL>`: each requirement's provider path, with the
    label upper-cased and every other character as `_`
    (`tool:make` → `KAIRO_PROVIDER_TOOL_MAKE`); a source dependency's
    label is `source:<last URL segment>` (`KAIRO_PROVIDER_SOURCE_M2LIBC`);
  - `PATH`: the `bin/` directories of the providers that have one, in
    requirement order.

## Not promised

- **The clock is not frozen.** Wall-clock reads differ between runs;
  execution records list `wall clock not frozen` under known
  nondeterminism. A contract that freezes time will have a new label.
- Resource limits beyond the wall-clock timeout.
- Anything about the host beyond the kernel interface.

## Equivalence

Engines that implement this label are interchangeable: a plan's hash
names the contract, not the engine, so an artifact built under one is a
valid input under any other, and agreeing results from different
engines are evidence for both (§34 sub-decision 4).
