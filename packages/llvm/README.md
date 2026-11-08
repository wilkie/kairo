# llvm

LLVM, its libraries and tools, for x86-64 alone (`specs/DECISIONS.md`
§65). One object for every release, each a version tag on its history
(§59), with a target for x86-64 and no environment of its own (§55).
Each release is built from this directory's recipe, its version the
only difference:

| Release | Tag | Built for |
|---|---|---|
| LLVM 20.1.8 | `20.1.8` | mrustc's bootstrap of rustc 1.90.0 (`../mrustc/`), the major rustc 1.90.0 bundles |
| LLVM 21.1.8 | `21.1.8` | `rust` from 1.91.1 (`../rust/`), the one major every release from 1.91 to 1.99 accepts |

This revision is 21.1.8; 20.1.8 is the revision before it.

**Why it's a package.** Rust builds against it (`../rust/`, §65): each
rustc accepts an external LLVM from a minimum major up to the one it
bundles, so one LLVM serves many releases of rustc, and whatever else
needs LLVM has it.

**Built with packages alone.** It builds with `data:build-base`
(`../build-base/`), CMake (`../cmake/`) and Ninja (`../ninja/`), which
build it, Python (`../python/`), which its build runs, and zlib
(`../zlib/`) and zstd (`../zstd/`), for compressed debug sections, each
a package whose labels only releases satisfy (§61): no bootstrap's
object takes part.

**x86-64 alone.** There is no x86 target: Rust, what it is for, is
built for x86-64 alone (§65 sub-decision 5). Its code generator is
x86's alone, too; more targets are a later release's.

**Its children.**

| Child | Where | Offers |
|---|---|---|
| `llvm-<version>` | `/usr/lib/llvm-<major>/`: its tools, `llvm-config` among them, headers, libraries, and CMake files | `lib:llvm`, `tool:llvm` |
| `libllvm-<version>` | its shared library, `libLLVM.so.<major>.<minor>`, in the library directory (§60) | `lib:llvm:shared` |

Each major is under its own directory, as Debian has them, so two
majors sit side by side; its tools find `libLLVM` beside them. A
program built against it loads the copy in the library directory, a
child of its own, as gcc's runtime libraries are (§56); its soname
names the major and minor, so each release's sits beside the others'.
`llvm-<version>` requires its own release's shared library, `~<major>.<minor>`.
A build asks for a major by version: `lib:llvm` with `^21`.

**What the build checks.** The shared checks of `../common/recipe.sh`:
everything under `usr/`, nothing in `lib64`, every program and library
the target's code, and what each child loads, reported. Then, installed
over what it was built with: `llvm-config` reports the release, a
shared build, and x86 as its target; `llc` compiles a program's IR to
an object that gcc links and that runs; and a program built against its
C API loads `libLLVM` from the library directory and reports its
version.

| File | What it is |
|---|---|
| `kairo.toml` | The source, and the target with what it builds with and its children. |
| `src/build.sh` | Builds it for the triple it is given, with `../common/recipe.sh`, which `import.sh` puts beside it. |
| `import.sh` | Archives the source and creates the object or its next revision, tagged with the release's version, with `../common/import.sh`. |

## Running it

Import 20.1.8, this directory with its version 20.1.8, and build it;
then this directory as the next revision:

```sh
packages/llvm/import.sh <your actor id>                       # 20.1.8
kairo build object:<object id> --target x86-64 --actor <your actor id>
packages/llvm/import.sh <your actor id> --wrapper <object id> # 21.1.8
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
