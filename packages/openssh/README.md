# openssh

OpenSSH, its client, its server, and their tools, with OpenSSL and zlib, configured in `/etc/ssh`. One object for every release, each a version tag on its
history (`specs/DECISIONS.md` §59), with a target for each environment
it is built for, `x86` and `x86-64`, and no environment of its own
(§55). This revision is 10.6p2, tagged `10.6.2`, the version its child
declares too, as a version is semver.

**Its server.** `sshd` is built, and checked to load its configuration
and a host key; it can't log anyone in during the build, whose root
maps no other user for its privilege separation. The artifact holds no
host keys, no `sshd` user, and no privilege-separation directory
(`/usr/share/empty.sshd`, empty, so not in the artifact): whatever runs
the server makes them.

**Built with packages alone.** It builds with `data:build-base`
(`../build-base/`) and OpenSSL (`../openssl/`) and zlib (`../zlib/`), `lib:ssl` and `lib:zlib`, which it builds against, whose labels only releases satisfy (§61): no
bootstrap's object takes part, as none is a release.

**Its child.** Each target's artifact is installed nowhere; it is its
child, `openssh-10.6p2`, offering `tool:ssh`, `tool:sshd`, `tool:openssh`, and requiring `lib:crypto`, `lib:zlib`, `lib:musl:shared`, what its programs and libraries load.

**What the build checks.** The shared checks of `../common/recipe.sh`:
everything under `usr/` or `etc/`, nothing in `lib64`, every program the
target's code, and what each child loads, reported. Then, installed over
what it was built with, that it works.

| File | What it is |
|---|---|
| `kairo.toml` | The sources, and the two targets with what they build with and their child. |
| `src/build.sh` | Builds it for the triple it is given, with `../common/recipe.sh`, which `import.sh` puts beside it. |
| `import.sh` | Archives the sources and creates the object or its next revision, tagged `10.6.2`, with `../common/import.sh`. |

## Running it

```sh
packages/openssh/import.sh <your actor id>
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
