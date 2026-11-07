# gawk

GNU awk, with `/usr/bin/awk` a link to it. One object for every release, each a version tag on its
history (`specs/DECISIONS.md` §59), with a target for each environment
it is built for, `x86` and `x86-64`, and no environment of its own
(§55). This revision is 5.3.0, its second round, tagged `5.3.0+2`; its first round is tagged `5.3.0`.

**Its second round.** It builds with `data:base-round` 1
(`../base-round/`), which brings what `build-base` does, each label
offered only at the snapshot of the base's first round (§64): this
package's first round, tagged without `+2`, was built with
`../bootstrap-base/`, the bootstraps' packages. Built with the other
packages of its own round, through `build-base`, it would be a cycle;
built with the round before, it never builds with itself. Everything
outside the base builds with `build-base` and gets this round.

**Its child.** Each target's artifact is installed nowhere; it is its
child, `gawk-5.3.0`, offering `tool:gawk`, `tool:awk`, and requiring `lib:musl:shared`, what its programs load.

**What the build checks.** The shared checks of `../common/recipe.sh`:
everything under `usr/`, nothing in `lib64`, every program the target's
code, and what each child loads, reported. Then, installed over what it
was built with, that it works.

| File | What it is |
|---|---|
| `kairo.toml` | The source, and the two targets with what they build with and their child. |
| `src/build.sh` | Builds it for the triple it is given, with `../common/recipe.sh`, which `import.sh` puts beside it. |
| `import.sh` | Archives the source and creates the object or its next revision, tagged `5.3.0`, with `../common/import.sh`. |

## Running it

```sh
packages/gawk/import.sh <your actor id>
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
