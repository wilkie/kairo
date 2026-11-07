# python

Python, with pip, its shared library in the library directory. One
object for every release, each a version tag on its history
(`specs/DECISIONS.md` §59), with a target for each environment it is
built for, `x86` and `x86-64`, and no environment of its own (§55).

| Release | Tag | Recipe |
|---|---|---|
| Python 3.11.1 | `3.11.1` | the object's revision at its tag, this directory as it was in commit `81d0da8`: without pip, its modules for OpenSSL, SQLite, ncurses, readline, gdbm, and libuuid, each from its package, superseding two earlier revisions tagged `3.11.1` that had fewer (§61) |
| Python 3.15.2 | `3.15.2` | this directory |

Only releases are offered by label (§61), so `tool:python` is the
newest, 3.15.2; a build can still ask for `^3.11` to have 3.11.1.

**Built with packages alone.** It builds with `data:build-base`
(`../build-base/`) and the libraries it builds against, each a package
of its own, whose labels only releases satisfy (§61): `lib:zlib`,
`lib:ffi`, `lib:lzma`, `lib:bz2`, `lib:ssl` (`../openssl/`),
`lib:sqlite3` (`../sqlite/`), `lib:ncurses` (`../ncurses/`),
`lib:readline` (`../readline/`), `lib:gdbm` (`../gdbm/`), `lib:uuid`
(`../util-linux/`), `lib:expat` (`../expat/`), `lib:mpdec`
(`../mpdecimal/`), and `lib:zstd` (`../zstd/`). No bootstrap's object
takes part, as none is a release, and none of the copies of libraries
Python bundles is built: expat and mpdecimal are the packages'.

**Its child.** Each target's artifact is installed nowhere; it is its
child, `python-3.15.2`, offering `tool:python`, and requiring each of
those libraries, `lib:crypto` and `lib:musl:shared` too, what its
programs and modules load.

**pip.** `ensurepip` installs pip 26.2.2 from the wheel Python's source
bundles, fetching nothing, as `pip3` beside `python3`; `venv` installs
it in a new environment from the same wheel. Of the optional modules
it lacks only `tkinter`, for which no Tk is built.

**What the build checks.** The shared checks of `../common/recipe.sh`:
everything under `usr/`, nothing in `lib64`, every program the target's
code, and what each child loads, reported. Then, installed over what it
was built with, that it works: that its modules load, `ssl`, `sqlite3`,
`readline`, `pyexpat`, `decimal`, and `compression.zstd` with the
packages' versions, `curses` reading the terminfo database, `dbm.gnu`
storing and fetching, and that `ssl` finds the certificate authorities
of `data:ca-certificates` (`../ca-certificates/`), which the build has
for that alone, where OpenSSL looks for them; and that pip runs, with
the installed python, and in a new `venv`.

| File | What it is |
|---|---|
| `kairo.toml` | The source, and the two targets with what they build with and their child. |
| `src/build.sh` | Builds it for the triple it is given, with `../common/recipe.sh`, which `import.sh` puts beside it. |
| `import.sh` | Archives the source and creates the object or its next revision, tagged with the release's version, with `../common/import.sh`. |

## Running it

Import 3.11.1 from this directory as it was in commit `81d0da8`, with
what it builds against, and build it; then import this directory, with
expat, mpdecimal, and zstd, as the next revision:

```sh
packages/python/import.sh <your actor id>                       # at 3.11.1
packages/python/import.sh <your actor id> --wrapper <object id> # 3.15.2
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
