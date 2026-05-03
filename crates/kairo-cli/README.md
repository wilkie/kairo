# kairo-cli

The `kairo` binary. A clap-based CLI that drives every other crate in
the workspace via direct/local mode (no daemon yet). Commands are
grouped: `actor`, `object`, `revision`, `manifest`, `branch`, `tag`,
`trust`, `bundle`, `snapshot`, `verify`.

The CLI is a thin orchestrator: parsing inputs, opening the store and
keystore, dispatching to the underlying crate, and rendering output
(plain text by default, `--json` for stable machine-readable shape on
commands that support it). Every signing command requires
`--actor <id>` and refuses to sign if the keystore key does not match
the actor's initial public key.

`verify object` is the end-to-end entrypoint, rolling up genesis
fixity, signature, actor resolution, object consistency, manifest
binding, content-layer (Git), and trust evaluation into a single
`VALID` / `INDETERMINATE` / `INVALID` verdict.

**Position in the dependency stack:** sits above every other
workspace crate.

**Read more:** crate-level docs in `src/main.rs`, `specs/CLI.md` for
the command catalog and rendering rules, and `examples/README.md`
for the end-to-end MVP walkthrough.
