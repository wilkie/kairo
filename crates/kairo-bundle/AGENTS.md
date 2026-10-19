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
`ObjectBranch` / `ObjectVersionTag` whose `body.object()` matches the
root; every `ObjectRevision` those refs and the genesis's initial
revision reach along parent links (`specs/DECISIONS.md` §20, as in a
Git fetch; export reads `FilesystemStore::object_closure` rather than
walking `statements/`); the `ActorGenesis` of every actor
that signed any of those statements; every blob those statements
reference.

Actor key events (rotations, revocations, emergency and attestation
kinds) deliberately stay out. Key knowledge belongs to the actor,
not the object, and may already be known to the importing node or
have changed since export. The importer is responsible for getting
it, for example from a federation peer's
`/federation/v1/actors/{id}` (`specs/DECISIONS.md` §16). Don't add
key events to object bundles to make one self-verifying.

**Does not go in:** `ActorTrust` statements (trust is first-person —
shipping it inside an object package would invite reading peers'
opinions as authority); `ActorCapabilityGrant` /
`ActorCapabilityRevocation` statements (capabilities are also
first-person speech acts — the grantor authorizes from their own
voice, and bundling grants with object data would mix authority
transport with object transport). The "first-person speech acts go in
their own bundle type" rule is the same boundary in both cases.
Adding any of these to object bundles would silently break that
boundary — don't.

The export-side dispatcher in `src/export.rs` filters by
`body.object() == target`, which incidentally excludes capability and
trust statements (they don't have an `object` field). Don't rely on
that incidental exclusion as the safety net — keep the explicit
"does not go in" list above as the doctrine, so future statement
types are evaluated against the boundary intentionally.

If a future statement type's body has no `object` field but is
relevant to objects (e.g. an attestation about a revision), think
carefully about whether it belongs in object bundles or its own
bundle type before adding it. The "what goes in" rule should remain
mechanically derivable from the body shape, not from per-type
bespoke logic.

## Closure bundles are their own type

`src/closure.rs` is the transfer form of a *published closure*
(`specs/DECISIONS.md` §26 sub-decision 8), which a push uploads. It
carries authority (grants, revocations) and every actor's key events,
because it stands in for a pull from a node that may not be reachable,
and a pull fetches those too. That is the "own bundle type" the rules
above anticipate; it doesn't change what object bundles carry.

- It may be partial: the manifest's inventory names the whole closure,
  the files are what the receiver asked for.
- Nothing in `closure.rs` verifies records. Ingest is the federation
  pull verifier reading from the unpacked bundle; don't add a second,
  weaker import path for it.
- `unpack_tar` accepts only regular files at the layout's exact paths,
  named by valid ids. Keep it that strict.

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
