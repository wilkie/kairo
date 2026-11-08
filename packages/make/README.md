# make

GNU make, the version the bootstraps build. Built without guile, as C17 with `-fpermissive`, since 4.2.1 predates what gcc 15 rejects.

One object for every release of GNU make, each a version tag on its
history (`specs/DECISIONS.md` §59), with a target for each environment
it is built for, `x86` and `x86-64`, and no environment of its own
(§55). This revision is 4.2.1, its fourth round, tagged `4.2.1+4`; its earlier rounds are tagged `4.2.1+3`, `4.2.1+2`, and `4.2.1`.

**Its fourth round.** It builds with `data:base-round` 3
(`../base-round/`), which brings what `build-base` does, each label
offered only at the snapshot of the base's round 3 (§64). Round 3 was
built with round 2, round 2 with round 1, and round 1, tagged without build metadata, with
`../bootstrap-base/`, the bootstraps' packages. Built with the other
packages of its own round, through `build-base`, it would be a cycle;
built with the round before, it never builds with itself. Everything
outside the base builds with `build-base` and gets the newest round.

**Its children.** Each target's artifact is installed nowhere; it is
its children: `make-4.2.1`, offering `tool:make`. Each child requires what
its programs and libraries load, which the build reports (§56).

**What the build checks.** Every child is under `usr/`, nothing is in
`lib64`, every program and library is the target's code, and, installed
over what it was built with, `make --version` names it, and it runs a makefile.

| File | What it is |
|---|---|
| `kairo.toml` | The source, and the two targets with what they build with and their children. |
| `src/build.sh` | Builds it for the triple it is given, with `../common/recipe.sh`, which `import.sh` puts beside it, lays out its children, and checks them. |
| `import.sh` | Archives the source, creates the object or its next revision with `../common/import.sh`, and tags it 4.2.1. |

## Running it

Import `../base-round/` and the base's round 3 it names. Then:

```sh
packages/make/import.sh <your actor id>
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
