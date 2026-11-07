# python

Python, without pip, with its shared library in the library directory. One object for every release, each a version tag on its
history (`specs/DECISIONS.md` §59), with a target for each environment
it is built for, `x86` and `x86-64`, and no environment of its own
(§55). This revision is 3.11.1, tagged `3.11.1`: its second, which has
`ssl`, `hashlib`'s OpenSSL digests, and `sqlite3`, and supersedes the
first, tagged `3.11.1` too, which had none of them (§61).

**Built with packages alone.** It builds with `data:build-base`
(`../build-base/`) and the libraries it builds against, `lib:zlib`, `lib:ffi`, `lib:lzma`, `lib:bz2`, `lib:ssl` (`../openssl/`),
and `lib:sqlite3` (`../sqlite/`), each a package of its own, whose labels only releases satisfy (§61): no
bootstrap's object takes part, as none is a release.

**Its child.** Each target's artifact is installed nowhere; it is its
child, `python-3.11.1`, offering `tool:python`, and requiring `lib:ffi`, `lib:zlib`, `lib:bz2`, `lib:lzma`, `lib:ssl`, `lib:crypto`,
`lib:sqlite3`, `lib:musl:shared`, what its
programs and libraries load.

**What the build checks.** The shared checks of `../common/recipe.sh`:
everything under `usr/`, nothing in `lib64`, every program the target's
code, and what each child loads, reported. Then, installed over what it
was built with, that it works: that its modules load, `ssl` and
`sqlite3` with the packages' versions, and that `ssl` finds the
certificate authorities of `data:ca-certificates`
(`../ca-certificates/`), which the build has for that alone, where
OpenSSL looks for them.

| File | What it is |
|---|---|
| `kairo.toml` | The source, and the two targets with what they build with and their child. |
| `src/build.sh` | Builds it for the triple it is given, with `../common/recipe.sh`, which `import.sh` puts beside it. |
| `import.sh` | Archives the source and creates the object or its next revision, tagged `3.11.1`, with `../common/import.sh`. |

## Running it

Its first revision is this directory as it was in commit `1a4cfcf`; this one
is its next revision, with OpenSSL, SQLite, and ca-certificates
imported and built first:

```sh
packages/python/import.sh <your actor id> --wrapper <object id>
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
