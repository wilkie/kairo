# amd64 stages 3 and 4: the x86_64 root, built natively, and its fixed point

The third and fourth stages of the 64-bit bootstrap (`../README.md`):
an x86_64 root built in `linux/x86-64` by the root before it, with no
x86 program taking part. It holds the Linux 4.14 headers, binutils
2.41, gcc 15.2.0 with C and C++ (gmp, mpfr, and mpc built
in its tree), musl 1.2.5, bash, coreutils, make, sed, grep, gawk, tar, xz, gzip,
bzip2, diffutils, patch, m4, findutils, zlib 1.3.1, libffi 3.5.2, perl
5.42.0, and Python 3.11.1.

Each package is installed into the artifact and into `/` as it is
built, so each after it is built with it. gcc bootstraps: it is built
by the root's gcc, then by itself twice, and the last two are compared.
Everything after it is built by that gcc, musl included: musl's
`libc.so` links libgcc in, so it is built after gcc, or it would carry
the previous root's libgcc and differ in stage 4.

One recipe, `src/build.sh`, serves two targets:

| Target | Stage | Built on | What it checks |
|---|---|---|---|
| `x86-64` | 3 | stage 2's cross-built root (`../cross-root/`, `data:amd64-bootstrap:x86-64:cross-root`) | that C, static and dynamic, and C++ compile and run, and perl and python run with the modules the libraries serve |
| `fixed-point` | 4 | stage 3's root (`data:amd64-bootstrap:x86-64:root`) | the same, and that it makes stage 3's root again: every link to the same target, every other file the same contents and executable or not, which is all an artifact keeps of a mode; it shows how each text file that differs differs |

When stage 4 holds, the root makes itself: stage 3's root depends on
what built it only through the source it was given.

The `x86-64` target provides the root as
`data:amd64-bootstrap:x86-64:root`, installed at `/`, and requires the
merged-/usr links (`data:linux-filesystem:merged-usr`), through which
its programs find musl's loader and scripts find `/bin/sh`.
Documentation and translations are left out.

perl's Configure records the machine's kernel version, the time it
ran, and any perl already installed, and Python's configure any Python
installed; `build.sh` fixes each, so the root doesn't depend on the
machine or on the root it is built on.
Python checks its bytecode by hash, not by time, under the
`SOURCE_DATE_EPOCH` Kairo sets (`specs/DECISIONS.md` §52). It is built
without pip, which it would fetch.

| File | What it is |
|---|---|
| `kairo.toml` | The sources, shared by both targets, and the two targets. |
| `src/build.sh` | Builds and installs each package in turn, then the checks; with `--fixed-point`, it lists the root it runs on before it starts and compares what it made with that. |
| `import.sh` | Archives the tarballs and creates the object with `../../wrapper.sh`. |

## Running it

Import stage 2 and what it needs (`../cross-root/README.md`). Then:

```sh
bootstrap/amd64/native-root/import.sh <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
kairo build object:<object id> --target fixed-point --actor <your actor id>
```

The second build builds the first if its artifact isn't held.
