# musl

The C library, the version the bootstraps build, in the environment's library directory (§60): its libraries and startup files there, its loader in `/usr/lib` as a relative link to the directory's `libc.so`, `ldd` a link to the loader, and `/etc/ld-musl-<arch>.path` naming the directory, so the loader searches it alone. Its startup files are linked in `/usr/lib` too, as live-bootstrap's are: a gcc running from its build tree, as one bootstrapping does, looks for them only there and in `/lib`.

One object for every release of musl, each a version tag on its
history (`specs/DECISIONS.md` §59), with a target for each environment
it is built for, `x86` and `x86-64`, and no environment of its own
(§55). This revision is 1.2.5, tagged `1.2.5`.

**Its first generation.** It builds with `data:bootstrap-base`
(`../bootstrap-base/`), whose labels only the bootstraps' objects may
satisfy: live-bootstrap's in x86, the 64-bit root's in x86-64. Built
with the other packages of its round, it would be a cycle. A later
release builds with `data:build-base` (`../build-base/`), and may use
this one, an older generation of itself.

**Its children.** Each target's artifact is installed nowhere; it is
its children: `musl-1.2.5-headers`, `-static`, `-shared`, offering `lib:musl:headers`; `lib:musl:static`; `lib:musl:shared`, conferring `^1.2.5` and requiring the merged-/usr links, through which its loader is reached as `/lib` (§51). Each child requires what
its programs and libraries load, which the build reports (§56).

**What the build checks.** Every child is under `usr/`, nothing is in
`lib64`, every program and library is the target's code, and, installed
over what it was built with, a program compiled against it, static and dynamic, runs.

| File | What it is |
|---|---|
| `kairo.toml` | The source, and the two targets with what they build with and their children. |
| `src/build.sh` | Builds it for the triple it is given, lays out its children, and checks them. |
| `import.sh` | Archives the source, creates the object or its next revision with `../../bootstrap/wrapper.sh`, and tags it 1.2.5. |

## Running it

Import `../bootstrap-base/` and the bootstraps it names. Then:

```sh
packages/musl/import.sh <your actor id>
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
