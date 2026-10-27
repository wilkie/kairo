# amd64 stage 2: a minimal x86_64 root, cross-built

The second stage of the 64-bit bootstrap (`../README.md`): an x86_64
root holding what stage 3 needs to build itself natively, cross-built
under x86 by stage 1's toolchain (`../cross/`). It holds the Linux 4.14
headers, musl 1.2.5, binutils 2.41, gcc 15.2.0 with C and C++ (gmp,
mpfr, and mpc built in its tree), and bash, coreutils, make, sed, grep,
gawk, tar, xz, gzip, bzip2, diffutils, patch, m4, and findutils. perl
and python come in stage 3, built natively: cross-building perl needs
the separate perl-cross project, and Python a build Python of its own
version, and natively neither does.

Its programs run in `linux/x86-64`, its top-level environment; its
build runs in `linux/x86`, its target's (`specs/DECISIONS.md` §53). They
find musl's loader at `/lib/ld-musl-x86_64.so.1`, and scripts `/bin/sh`,
through the merged-/usr links, which it requires
(`data:linux-filesystem:merged-usr`). gcc's own libraries go in
`/usr/lib`, beside musl's, not in `lib64`. Documentation and
translations are left out: this root only builds the next.

It provides the whole root as `data:amd64-bootstrap:x86-64:cross-root`,
and no tool labels: stage 3's tools are the ones to use.

## How it's put together

| Guest path | What | How it's pinned |
|---|---|---|
| `/` | live-bootstrap's finished x86 root | artifact, via `data:live-bootstrap:x86:libarchive-3.5.2` |
| `/` | stage 1's cross toolchain | artifact, via `tool:gcc:x86_64-linux-musl` and `tool:g++:x86_64-linux-musl` 15.2.0 |
| `/` | the merged-/usr links | content, via `data:linux-filesystem:merged-usr` |
| `/kairo/src` | `src/` of this object's tree | the snapshot |
| `/kairo/src/distfiles` | the 21 tarballs | file sources, by SHA-256 |

| File | What it is |
|---|---|
| `kairo.toml` | The pins and placements above; one target, `x86-64`. |
| `src/build.sh` | Builds each package with `--host=x86_64-linux-musl` and installs it under `/usr` in the artifact; then checks that the artifact holds only `usr/`, no `usr/lib64`, and that every program is ELF64 x86_64. |
| `import.sh` | Archives the tarballs and creates the object with `../../wrapper.sh`. The tarballs are live-bootstrap's but for coreutils 9.4, GNU's release rather than live-bootstrap's own, and findutils 4.10.0. |

## Running it

Import live-bootstrap's six passes, `bootstrap/linux-filesystem/`, and
stage 1 (their READMEs). Then:

```sh
bootstrap/amd64/cross-root/import.sh <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
