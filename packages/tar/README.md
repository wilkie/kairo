# tar

GNU tar. One object for every release, each a version tag on its
history (`specs/DECISIONS.md` §59), with a target for each environment
it is built for, `x86` and `x86-64`, and no environment of its own
(§55). This revision is 1.34, tagged `1.34.0`, as a tag is a full semver version.

**Its first generation.** It builds with `data:bootstrap-base`
(`../bootstrap-base/`), whose labels only the bootstraps' objects may
satisfy. A later release builds with `data:build-base`, once every
package it names is a release (§61).

**Its child.** Each target's artifact is installed nowhere; it is its
child, `tar-1.34`, offering `tool:tar`, and requiring `lib:musl:shared`, what its programs load.

**What the build checks.** The shared checks of `../common/recipe.sh`:
everything under `usr/`, nothing in `lib64`, every program the target's
code, and what each child loads, reported. Then, installed over what it
was built with, that it works.

| File | What it is |
|---|---|
| `kairo.toml` | The source, and the two targets with what they build with and their child. |
| `src/build.sh` | Builds it for the triple it is given, with `../common/recipe.sh`, which `import.sh` puts beside it. |
| `import.sh` | Archives the source and creates the object or its next revision, tagged `1.34.0`, with `../common/import.sh`. |

## Running it

```sh
packages/tar/import.sh <your actor id>
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
