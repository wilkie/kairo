# AGENTS.md

This repository is specification-led: substantial implementation now exists
in `crates/`, but the specs under `specs/` remain authoritative. Before
implementing or modifying behavior, read the relevant specs and keep
changes aligned with the decision log. When code disagrees with the spec,
the spec wins and the code is the bug — unless the change *is* a spec
update, in which case land both together.

## Primary Specs

Start with:

- `specs/OVERVIEW.md` - project purpose and system model.
- `specs/DECISIONS.md` - resolved choices where specs previously conflicted.
- `specs/GLOSSARY.md` - shared terminology.
- `specs/PROJECT_LAYOUT.md` - intended repository and crate layout.
- `specs/RUST.md` - Rust implementation conventions.

For identifier and reference work, read:

- `specs/IDENTIFIERS.md`
- `specs/OBJECT.md`
- `specs/OBJECT_STORE.md`

For core semantics, read:

- `specs/CORE_LIBRARY.md`
- `specs/STATEMENTS.md`
- `specs/ACTORS.md`
- `specs/CAPABILITIES.md`
- `specs/POLICY.md`
- `specs/SCHEMA.md`

For build, run, and execution behavior, read:

- `specs/BUILD.md`
- `specs/PLANNER.md`
- `specs/ENVIRONMENTS.md`
- `specs/SANDBOX.md`
- `specs/EXECUTOR.md`
- `specs/FORMATS.md`

For storage, packaging, federation, and search, read:

- `specs/STORE.md`
- `specs/PACKAGE.md`
- `specs/FEDERATION.md`
- `specs/SEARCH.md`

For user-facing surfaces, read:

- `specs/CLI.md`
- `specs/API.md`
- `specs/DAEMON.md`
- `specs/WEB_CLIENT.md`
- `specs/WORKSPACE.md`

`specs/CORE_LIBRARY_SPEC.md` is superseded by `specs/CORE_LIBRARY.md` and should
not override it.

For phase status and roadmap context, read:

- `specs/PHASE_1.md` — what shipped in the first implementation pass.
- `specs/PHASE_2.md` — current/next-up work, with completion checkboxes
  per section.

Per-crate `AGENTS.md` files under `crates/<crate>/AGENTS.md` describe
crate-specific implementation conventions (statement-type addition flow
in `kairo-statement`, store-index sharding rules in `kairo-store`, CLI
command pattern in `kairo-cli`, bundle invariants in `kairo-bundle`).
Read the relevant one before touching that crate.

## Implementation Rules

- Follow `specs/RUST.md` for Rust conventions.
- Use `kairo` as the CLI binary name.
- Use bare ID payloads in typed fields.
- Use internal typed references such as `object:<id>` when a string stands
  alone.
- Use external references such as `kairo:object:<id>` for federation,
  documentation, exported packages, and cross-system references.
- Reject unsupported identifier spellings instead of adding compatibility
  parsers.
- Keep semantic validation distinct from operational errors.
- Parse external inputs into strong types at boundaries.
- Prefer small, dependency-light core crates and acyclic crate dependencies.

## Editing Specs

- Update `specs/DECISIONS.md` when resolving a conflict between specs.
- Do not preserve contradictory notes as equally valid alternatives.
- Keep examples consistent with `specs/IDENTIFIERS.md`.
- Treat files ending in `:Zone.Identifier` as non-spec sidecar files.

## Verification

When Rust code exists, the expected local checks are:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

When editing only specs, scan for stale identifier forms and inconsistent
terminology before finishing.
