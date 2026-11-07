# curl

curl and libcurl, shared, in the library directory, with OpenSSL, nghttp2, libpsl, libidn2, zlib, zstd, and brotli, trusting the ca-certificates bundle. One object for every release, each a version tag on its
history (`specs/DECISIONS.md` §59), with a target for each environment
it is built for, `x86` and `x86-64`, and no environment of its own
(§55). This revision is 8.23.0, tagged `8.23.0`: its second, with
HTTP/2, the public suffix list, international domain names, and brotli,
superseding the first, tagged `8.23.0` too, which had none of them
(§61). It lacks only SSH and LDAP, for which no package is built yet.

**Built with packages alone.** It builds with `data:build-base`
(`../build-base/`) and the libraries it builds against, OpenSSL (`../openssl/`), zlib (`../zlib/`), zstd (`../zstd/`), brotli (`../brotli/`), nghttp2
(`../nghttp2/`), libpsl (`../libpsl/`), and libidn2 (`../libidn2/`), with pkgconf (`../pkgconf/`) to find them, `lib:ssl`, `lib:zlib`, `lib:zstd`, `lib:brotli`, `lib:nghttp2`,
`lib:psl`, `lib:idn2`, and `tool:pkgconf`, whose labels only releases satisfy (§61): no
bootstrap's object takes part, as none is a release.

**Its child.** Each target's artifact is installed nowhere; it is its
child, `curl-8.23.0`, offering `lib:curl`, `tool:curl`, and requiring `lib:ssl`, `lib:crypto`, `lib:zlib`, `lib:zstd`, `lib:brotli`, `lib:nghttp2`, `lib:psl`,
`lib:idn2`, `lib:musl:shared`, what its programs and libraries load.

**What the build checks.** The shared checks of `../common/recipe.sh`:
everything under `usr/` or `etc/`, nothing in `lib64`, every program the
target's code, and what each child loads, reported. Then, installed over
what it was built with, that it works: that it has each library, at
the package's version, reads a `file://` URL, and makes a request over
TLS to `openssl s_server` on the loopback.

| File | What it is |
|---|---|
| `kairo.toml` | The sources, and the two targets with what they build with and their child. |
| `src/build.sh` | Builds it for the triple it is given, with `../common/recipe.sh`, which `import.sh` puts beside it. |
| `import.sh` | Archives the sources and creates the object or its next revision, tagged `8.23.0`, with `../common/import.sh`. |

## Running it

```sh
packages/curl/import.sh <your actor id> --wrapper <object id>
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
