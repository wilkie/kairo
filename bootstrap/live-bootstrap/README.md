# live-bootstrap through its CI pass 1 (x86)

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
own CI pass 1 ends with. All of it runs under the bubblewrap engine, as
root inside its namespaces, with no host tool taking part.

From bash on, each step also packages what it installed and checks the
package against live-bootstrap's `SHA256SUMS.pkgs`; the artifact keeps
those packages as its `repo` output, byte-for-byte upstream's.

Every step checks its distfiles and what it built against the checksums
live-bootstrap publishes (`sha256sum -c`), so a successful build agrees
with upstream. The milestone is x86 because that is live-bootstrap's own
path: for AMD64 it publishes checksums only through Mes, and its tcc
steps target i386 (§38 sub-decision 7).

## How it's put together

live-bootstrap assumes it owns `/`: its scripts run from `/steps`,
install into `/usr`, and bake those paths into what they build. So the
build runs under `environment:linux-root/x86`
(`schemas/environment/linux-root-v1.md`), where the build tree is the
guest's root, and every input is placed in it where live-bootstrap's own
`rootfs.py` puts it:

| Guest path | What | How it's pinned |
|---|---|---|
| `/steps` | live-bootstrap's `steps/`, under this directory's `steps/bootstrap.cfg` and truncated `steps/manifest` (`overlay = true`, so the wrapper's files win) | source object, by commit |
| `/seed` → `/` | live-bootstrap's `seed/` files; `start.kaem` copies them to `/`, as `rootfs.py` does | source object, by commit |
| `/M2libc` | stage0-posix's M2libc, where `seed.kaem`'s `M2LIBC_PATH` points | source object, by commit |
| `/x86/bin` | stage0-posix 1.9.1's x86 tools, the artifact its wrapper builds; this replaces live-bootstrap's rerun of stage0 | artifact, via `tool:kaem` |
| `/external/distfiles` | the 67 distfiles the steps unpack | file sources, by SHA-256 |

| File | What it is |
|---|---|
| `kairo.toml` | The pins and placements above; one target, `x86`, whose outputs are the installed `usr/` tree, providing `tool:mes`, `tool:tcc`, `tool:make`, `tool:bash`, `lib:musl`, and the rest, and the checked packages in `repo/`. |
| `start.kaem` | Stands in for the end of stage0-posix's `kaem.run`: sets its variables, copies the seed files to `/`, and runs live-bootstrap's `after.kaem`, which concatenates `bootstrap.cfg`, `env`, and `seed.kaem` and runs them. |
| `steps/bootstrap.cfg` | What `rootfs.py --arch x86 --bwrap --external-sources` writes. |
| `steps/manifest` | live-bootstrap's manifest, truncated before `jump: break`, the end of upstream's CI pass 1. |
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

The full flow is the ignored test
`crates/kairo-cli/tests/bootstrap_live.rs`, which needs the network:

```sh
cargo test -p kairo-cli --test bootstrap_live -- --ignored
```
