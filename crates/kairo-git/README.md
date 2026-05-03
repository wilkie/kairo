# kairo-git

Read-only Git repository access wrapping `gix`. Provides
`Repository::discover` / `open`, `find_commit` (returning commit OID +
parents), and `read_blob_at_path` (used by the verifier to read
`kairo.toml` from a commit's tree).

The crate intentionally exposes **no write operations** — Kairo never
mutates the user's Git repo. Future managed-mirror work
(`~/.kairo/git/`, see TODO §11 + §9 follow-up) will live in a separate
crate or behind a clearly-named module so the read-only contract here
stays obvious.

**Position in the dependency stack:** sits above `kairo-statement`
(for the `RevisionId` type used in error messages). Depended on by
`kairo-cli`.

**Read more:** crate-level docs in `src/lib.rs` and `specs/OBJECT.md`
on the content-layer model the verifier consumes.
