# amd64 stage 1: the x86_64-linux-musl cross toolchain

The first stage of the 64-bit bootstrap (`../README.md`): binutils 2.41
and gcc 15.2.0, with C and C++, that run on x86 and make code for
`x86_64-linux-musl`, and the sysroot they build against, musl 1.2.5 and
the Linux 4.14 headers for x86_64. It's built on live-bootstrap's
finished x86 root, by that root's gcc 15.2.0, under the bubblewrap
engine, with no host tool taking part.

It is one artifact, installed at `/`: the compilers run their binutils
and read their sysroot by fixed paths, so the three come together. It
provides `tool:gcc:x86_64-linux-musl` and `tool:g++:x86_64-linux-musl`
15.2.0 and `tool:binutils:x86_64-linux-musl` 2.41 (`specs/DECISIONS.md`
§53), and not the plain `tool:gcc`: it isn't native to x86. Every path
it holds is named by the target (`usr/bin/x86_64-linux-musl-*`,
`usr/x86_64-linux-musl/`, `usr/lib/gcc/x86_64-linux-musl/`,
`usr/libexec/gcc/x86_64-linux-musl/`), so it installs beside the x86
root's own tools; the build fails if it holds any other.

## How it's put together

| Guest path | What | How it's pinned |
|---|---|---|
| `/` | live-bootstrap's finished x86 root | artifact, via `data:live-bootstrap:x86:libarchive-3.5.2` |
| `/` | the merged-/usr links, for `/bin/sh` | content, via `data:linux-filesystem:merged-usr` |
| `/kairo/src` | `src/` of this object's tree (`tree = "src"`) | the snapshot |
| `/kairo/src/distfiles` | binutils', gcc's, musl's, and Linux's tarballs | file sources, by SHA-256 |

| File | What it is |
|---|---|
| `kairo.toml` | The pins and placements above; one target, `x86`. |
| `src/build.sh` | Builds binutils; installs the kernel headers into the sysroot; builds a C-only gcc without a libc, and musl with it; then the whole gcc against that musl. Each part goes into the artifact and into `/`, for the parts after it. It then checks that the toolchain makes ELF64 x86_64 programs, static and dynamic, in C and C++. |
| `import.sh` | Archives the tarballs and creates the object with `../../wrapper.sh`. |

## Running it

Import live-bootstrap's six passes and `bootstrap/linux-filesystem/`
(their READMEs). Then:

```sh
bootstrap/amd64/cross/import.sh <your actor id>
kairo build object:<object id> --target x86 --actor <your actor id>
```
