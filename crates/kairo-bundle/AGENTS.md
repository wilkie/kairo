# AGENTS.md — kairo-bundle

## Fixity is the entire point

Every record in a bundle is fixity-checked on import: ids are
re-derived from canonical bytes; blob hashes are recomputed against
`OBJECT_MANIFEST_DOMAIN`. A mismatch is a hard import failure, never
silently repaired. If you add a new code path that bypasses
re-derivation, you have introduced a way for a tampered bundle to
land in the store undetected — fix the bypass.

## What goes in an object bundle, and what does not

**Goes in:** `ObjectGenesis` for the root object; every
`ObjectRevision` / `ObjectBranch` / `ObjectVersionTag` whose
`body.object()` matches the root; every actor that signed any of
those statements; every blob those statements reference.

**Does not go in:** `ActorTrust` statements. Trust is first-person;
shipping it inside an object package would invite reading peers'
opinions as authority. Adding `ActorTrust` to object bundles would
silently break that boundary — don't.

If a future statement type's body has no `object` field but is
relevant to objects (e.g. an attestation about a revision), think
carefully about whether it belongs in object bundles or its own
bundle type before adding it. The "what goes in" rule should remain
mechanically derivable from the body shape, not from per-type
bespoke logic.

## Adding a new statement type to bundles

1. Extend `EnvelopePeek`'s match arms in `src/export.rs` to dispatch
   to the right typed DTO and select on the body's object reference.
2. Add a typed map to `CollectedStatements` and write its files in
   the appropriate loop.
3. Mirror the import side in `src/import.rs` — add the matching arm
   to the dispatcher and call the right `put_*` method on the store.
4. Add tests covering: round-trip; the new statement type appears in
   `manifest.contents.statements`; an exclusion case if the new type
   is intentionally filtered.

## When extending the manifest schema

`schema = "kairo.bundle.v1"` is a fixed string in MVP. Adding new
**optional** fields (`#[serde(default)]`) is backward-compatible and
should not require a schema bump. Adding new **required** fields, or
changing the meaning of existing fields, requires a `v2` schema and
import-side migration. Document the choice in the manifest module
top-of-file `//!` comment.

The `git_history` field is the future-extension hook: today
`included = false` always; a future bundle version will flip it to
`true` and add a `git/` subdirectory carrying a Git pack, plus
import-side ingestion into the planned `~/.kairo/git/` managed
mirror. The manifest schema does not need to change for that — only
new optional fields under `git_history` and a new code path triggered
by `included = true`.
