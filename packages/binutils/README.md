# binutils

GNU binutils for the machine it runs on, the version the bootstraps build, with deterministic archives and without gprofng, which doesn't compile under C23.

One object for every release of GNU binutils, each a version tag on its
history (`specs/DECISIONS.md` §59), with a target for each environment
it is built for, `x86` and `x86-64`, and no environment of its own
(§55). This revision is 2.41, tagged `2.41.0`, as a tag is a full
semver version.

**Its first generation.** It builds with `data:bootstrap-base`
(`../bootstrap-base/`), whose labels only the bootstraps' objects may
satisfy: live-bootstrap's in x86, the 64-bit root's in x86-64. Built
with the other packages of its round, it would be a cycle. A later
release builds with `data:build-base` (`../build-base/`), and may use
this one, an older generation of itself.

**Its children.** Each target's artifact is installed nowhere; it is
its children: `binutils-2.41`, offering `tool:binutils`, and `tool:binutils:<triple>` (`i686-linux-musl`, `x86_64-linux-musl`), for its own machine (§53). Each child requires what
its programs and libraries load, which the build reports (§56).

**What the build checks.** Every child is under `usr/`, nothing is in
`lib64`, every program and library is the target's code, and, installed
over what it was built with, `ld --version` names it, and gcc assembles and links a program with it that runs.

| File | What it is |
|---|---|
| `kairo.toml` | The source, and the two targets with what they build with and their children. |
| `src/build.sh` | Builds it for the triple it is given, lays out its children, and checks them. |
| `import.sh` | Archives the source, creates the object or its next revision with `../../bootstrap/wrapper.sh`, and tags it 2.41.0. |

## Running it

Import `../bootstrap-base/` and the bootstraps it names. Then:

```sh
packages/binutils/import.sh <your actor id>
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
