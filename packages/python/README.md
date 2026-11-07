# python

Python, without pip, with its shared library in the library directory. One object for every release, each a version tag on its
history (`specs/DECISIONS.md` §59), with a target for each environment
it is built for, `x86` and `x86-64`, and no environment of its own
(§55). This revision is 3.11.1, tagged `3.11.1`: its third, which has
`ssl`, `hashlib`'s OpenSSL digests, `sqlite3`, `curses`, `readline`,
`dbm.gnu` and `dbm.ndbm`, and `uuid`'s libuuid, and supersedes the
earlier ones, tagged `3.11.1` too (§61). It lacks only `tkinter` and
`nis` of the optional modules.

**Built with packages alone.** It builds with `data:build-base`
(`../build-base/`) and the libraries it builds against, `lib:zlib`, `lib:ffi`, `lib:lzma`, `lib:bz2`, `lib:ssl` (`../openssl/`),
`lib:sqlite3` (`../sqlite/`), `lib:ncurses` (`../ncurses/`),
`lib:readline` (`../readline/`), `lib:gdbm` (`../gdbm/`), and `lib:uuid`
(`../util-linux/`), each a package of its own, whose labels only releases satisfy (§61): no
bootstrap's object takes part, as none is a release.

**Its child.** Each target's artifact is installed nowhere; it is its
child, `python-3.11.1`, offering `tool:python`, and requiring `lib:ffi`, `lib:zlib`, `lib:bz2`, `lib:lzma`, `lib:ssl`, `lib:crypto`,
`lib:sqlite3`, `lib:ncurses`, `lib:readline`, `lib:gdbm`, `lib:uuid`,
`lib:musl:shared`, what its
programs and libraries load.

**What the build checks.** The shared checks of `../common/recipe.sh`:
everything under `usr/`, nothing in `lib64`, every program the target's
code, and what each child loads, reported. Then, installed over what it
was built with, that it works: that its modules load, `ssl`,
`sqlite3`, and `readline` with the packages' versions, `curses` reading
the terminfo database, `dbm.gnu` storing and fetching, and that `ssl` finds the
certificate authorities of `data:ca-certificates`
(`../ca-certificates/`), which the build has for that alone, where
OpenSSL looks for them.

| File | What it is |
|---|---|
| `kairo.toml` | The source, and the two targets with what they build with and their child. |
| `src/build.sh` | Builds it for the triple it is given, with `../common/recipe.sh`, which `import.sh` puts beside it. |
| `import.sh` | Archives the source and creates the object or its next revision, tagged `3.11.1`, with `../common/import.sh`. |

## Running it

Its earlier revisions are this directory as it was in commits
`1a4cfcf` and `a845034`; this one is its next revision, with what it
builds against imported first:

```sh
packages/python/import.sh <your actor id> --wrapper <object id>
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
