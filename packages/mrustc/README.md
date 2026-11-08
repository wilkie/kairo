# mrustc

mrustc 0.12.0, a Rust compiler written in C++, and the rustc 1.90.0 and
cargo it bootstraps, for x86-64 alone (`specs/DECISIONS.md` §65). It is
the first step of Rust built from source: no Rust compiler is
downloaded, and `../rust/`'s first release, 1.91.1, builds with what
this package makes.

**mrustc's own route.** mrustc emits C, which gcc compiles. This package
follows mrustc's `build-1.90.0.sh` and `run_rustc`:

1. mrustc and its minicargo are built with gcc, with one upstream fix
   made after 0.12.0, commit `aea331a4`
   (`src/mrustc-typeck-out-of-bounds.patch`): its type inference wrote
   past the end of a vector, which glibc's allocator tolerates and
   musl's doesn't, crashing mrustc on one of rustc's crates.
2. mrustc compiles rustc 1.90.0, its standard library, and cargo, from
   rustc's source with mrustc's patch for it.
3. That rustc builds the standard library, and rustc and cargo again,
   optimised, with cargo, and the standard library with them. It has no
   native thread-locals, as mrustc emits none, so each of its
   `thread_local!`s is a pthread key, and rustc 1.90.0 has more than
   musl's 128 (mrustc#400); it runs with `src/tls-keys.c` preloaded,
   which gives it a table of keys per thread under one musl key. The
   rustc it builds is rustc's own, with native thread-locals, and runs
   without it.

The last is the toolchain this package offers: the one mrustc's authors
build rustc 1.91.1 with and compare with the official 1.91.1.

**For the next step alone.** Its child offers `tool:rustc-bootstrap`
and `tool:cargo-bootstrap`, not `tool:rustc` or `tool:cargo`, so the
only build that takes it is `rust` 1.91.1's (§65 sub-decision 1).

**Built with packages alone.** It builds with `data:build-base`
(`../build-base/`); LLVM 20 (`../llvm/`, `lib:llvm` at `^20`), the
major rustc 1.90.0 bundles, not the copy in rustc's source, which its
rustc links statically; and what cargo links, found by pkgconf
(`../pkgconf/`): OpenSSL (`../openssl/`), curl (`../curl/`), libssh2
(`../libssh2/`), and zlib (`../zlib/`), not the OpenSSL and libssh2
its crates would build in; libgit2, which no package has, is its own.
Rustc's source carries every crate it builds with, so nothing is
fetched.

**For musl.** Everything is for the target Rust calls
`x86_64-unknown-linux-musl`, linked dynamically against the musl
package. Rust's musl target links statically by default, with the C
runtime and unwinder Rust ships beside its standard library, which this
toolchain hasn't; its default is changed in rustc's source, as Alpine's
is, so that everything, build scripts too, links dynamically.

**Its child.** `rust-bootstrap-1.90.0`, under
`/usr/lib/rust-bootstrap-1.90.0/`: `bin/rustc`, a wrapper that finds
rustc's libraries beside it, `bin/cargo`, and the standard library in
`lib/rustlib/x86_64-unknown-linux-musl/lib/`.

**What the build checks.** The shared checks of `../common/recipe.sh`.
Then, installed over what it was built with: rustc and cargo report
1.90.0, rustc builds a program that runs, and cargo builds a package
that runs.

| File | What it is |
|---|---|
| `kairo.toml` | mrustc's and rustc's sources, and the target with what it builds with and its child. |
| `src/build.sh` | Builds mrustc and the toolchain, with `../common/recipe.sh`, which `import.sh` puts beside it. |
| `src/mrustc-typeck-out-of-bounds.patch` | mrustc's commit `aea331a4`, as GitHub serves it. |
| `src/tls-keys.c` | More pthread keys than musl's 128, preloaded for the rustc mrustc compiles alone. |
| `import.sh` | Archives the sources and creates the object or its next revision, tagged `0.12.0`, with `../common/import.sh`. |

## Running it

```sh
packages/mrustc/import.sh <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
