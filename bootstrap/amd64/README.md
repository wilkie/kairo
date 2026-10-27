# The 64-bit bootstrap (x86_64)

Kairo's 64-bit base, built from live-bootstrap's finished x86 root
(`bootstrap/live-bootstrap-gcc-15.2.0/`) by cross-compiling, so that no
64-bit binary from outside takes part. Each stage is an object; each
goes on from the one before it.

| Stage | Directory | Runs in | Built in | What |
|---|---|---|---|---|
| 1 | `cross/` | `linux/x86` | `linux/x86` | binutils 2.41 and gcc 15.2.0 for `x86_64-linux-musl`, with musl 1.2.5 and the Linux 4.14 headers as their sysroot |
| 2 | `cross-root/` | `linux/x86-64` | `linux/x86` | a minimal 64-bit root, cross-built with stage 1: musl, binutils, gcc, and a GNU userland |
| 3 | (next) | `linux/x86-64` | `linux/x86-64` | the same root, rebuilt natively by stage 2's, gcc bootstrapping itself, with perl and python |
| 4 | (next) | `linux/x86-64` | `linux/x86-64` | stage 3 rebuilt by itself, which must give the same bytes: the fixed point |

A stage that runs in one environment and is built in another says so
as `specs/DECISIONS.md` §53 has it: its top-level environment
dependency is what its artifacts run in, and its build target's own is
where the build runs. The compilers and binutils carry their target's
triple in their labels (`tool:gcc:x86_64-linux-musl`); only a native one
also offers the plain label (§53).

The builds use as many jobs as the guest has processors (`nproc`). The
job count isn't part of a build key (§52), so a build must give the
same bytes at any count; `kairo reproduce` checks that.
