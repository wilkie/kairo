# stage0-posix bootstrap (AMD64 and x86)

The archive's first bootstrap (`specs/DECISIONS.md` §34 sub-decision 9,
§36, §37): [stage0-posix](https://github.com/oriansj/stage0-posix)
`Release_1.9.1`, built under the bubblewrap engine (§35) from 847 bytes
of seed to M2-Planet, mescc-tools, and mescc-tools-extra, with no host
tool taking part, for AMD64 and for x86 (§38 sub-decision 7). The x86
build is the input to the live-bootstrap wrapper
(`bootstrap/live-bootstrap/`).

Every upstream repository is archived faithfully as a **source object**,
identified by its URL, with upstream's own commits: stage0-posix, the
submodules it pins (the seeds, M2-Planet, M2libc, mescc-tools,
mescc-tools-extra, M2-Mesoplanet, the per-architecture repositories),
and theirs in turn. This directory holds only the **native wrapper**
that builds them, born in the archive with SHA-256 history:

| File | What it is |
|---|---|
| `kairo.toml` | Pins every source the chains share at its Release_1.9.1 commit, placed at its upstream path under `stage0/` (nested ones too: mescc-tools' own `M2libc` is a different commit from stage0-posix's); marks `bootstrap-seeds` as the seed; two targets, `amd64` and `x86`, each adding its architecture's repository and environment (`environment:linux/x86-64`, `environment:linux/x86`), and the tools each `bin` output provides. The resolver picks the one built for the consumer's architecture (§38 sub-decision 8). |
| `kairo-amd64.kaem`, `kairo-x86.kaem` | The three lines of upstream's `kaem.amd64` and `kaem.x86`, run by the seed `kaem`. |
| `import.sh` | Archives the sources from upstream and creates the wrapper. |

## Running it

```sh
bootstrap/stage0-posix/import.sh <your actor id>
```

runs `kairo source fetch https://github.com/oriansj/stage0-posix
--require-complete`, which archives stage0-posix and, recursively, every
submodule commit it pins (this needs the network; if Savannah is
unreachable, mescc-tools comes from its GitHub mirror, and its
observation names the mirror), then commits the
wrapper with fixed author and dates (so a re-import gives the same
revision) and prints its id and the config lines to add:

```toml
[[runtime.seeds]]   # after auditing POSIX/{AMD64,x86}/*-seed against their .hex0 sources
source = "https://github.com/oriansj/bootstrap-seeds"
revision = "git:sha1:cedec6b8066d1db229b6c77d42d120a23c6980ed"

[[runtime.engines]]
kind = "bubblewrap"
path = "/usr/bin/bwrap"
```

Then:

```sh
kairo build object:<wrapper id> --target amd64 --actor <your actor id>
kairo build object:<wrapper id> --target x86 --actor <your actor id>
```

each copies the pinned sources into place, runs phases 0–23 in an empty
root (about 20 seconds), ending in upstream's `sha256sum -c
<arch>.answers`, and signs a `BuildResult` for the `bin` artifact. The
x86 target runs i386 code, so it needs an x86-64 kernel with IA32
emulation; bubblewrap offers `environment:linux/x86` only when its
probe shows that. `kairo reproduce
statement:<result id>` re-runs it and should report `matches`.

The full flow is the ignored test
`crates/kairo-cli/tests/bootstrap_stage0.rs`, which needs the network:

```sh
cargo test -p kairo-cli --test bootstrap_stage0 -- --ignored
```
