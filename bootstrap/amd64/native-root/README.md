# amd64 stages 3 and 4: the x86_64 root, built natively, and its fixed point

The third and fourth stages of the 64-bit bootstrap (`../README.md`):
an x86_64 root built in `linux/x86-64` by the root before it, with no
x86 program taking part. It holds the Linux 4.14 headers, binutils
2.41, gcc 15.2.0 with C and C++ (gmp, mpfr, and mpc built
in its tree), musl 1.2.5, bash, coreutils, make, sed, grep, gawk, tar, xz, gzip,
bzip2, diffutils, patch, m4, findutils, zlib 1.3.1, libffi 3.5.2, perl
5.42.0, and Python 3.11.1.

Each package is installed into a directory of its own in the artifact,
a child object (`specs/DECISIONS.md` §56), and into `/` as it is built,
so each after it is built with it. gcc bootstraps: it is built
by the root's gcc, then by itself twice, and the last two are compared.
Everything after it is built by that gcc, musl included: musl's
`libc.so` links libgcc in, so it is built after gcc, or it would carry
the previous root's libgcc and differ in stage 4.

One recipe, `src/build.sh`, serves two targets:

| Target | Stage | Built on | What it checks |
|---|---|---|---|
| `x86-64` | 3 | stage 2's cross-built root (`../cross-root/`, `data:amd64-bootstrap:x86-64:cross-root`) | that C, static and dynamic, and C++ compile and run, and perl and python run with the modules the libraries serve |
| `fixed-point` | 4 | stage 3's children, each by its first label at its version | the same, and that it makes those children again, together: every link to the same target, every other file the same contents and executable or not; it shows how each text file that differs differs. Its artifact is stage 3's, the same id, which compares modes, directories, and hard links too (`specs/DECISIONS.md` §54) |

When stage 4 holds, the root makes itself: stage 3's packages depend on
what built them only through the source they were given.

## Children

Nothing uses the root whole: the targets' artifacts are installed
nowhere (`install = false`). Each package is a child, installed at `/`,
offering its labels at its version, and requiring what its programs and
libraries load (§46), as `build.sh` reports each child's `NEEDED`
sonames. The build fails if two children would hold one path.

| Children | Labels |
|---|---|
| `linux-headers-4.14.336` | `lib:linux:headers` |
| `binutils-2.41` | `tool:binutils`, `tool:binutils:x86_64-linux-musl` |
| `gcc-15.2.0` | `tool:gcc`, `tool:g++`, their `x86_64-linux-musl` labels, `lib:stdc++:static` |
| `libstdc++-15.2.0`, `libgcc_s-15.2.0`, `libgomp-15.2.0`, `libatomic-15.2.0`, `libitm-15.2.0`, `libquadmath-15.2.0` | `lib:stdc++:shared`, `lib:gcc:shared`, `lib:gomp:shared`, `lib:atomic:shared`, `lib:itm:shared`, `lib:quadmath:shared`, each conferring itself `>=15.2.0` |
| `musl-1.2.5-headers`, `-static`, `-shared` | `lib:musl:headers`, `lib:musl:static`, `lib:musl:shared` (conferring `^1.2.5`, requiring the merged-/usr links) |
| `bash-5.2.15`, `coreutils-9.4`, `make-4.2.1`, `sed-4.8`, `grep-3.12`, `gawk-5.3.0`, `tar-1.34`, `gzip-1.13`, `diffutils-3.10`, `patch-2.8`, `m4-1.4.19`, `findutils-4.10.0` | `tool:<name>`, and `tool:awk`, `tool:diff`, `tool:find` |
| `xz-5.6.4`, `bzip2-1.0.8` | `tool:xz` and `lib:lzma`, `tool:bzip2` and `lib:bz2` |
| `zlib-1.3.1`, `libffi-3.5.2` | `lib:zlib`, `lib:ffi` |
| `perl-5.42.0`, `python-3.11.1` | `tool:perl`, `tool:python` |

Every library is in `/usr/lib/x86_64-linux-musl`, x86-64's directory
for musl's (`specs/DECISIONS.md` §60), which musl's loader alone
searches, through `/etc/ld-musl-x86_64.path`, part of musl's shared
child; `/usr/lib` holds only the loader's link. Every package is
configured with that `--libdir`, gcc too, its multilib directory for
the OS the directory itself rather than `lib64`. musl's startup files
are linked in `/usr/lib` as well, where a gcc running from its build
tree, as one bootstrapping does, looks for them. Stage 2's root has the
same layout, so stage 3 builds as stage 4 does, on stage 3's children:
what gcc's configure finds of the C library is built into its driver,
and stage 4 would otherwise make another one.

gcc keeps its runtime libraries and C++ headers in its versioned
directory, where it links against them, so another gcc can sit beside
it; the shared libraries are copied out to `/usr/lib`, each a child of
its own, so one copy, the newest any gcc offers, serves every program
in an environment (§56). Documentation and translations are left out.

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
