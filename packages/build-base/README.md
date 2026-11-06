# build-base

What a typical C or C++ package builds with, as one object to depend
on (`specs/DECISIONS.md` §57). It doesn't build, and it holds no files:
it is a list of dependencies. A build that depends on `data:build-base`
takes that list as its own dependencies, as if its manifest named
each.

| Group | Labels |
|---|---|
| A shell and the GNU userland | `tool:bash`, `tool:coreutils`, `tool:sed`, `tool:grep`, `tool:gawk`, `tool:make`, `tool:diffutils`, `tool:findutils`, `tool:patch` |
| Archivers, compressors, and m4 | `tool:tar`, `tool:gzip`, `tool:xz`, `tool:bzip2`, `tool:m4` |
| The native toolchain | `tool:binutils`, `tool:gcc`, `tool:g++`, `lib:stdc++:shared`, `lib:gcc:shared`, `lib:atomic:shared` |
| The C library and headers | `lib:musl:headers`, `lib:musl:shared`, `lib:musl:static`, `lib:linux:headers` |

**One object for every environment.** It names no environment, and its
labels name no target triple. Each label resolves to the package for
the environment the build runs in (§53), so the same object brings
live-bootstrap's packages to a build in `linux/x86` and the 64-bit
bootstrap's to one in `linux/x86-64`.

**A build's own labels win.** A build that names one of these labels
itself replaces this object's requirement on it. A build that wants gcc
7 names `tool:gcc` with `^7` and still takes everything else from here.

**Releases only.** Its labels are resolved as any requirement that
names no object is: only releases, version-tagged snapshots, satisfy
them (§61). So it serves a build once every package it names is a
release; until then a package's first generation builds with
`../bootstrap-base/`, which names the bootstraps' objects.

**In a plan.** Each brought dependency is a requirement of the build,
resolved, chosen, and conferred as its own are. It is recorded after
this object, with `brought_by` naming it, and `kairo build` shows
`(brought by object:<id>)`.

| File | What it is |
|---|---|
| `kairo.toml` | The label it provides, `data:build-base` 1.0.0, and the dependencies it brings. |
| `import.sh` | Creates the object with `../../bootstrap/wrapper.sh`. |

## Running it

```sh
packages/build-base/import.sh <your actor id>
```

Then depend on it from a target:

```toml
[[build.targets.dependencies]]
kind = "provides"
provides = "data:build-base"
```
