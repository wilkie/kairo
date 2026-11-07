# readline

GNU readline, with its six official patches, shared, in the library directory. One object for every release, each a version tag on its
history (`specs/DECISIONS.md` §59), with a target for each environment
it is built for, `x86` and `x86-64`, and no environment of its own
(§55). This revision is 8.3.6, tagged `8.3.6`.

**Built with packages alone.** It builds with `data:build-base`
(`../build-base/`) and ncurses (`../ncurses/`), `lib:ncurses`, which it links, whose labels only releases satisfy (§61): no
bootstrap's object takes part, as none is a release.

**Its child.** Each target's artifact is installed nowhere; it is its
child, `readline-8.3.6`, offering `lib:readline`, and requiring `lib:ncurses`, `lib:musl:shared`, what its programs and libraries load.

**What the build checks.** The shared checks of `../common/recipe.sh`:
everything under `usr/` or `etc/`, nothing in `lib64`, every program the
target's code, and what each child loads, reported. Then, installed over
what it was built with, that it works.

| File | What it is |
|---|---|
| `kairo.toml` | The sources, and the two targets with what they build with and their child. |
| `src/build.sh` | Builds it for the triple it is given, with `../common/recipe.sh`, which `import.sh` puts beside it. |
| `import.sh` | Archives the sources and creates the object or its next revision, tagged `8.3.6`, with `../common/import.sh`. |

## Running it

```sh
packages/readline/import.sh <your actor id>
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
