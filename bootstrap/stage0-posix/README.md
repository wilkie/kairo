# stage0-posix bootstrap (AMD64)

The archive's first bootstrap (`specs/DECISIONS.md` §34 sub-decision 9,
§36): [stage0-posix](https://github.com/oriansj/stage0-posix)
`Release_1.9.1`, imported as two Kairo objects and built under the
bubblewrap engine (§35) from 847 bytes of seed to M2-Planet,
mescc-tools, and mescc-tools-extra, with no host tool taking part.

| File | What it is |
|---|---|
| `bootstrap-seeds.kairo.toml` | The seed object's manifest: provides `seed:stage0-posix/amd64` as content. |
| `stage0-posix.kairo.toml` | The sources object's manifest: one target, `amd64`, and the tools its `bin` output provides. |
| `kairo.kaem` | The three lines of upstream's `kaem.amd64`, run by the seed `kaem`. |
| `import.sh` | Imports both objects from a local checkout through the `kairo` CLI. |

## Running it

```sh
git clone --branch Release_1.9.1 --recurse-submodules \
  https://github.com/oriansj/stage0-posix.git
bootstrap/stage0-posix/import.sh stage0-posix <your actor id>
```

`import.sh` checks the checkout is the pinned release, commits the seed
and the flattened sources as SHA-256 Git history with fixed author and
dates (so a re-import gives the same revisions), creates and branches both objects, and prints
their ids and the config lines to add:

```toml
[[runtime.seeds]]
object = "<seed object id>"   # after auditing POSIX/AMD64/*-seed against their .hex0 sources

[[runtime.engines]]
kind = "bubblewrap"
path = "/usr/bin/bwrap"
```

Then:

```sh
kairo build object:<sources object id> --target amd64 --actor <your actor id>
```

runs phases 0–23 in an empty root (about 20 seconds), ending in
upstream's `sha256sum -c amd64.answers`, and signs a `BuildResult` for
the `bin` artifact. `kairo reproduce statement:<result id>` re-runs it
and should report `matches`.

The full flow is the ignored test
`crates/kairo-cli/tests/bootstrap_stage0.rs`:

```sh
KAIRO_STAGE0_DIR=/path/to/stage0-posix \
  cargo test -p kairo-cli --test bootstrap_stage0 -- --ignored
```
