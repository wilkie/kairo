# git

Git, with HTTP and HTTPS remotes by curl and Perl-compatible grep, with its Perl parts, without its Python and Rust parts. One object for every release, each a version tag on its
history (`specs/DECISIONS.md` §59), with a target for each environment
it is built for, `x86` and `x86-64`, and no environment of its own
(§55). This revision is 2.56.0, tagged `2.56.0`: its second, with its
Perl parts, `send-email` among them, run by the perl package, superseding
the first, tagged `2.56.0` too, which had none (§61). `git svn` is
installed but needs Subversion's Perl bindings, which no package has.

**Built with packages alone.** It builds with `data:build-base`
(`../build-base/`) and the libraries it builds against, curl (`../curl/`), expat (`../expat/`), PCRE2 (`../pcre2/`), OpenSSL (`../openssl/`), and zlib (`../zlib/`), and perl (`../perl/`), `lib:curl`, `lib:expat`, `lib:pcre2`, `lib:ssl`, `lib:zlib`, and `tool:perl`, whose labels only releases satisfy (§61): no
bootstrap's object takes part, as none is a release.

**Its child.** Each target's artifact is installed nowhere; it is its
child, `git-2.56.0`, offering `tool:git`, and requiring `lib:curl`, `lib:expat`, `lib:pcre2`, `lib:ssl`, `lib:crypto`, `lib:zlib`, `lib:musl:shared`, what its programs and libraries load, and `tool:perl`, which runs its Perl parts.

**What the build checks.** The shared checks of `../common/recipe.sh`:
everything under `usr/` or `etc/`, nothing in `lib64`, every program the
target's code, and what each child loads, reported. Then, installed over
what it was built with, that it works.

| File | What it is |
|---|---|
| `kairo.toml` | The sources, and the two targets with what they build with and their child. |
| `src/build.sh` | Builds it for the triple it is given, with `../common/recipe.sh`, which `import.sh` puts beside it. |
| `import.sh` | Archives the sources and creates the object or its next revision, tagged `2.56.0`, with `../common/import.sh`. |

## Running it

```sh
packages/git/import.sh <your actor id> --wrapper <object id>
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
