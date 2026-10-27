# live-bootstrap into its CI pass 2 (x86)

The archive's second bootstrap milestone (`specs/DECISIONS.md` §38):
[live-bootstrap](https://github.com/fosslinux/live-bootstrap) at
`b1ceced7`, run **unmodified** from the archive's stage0-posix x86
artifact (`bootstrap/stage0-posix/`) through Mes 0.27.1 and tcc 0.9.27
to make, patch, gzip, tar, sed, bzip2, coreutils, yacc, and bash 2.05b
(every step live-bootstrap builds with kaem, its minimal shell), then
under bash to tcc and musl 1.1.24 rebuilt against each other, grep, sed
and bzip2 rebuilt on musl, m4, flex, bison, diffutils, coreutils 6.10,
gawk, and perl 5.6.2 (built up through four older perls that
live-bootstrap then uninstalls), the autotools, binutils 2.30, gcc
4.0.4 (the first GCC built from the seeds) twice, musl 1.2.5, the Linux
headers, and util-linux, e2fsprogs, and the other base tools upstream's
own CI pass 1 ends with; then into its pass 2, through bash 5.2, curl,
xz, tar 1.34, coreutils 9.4, gmp, mpfr, mpc, and zlib, before its perl
chain. All of it runs under the bubblewrap engine, as root inside its
namespaces, with no host tool taking part.

From bash on, each step also packages what it installed and checks the
package against live-bootstrap's `SHA256SUMS.pkgs`; the artifact keeps
those packages in `repo/`, byte-for-byte upstream's, and each one
extracted under `<package>/`, the root of a **child object**
(`specs/DECISIONS.md` §41 sub-decision 6). A consumer that needs only
live-bootstrap's bash depends on `tool:bash` and gets the bash package,
installed at `/usr/bin/bash`, not the whole system; one that needs its
compiler takes `tool:gcc` and, as gcc requires of it, the musl it links
against, `lib:musl:static` or `lib:musl:shared` (`specs/DECISIONS.md`
§51), and gets binutils and the headers beside them. Each musl package
is split into those parts and its headers, children of their own.
The last revision of each package provides its name's label with its
version (`tool:autoconf` 2.52 through 2.69 are nine children); earlier
revisions that later steps rebuilt, such as `tcc-0.9.27_0` through
`_2`, are children reachable by id only. What only the kaem-run steps
built, which make no packages (Mes, patch, gzip), the parent provides
as before.

Every step checks its distfiles and what it built against the checksums
live-bootstrap publishes (`sha256sum -c`), so a successful build agrees
with upstream. The milestone is x86 because that is live-bootstrap's own
path: for AMD64 it publishes checksums only through Mes, and its tcc
steps target i386 (§38 sub-decision 7).

## How it's put together

live-bootstrap assumes it owns `/`: its scripts run from `/steps`,
install into `/usr`, and bake those paths into what they build. Under
`environment:linux/x86` (`schemas/environment/linux-v1.md`,
`DECISIONS.md` §41) the guest is root and `/` is writable, so every
input is placed at the absolute path live-bootstrap's own `rootfs.py`
puts it, and the wrapper's own files are in `/kairo/src`:

| Guest path | What | How it's pinned |
|---|---|---|
| `/steps` | live-bootstrap's `steps/`; `start.kaem` copies this directory's `steps/bootstrap.cfg` and truncated `steps/manifest` over them | source object, by commit |
| `/seed` → `/` | live-bootstrap's `seed/` files; `start.kaem` copies them to `/`, as `rootfs.py` does | source object, by commit |
| `/M2libc` | stage0-posix's M2libc, where `seed.kaem`'s `M2LIBC_PATH` points | source object, by commit |
| `/x86/bin` | stage0-posix 1.9.1's x86 tools, the artifact its wrapper builds; this replaces live-bootstrap's rerun of stage0 | artifact, via `tool:kaem` |
| `/external/distfiles` | the 108 distfiles the steps unpack | file sources, by SHA-256 |

| File | What it is |
|---|---|
| `kairo.toml` | The pins and placements above; one target, `x86`, whose artifact (installed at `/`) is the installed `usr/` tree, the checked packages in `repo/`, the `state/` a later pass needs, and each package extracted as a child, 83 in all (each musl package as two), providing `tool:bash`, `tool:gcc`, `lib:musl:static`, and the rest; the parent itself provides `tool:mes`, `tool:patch`, and `tool:gzip`, and the whole root as `data:live-bootstrap:x86:zlib-1.3.1`, which pass 2 goes on from. |
| `children.py` | Writes the `[[build.targets.children]]` entries from the package names a build lists, when the milestone moves (the later passes use it too); gives gcc what it requires beside it (binutils, musl's headers, the kernel headers, and `one_of` static or shared musl, chosen by whatever builds with it) and makes each musl package its headers, static, and shared children, only the shared one conferring itself (`specs/DECISIONS.md` §46, §51). |
| `hello/` | A consumer of the children: a C program that names `tool:gcc` and the musl it links against, gets binutils and the headers as what gcc requires, and prints the version of the gcc that built it (the highest held: 4.0.4 here, 4.7.4 once pass 3 is). Its `x86-static` target links against `lib:musl:static` and requires nothing at run time; its `x86` target links against `lib:musl:shared`, which pass 3 builds, and requires it. `hello/create.sh <actor id>` makes the object; the end-to-end test builds it last. |
| `start.kaem` | Stands in for the end of stage0-posix's `kaem.run`: puts the wrapper's step files over upstream's, sets `kaem.run`'s variables, copies the seed files to `/`, and runs live-bootstrap's `after.kaem`, which concatenates `bootstrap.cfg`, `env`, and `seed.kaem` and runs them. At the end it runs `package.sh`. |
| `package.sh` | Run under the bash the build made: copies `/usr` and `/external/repo` into `/kairo/package`, the artifact; copies `/steps/env`, `/steps/bootstrap.cfg`, and `/script-generator` into its `state/`; and extracts each package into `/kairo/package/<package>/`, musl's split by `split-musl.sh` into `<package>-headers`, `-static`, and `-shared`. |
| `split-musl.sh` | Lays out a musl package as the children linking needs of it (`specs/DECISIONS.md` §51); every pass's `package.sh` uses it. |
| `steps/bootstrap.cfg` | What `rootfs.py --arch x86 --bwrap --external-sources` writes. |
| `steps/manifest` | live-bootstrap's manifest, truncated after `zlib-1.3.1`, before the perl chain of upstream's CI pass 2. |
| `import.sh` | Archives live-bootstrap and the distfiles, and creates the wrapper. |

## Running it

Import the stage0-posix wrapper and build its `x86` target first
(`bootstrap/stage0-posix/README.md`). Then:

```sh
bootstrap/live-bootstrap/import.sh <your actor id>
kairo build object:<wrapper id> --target x86 --actor <your actor id>
```

`import.sh` runs `kairo source fetch` for live-bootstrap and `kairo
source fetch-file` for each distfile, trying its upstream mirrors in
order, and `ftp.gnu.org` after kernel.org's GNU mirror (this needs the
network), then commits the wrapper with fixed
author and dates and prints its id. The build needs the same
`[[runtime.seeds]]` and `[[runtime.engines]]` config as stage0-posix,
and, since it takes longer than the default hour on slower machines, a
larger `[runtime] timeout_secs`; on a slow connection the 100 MB Linux
tarball also needs a larger `[federation.limits] file_timeout_secs`.

To update a wrapper you already imported, after changing this
directory, pass its id: `import.sh <your actor id> --wrapper <wrapper
id>` commits the new files on top of the object's history and signs
its next revision, so one object goes on offering the wrapper's labels
instead of a second one offering the same labels beside it; an
unchanged wrapper adds nothing.

## Pass 2 and after

The chain goes on in passes (`specs/DECISIONS.md` §49), each its own
object, so a later milestone doesn't rebuild this one:
`bootstrap/live-bootstrap-perl/` installs this pass's whole root and
runs upstream's perl chain from the step after `zlib-1.3.1`,
`bootstrap/live-bootstrap-gcc-4.7.4/` goes on from that to gcc 4.7.4,
`bootstrap/live-bootstrap-python/` from that through the python chain,
guile, and autogen, `bootstrap/live-bootstrap-gcc-10.5.0/` from that
to gcc 10.5.0, and `bootstrap/live-bootstrap-gcc-15.2.0/` from that to
gcc 15.2.0 and the end of upstream's CI pass 2.
Every pass after this one shares `pass/start.sh` and `pass/package.sh`,
which its `import.sh` puts in its wrapper.

The full flow is the ignored test
`crates/kairo-cli/tests/bootstrap_live.rs`, which needs the network:

```sh
cargo test -p kairo-cli --test bootstrap_live -- --ignored
```
