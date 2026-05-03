# kairo-bundle

Portable directory-form export of an object's Kairo statements,
signing actors, and referenced blobs. `write_bundle` walks a
`FilesystemStore` for one root object and produces:

```text
manifest.json
actors/<actor-id>.json
objects/<object-id>.json
statements/<statement-id>.json
blobs/<blob-id>
```

`import_bundle` ingests a bundle into a destination store with
**fixity at every step** — every record's id is re-derived from its
canonical bytes; every blob's hash is recomputed; mismatches abort
the import rather than being silently repaired. Re-importing the same
bundle is idempotent.

`ActorTrust` statements are intentionally excluded from object
bundles — trust is first-person and shipping it inside an object
package would invite reading peers' opinions as authority.

Git history is **not** carried in MVP bundles. The manifest declares
`git_history.expected_commits` so recipients know which commits to
obtain externally; a future bundle version will flip
`git_history.included = true`, ship a Git pack, and ingest it into
the (planned) `~/.kairo/git/` managed mirror on import.

**Position in the dependency stack:** sits above `kairo-core`,
`kairo-identity`, `kairo-object`, `kairo-statement`, and
`kairo-store`. Depended on by `kairo-cli`.

**Read more:** crate-level docs in `src/lib.rs`, `specs/PACKAGE.md`
("MVP slice (current implementation)" subsection), TODO §9, and
`memory/project_bundle_design.md`.
