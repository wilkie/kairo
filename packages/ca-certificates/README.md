# ca-certificates

The certificate authorities Mozilla's NSS 3.119.1 trusts to identify servers, as one bundle in `/etc/ssl`. One object for every release, each a version tag on its
history (`specs/DECISIONS.md` §59), with a target for each environment
it is built for, `x86` and `x86-64`, and no environment of its own
(§55). This revision is 3.119.1, tagged `3.119.1`.

**Built with packages alone.** It builds with `data:build-base`
(`../build-base/`), and perl, which runs curl's `mk-ca-bundle.pl`, `tool:perl`, whose labels only releases satisfy (§61): no
bootstrap's object takes part, as none is a release.

**Its child.** Each target's artifact is installed nowhere; it is its
child, `ca-certificates-3.119.1`, offering `data:ca-certificates`, and requiring nothing.

**What the build checks.** The shared checks of `../common/recipe.sh`:
everything under `usr/` or `etc/`, nothing in `lib64`, every program the
target's code, and what each child loads, reported. Then, installed over
what it was built with, that it works.

| File | What it is |
|---|---|
| `kairo.toml` | The sources, and the two targets with what they build with and their child. |
| `src/build.sh` | Builds it for the triple it is given, with `../common/recipe.sh`, which `import.sh` puts beside it. |
| `import.sh` | Archives the sources and creates the object or its next revision, tagged `3.119.1`, with `../common/import.sh`. |

## Running it

```sh
packages/ca-certificates/import.sh <your actor id>
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
