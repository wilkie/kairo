# AGENTS.md — kairo-cli

## Adding a new command

For a top-level command (e.g. `kairo foo`):

1. **Define a `FooCommand` enum** with `#[derive(Debug, Subcommand)]`,
   one variant per subcommand. Each variant uses `#[arg(long)]` for
   every flag — positional args are reserved for the rare cases
   where they read more naturally (file paths to `bundle import`,
   etc.).

2. **Add a `Foo { command: FooCommand }` variant** to the top-level
   `Command` enum and dispatch it in `run`:
   `Some(Command::Foo { command }) => run_foo_command(command, &paths)`.

3. **Write `run_foo_command`** in `src/main.rs`. Pattern:
   - Parse string ids into typed ids via `ActorId::new(...).map_err(...)`.
   - Open the store / keystore via `open_store(paths)?` /
     `open_keystore(paths)?`.
   - For signing commands, fetch the actor body and verify
     `secret.public_key() == actor_body.initial_key()` — refuse with
     `KeyDoesNotMatchActor` otherwise.
   - Return `Result<String, CliError>`. The `String` is the rendered
     output; main prints it.

4. **Add error variants** to `CliError` for any new failure modes.
   Wire `Display` (with a clear, single-sentence message) and the
   `Error::source` arm. Group source-bearing variants by source type
   in the source impl (see existing `Self::OpenStore { source, .. } |
   Self::WriteActor { source, .. } | …` pattern).

5. **Update help text** in `help_text()` — add the new command lines
   in the existing flat list. Keep the synopsis line under ~120 chars.

6. **Write end-to-end tests** in the `tests` module at the bottom of
   `src/main.rs`. Pattern: drive the CLI via `run(Cli { ... })?`
   against a `tempfile::TempDir` store, parse output with
   `parse_field`, assert observable strings. Smoke-test happy path +
   one error path. Avoid pattern-matching the exact `CliError`
   variant in the happy path — the rendered string is what users see.

## Output rendering rules

- Plain output is `key = value` lines, one per line, lowercase
  underscored keys. Multi-line sections use `key:` on its own line
  followed by indented child lines. See `format_object_verification`
  for the canonical example.
- `--json` should produce a stable shape (`serde_json::json!({ ... })`
  with `to_string_pretty`). Never include trailing whitespace; always
  end with a `\n`.
- Errors that aggregate multiple lines (e.g. `AmbiguousLocalActor`
  listing candidates) are written via `writeln!` so each line is
  newline-terminated.

## Trust + signing semantics

- Signing always requires `--actor <id>`. Do not add an implicit
  "current actor" default — `memory/project_session_auth_future.md`
  explains why this is intentionally deferred.
- Trust is informational. `verify object` reports it as a separate
  field alongside the VALID/INVALID/INDETERMINATE verdict; it never
  changes the verdict. If you add a new command that consumes trust,
  preserve this separation.

## Capability commands

- `kairo capability grant` auto-chains via
  `CapabilityResolver::latest_capability` — if a chain leaf for the
  `(grantor, grantee, scope)` triple already exists, the new statement
  supersedes it; otherwise it's the genesis grant. This mirrors the
  trust auto-chain pattern; new commands that mint capability
  statements should follow it.
- `kairo capability revoke` enforces the v1 rule that only the grant's
  original grantor may revoke (`CAPABILITIES.md` §5.2). The CLI loads
  the named grant via `get_actor_capability_grant`, compares the
  grant's `actor()` to the `--grantor` flag, and errors with
  `CliError::RevokeWrongGrantor` on mismatch. Don't relax this in
  command surfaces that wrap revocation.
- Capability resolution is on the read path. `tag show` and
  `tag list` already honor cross-actor `supersedes` transparently
  (the resolver flip is in `kairo-store`, not the CLI). New commands
  that consume tag heads inherit this for free; do not add a
  separate "evaluate capability" command unless an audit query
  genuinely needs the structured `CapabilityEvaluation` enum.
