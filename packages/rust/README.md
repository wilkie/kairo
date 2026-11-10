# rust

Rust, rustc and cargo, built from source for x86-64 alone
(`specs/DECISIONS.md` §65). One object for every release, each a
version tag on its history, each built by the release before it (§59),
with a target for x86-64 and no environment of its own (§55).

| Release | Tag | Built by |
|---|---|---|
| Rust 1.91.1 | `1.91.1` | mrustc's rustc 1.90.0 and cargo (`../mrustc/`), the first Rust built from source |
| Rust 1.91.1, again | `1.91.1+2` | 1.91.1 itself: the double compile (§65 sub-decision 3), whose artifacts must be 1.91.1's, byte for byte |
| Rust 1.92.0 | `1.92.0` | 1.91.1 |
| Rust 1.93.1 | `1.93.1` | 1.92.0 |
| Rust 1.94.1 | `1.94.1` | 1.93.1 |
| Rust 1.95.0 | `1.95.0` | 1.94.1 |
| Rust 1.96.1 | `1.96.1` | 1.95.0 |
| Rust 1.97.1 | `1.97.1` | 1.96.1 |
| Rust 1.98.1 | `1.98.1` | 1.97.1 |
| Rust 1.99.1 | `1.99.1` | 1.98.1 |

This revision is 1.99.1; the object's revision at each other tag is
this directory as it was for that release, its stage 0 and tag the
difference.

**Built with packages alone.** It builds with `data:build-base`
(`../build-base/`); the release before, mrustc's `tool:rustc-bootstrap`
`~1.90` for 1.91.1, and this object's own `tool:rustc` for each later; LLVM 21 (`../llvm/`, `lib:llvm` and
`lib:llvm:shared` at `^21`), which every release from 1.91 to 1.99
accepts (§65 sub-decision 4), linked shared; Python (`../python/`),
which runs Rust's build, `x.py`; and what cargo links, found by pkgconf
(`../pkgconf/`): OpenSSL (`../openssl/`), curl (`../curl/`), libssh2
(`../libssh2/`), and zlib (`../zlib/`), not the copies its crates would
build in; libgit2, which no package has, is its own. rustc's source
carries every crate it builds with, so nothing is fetched.

**As a distribution builds it.** `x.py` is configured, in
`src/build.sh`, for the target Rust calls `x86_64-unknown-linux-musl`
alone, installed under `/usr`, its libraries in the library directory
(§60) without rpaths, without documentation or debug information. The
musl target's default is changed in rustc's source, as Alpine's is, to
link programs dynamically against the musl package (§65 sub-decision
6); musl's `libc.a` and startup files are copied beside the standard
library, as Rust does, for programs that ask to be linked statically.

**Its child.** `rust-<version>`: `rustc`, `rustdoc`, and `cargo` in
`/usr/bin`, rustc's libraries and the standard library in the library
directory, offering `tool:rustc`, `tool:cargo`, and `tool:rustdoc`.

**What the build checks.** The shared checks of `../common/recipe.sh`.
Then, installed over what it was built with: rustc reports the release,
the musl host, and LLVM 21, and `/usr` as its sysroot; it builds a
program that runs, linked dynamically against musl; and cargo builds a
package with a procedural macro, which rustc loads as a library, that
runs.

| File | What it is |
|---|---|
| `kairo.toml` | The source, and the target with what it builds with and its child. |
| `src/build.sh` | Configures and runs `x.py`, with `../common/recipe.sh`, which `import.sh` puts beside it. |
| `import.sh` | Archives the source and creates the object or its next revision, tagged with the release's version, with `../common/import.sh`. |

## Running it

```sh
packages/rust/import.sh <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
