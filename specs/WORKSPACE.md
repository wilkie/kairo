# WORKSPACE.md

## Status

Draft specification.

This document defines the Kairo local workspace model: the Git-like working
directory interface used by authors when creating and modifying Kairo objects.

The workspace model is intentionally familiar:

```text
working directory -> staging area -> commit -> signed statements -> snapshot
```

But the underlying semantics are Kairo-specific:

```text
Kairo commit != Git commit
Kairo commit = signed semantic statements
```

---

## 1. Purpose

A Kairo workspace lets an author develop an object using normal local files.

The workspace is responsible for:

1. Tracking local file changes.
2. Staging file additions, modifications, and removals.
3. Mapping file paths to object artifacts.
4. Hashing file content into immutable blobs.
5. Generating statement intents for commit.
6. Supporting `kairo status`, `kairo add`, `kairo rm`, and `kairo commit`.
7. Maintaining local workspace metadata.

The workspace is not responsible for:

1. Core validation semantics.
2. Long-term object store layout.
3. Federation.
4. Runtime execution.
5. Authority evaluation beyond preparing commit inputs.

---

## 2. Core Principle

```text
Filesystem paths are the default artifact identities.
```

For ordinary files:

```text
main.c -> artifact path "main.c"
src/foo.c -> artifact path "src/foo.c"
```

Authors do not need to assign separate artifact names for normal files.

Logical or generated artifacts may still have explicit names in higher-level object
metadata, but the default authoring model is path-based.

---

## 3. Workspace Initialization

Command:

```bash
kairo init
```

Creates a Kairo object workspace in the current directory.

Required files/directories:

```text
kairo.toml
.kairo/
```

Recommended initial layout:

```text
.kairo/
  workspace.json
  index.json
  staged/
  blobs/
  refs/
  logs/
```

`kairo init` must not initialize the global daemon store. Global store setup is
handled by daemon/store commands or daemon startup.

---

## 4. `kairo.toml`

`kairo.toml` is the human-editable object/workspace manifest.

It may include:

```toml
[object]
name = "Example Object"
description = "A Kairo object."

[workspace]
ignore = [".git/", "target/", "dist/"]

[build.default]
# build declarations may be added later
```

The exact object schema is defined by `OBJECT.md`.

`kairo.toml` itself should normally be tracked as part of the object unless ignored.

---

## 5. `.kairo/` Directory

`.kairo/` contains local workspace metadata.

It is not itself part of the object unless explicitly exported for debugging.

### 5.1 `workspace.json`

Stores workspace-local metadata.

Recommended shape:

```json
{
  "schema": "kairo.workspace.v1",
  "object_id": "z6MkObject...",
  "created_at": "2026-04-30T00:00:00Z",
  "default_actor": "z6MkActor...",
  "head": {
    "snapshot_id": "z6MkSnapshot...",
    "frontier": []
  }
}
```

### 5.2 `index.json`

The staging index.

It records the staged artifact state that will be committed.

Recommended shape:

```json
{
  "schema": "kairo.workspace.index.v1",
  "entries": {
    "main.c": {
      "status": "added",
      "blob_id": "z6MkBlob...",
      "size": 123,
      "mtime": "2026-04-30T00:00:00Z"
    }
  }
}
```

Index entries are workspace-local and not authoritative object history.

### 5.3 `blobs/`

Workspace-local content-addressed blob cache.

Recommended layout:

```text
.kairo/blobs/sha256/ab/abcdef...
```

These blobs may later be imported into the local store.

### 5.4 `refs/`

Local references to selected snapshots/frontiers.

Recommended examples:

```text
.kairo/refs/head.json
.kairo/refs/last-commit.json
```

### 5.5 `logs/`

Local command logs and diagnostics.

Logs are not semantic object history.

---

## 6. Path Normalization

Artifact paths must be normalized before staging.

Rules:

1. Paths are relative to workspace root.
2. Use `/` as separator internally.
3. Reject absolute paths.
4. Reject paths containing `..` after normalization.
5. Reject paths inside `.kairo/` unless explicitly allowed.
6. Normalize `./main.c` to `main.c`.
7. Preserve case where the host filesystem preserves case.

Examples:

```text
./main.c -> main.c
src/../main.c -> main.c
/abs/path -> rejected
../outside.c -> rejected
.kairo/index.json -> ignored/rejected by default
```

---

## 7. Ignore Rules

Kairo should support ignore files similar to Git.

Recommended file:

```text
.kairoignore
```

Ignore sources, in precedence order:

1. Command-line explicit include/exclude flags.
2. `.kairoignore`.
3. `kairo.toml` workspace ignore rules.
4. Built-in defaults.

Built-in defaults should include:

```text
.kairo/
.git/
node_modules/
target/
dist/
.DS_Store
```

### 7.1 Ignore semantics

Ignored files:

1. Do not appear as untracked by default.
2. Are not added by `kairo add .`.
3. May be force-added with an explicit flag, if supported.

Recommended force-add:

```bash
kairo add --force path
```

---

## 8. Working Tree Status

Command:

```bash
kairo status
```

Status compares three states:

1. Current filesystem working tree.
2. Staging index.
3. Current object snapshot/head.

The output should distinguish:

```text
staged changes
unstaged modified files
untracked files
removed files
ignored files (only when requested)
```

Recommended output:

```text
On object z6MkObject...

Changes to be committed:
  added:    main.c
  modified: kairo.toml

Changes not staged:
  modified: src/foo.c
  deleted:  old.c

Untracked:
  notes.txt
```

### 8.1 Status categories

Recommended categories:

```text
added
modified
deleted
renamed
untracked
ignored
staged
unchanged
conflicted
```

Rename detection is optional. If not implemented, a rename may appear as delete +
add.

---

## 9. Add / Stage

Commands:

```bash
kairo add <path>
kairo add .
```

Behavior:

1. Normalize path.
2. Apply ignore rules.
3. Read file content.
4. Hash content.
5. Store blob in workspace blob cache.
6. Update staging index.
7. Mark artifact path as added or modified.

For directories, `kairo add` recurses by default.

### 9.1 Add file

```bash
kairo add main.c
```

Stages:

```text
artifact path: main.c
blob: z6MkBlob...
```

### 9.2 Add directory

```bash
kairo add src/
```

Stages all non-ignored files under `src/`.

### 9.3 Add all

```bash
kairo add .
```

Stages all non-ignored changes in the workspace.

### 9.4 No implicit commit

`kairo add` must not create object-history statements visible as committed history.

It only stages local intent.

---

## 10. Remove

Command:

```bash
kairo rm <path>
```

Behavior:

1. Normalize path.
2. Remove file from working tree unless `--cached` is supplied.
3. Stage artifact removal.

Recommended options:

```text
--cached   stage removal from object but keep local file
--force    remove even if modified
```

Removal creates a committed artifact-removal statement only after `kairo commit`.

---

## 11. Commit

Command:

```bash
kairo commit -m "message"
```

Commit consumes the staging index and creates signed statements.

A commit must:

1. Verify staging index is non-empty.
2. Verify default actor/key is available.
3. Convert staged changes into statement bodies.
4. Link statements to previous actor statement.
5. Include required causal parents.
6. Sign statements.
7. Persist statements to local workspace/store.
8. Advance local workspace head/frontier.
9. Clear committed staging entries.

### 11.1 Commit is not a Git commit

A Kairo commit is a user-facing command that produces one or more signed Kairo
statements.

It is not a filesystem snapshot object.

### 11.2 Statement generation

Staged file changes should produce statements such as:

```text
SetArtifactPathBlob(path, blob_id)
RemoveArtifactPath(path)
UpdateObjectMetadata(...)
```

The exact statement kinds are defined by `STATEMENTS.md`.

### 11.3 Commit message

Commit messages are metadata.

They may be included in a statement or statement group record.

Commit messages are not a substitute for structured statement semantics.

### 11.4 Empty commit

Empty commits should be rejected by default.

Optional:

```bash
kairo commit --allow-empty -m "message"
```

---

## 12. Artifact Path State

The effective object state maps artifact paths to blob IDs.

Conceptual state:

```json
{
  "artifacts": {
    "main.c": {
      "blob_id": "z6MkBlob...",
      "format": {
        "kind": "source",
        "media_type": "text/x-c"
      }
    }
  }
}
```

Format detection may occur during add or commit, but explicit metadata should
override detection.

---

## 13. Directory Representation

MVP behavior:

```text
Each tracked file is an artifact path.
```

Directory artifacts are optional and may be introduced later.

For MVP:

```text
src/foo.c
src/bar.c
```

are separate path artifacts.

Future optimization may represent trees as Merkle tree objects.

---

## 14. Workspace Head

The workspace tracks a selected current snapshot/frontier.

This is analogous to a Git HEAD but semantically different.

Recommended file:

```text
.kairo/refs/head.json
```

Contains:

```json
{
  "schema": "kairo.workspace.ref.v1",
  "snapshot_id": "z6MkSnapshot...",
  "frontier": []
}
```

The head is local workspace state. It is not itself authoritative object history.

---

## 15. Relationship to Store

The workspace may maintain a local blob cache and local statement files.

The daemon/local store may later ingest these records.

Valid implementation options:

1. Workspace writes directly to local store through provider APIs.
2. Workspace keeps local `.kairo/` records and syncs to store on commit.
3. Workspace operates standalone until imported/published.

Regardless of implementation, committed statements must be valid Kairo statements.

---

## 16. Relationship to Git

A Kairo workspace may coexist with Git.

Kairo may optionally use Git as:

- a working tree helper
- a diff helper
- a blob cache backend

But Kairo must not depend on Git commits as semantic object history.

Kairo source of truth remains:

```text
signed statements + blobs + snapshots
```

---

## 17. Example Workflow

```bash
mkdir hello-kairo
cd hello-kairo

cat > main.c <<'EOF'
#include <stdio.h>

int main(void) {
  printf("Hello from Kairo!\n");
  return 0;
}
EOF

cat > Makefile <<'EOF'
hello: main.c
	cc -O2 -o hello main.c
EOF

kairo init
kairo status

kairo add main.c Makefile kairo.toml
kairo commit -m "Initial C project"

kairo verify --purpose build
kairo build
kairo run
```

After editing:

```bash
vim main.c
kairo status
kairo add main.c
kairo commit -m "Update greeting"
```

---

## 18. Security Requirements

Workspace implementation must:

1. Reject path traversal.
2. Reject absolute artifact paths.
3. Avoid following unsafe symlinks by default.
4. Ignore `.kairo/` by default.
5. Not execute files during add/status/commit.
6. Treat file content as untrusted bytes.
7. Preserve committed blob hashes.
8. Avoid leaking private local paths into statements unless explicitly requested.
9. Protect private signing keys.
10. Require explicit signing on commit.

---

## 19. Symlinks

Initial MVP recommendation:

```text
Do not follow symlinks by default.
```

Options:

1. Store symlink as symlink metadata.
2. Follow symlink only with explicit flag.
3. Reject symlink if target leaves workspace.

Recommended command:

```bash
kairo add --follow-symlinks path
```

if support is needed.

---

## 20. File Metadata

MVP should track content, not full filesystem metadata.

Optional metadata:

- executable bit
- media type
- format descriptor
- declared role
- size

Avoid storing host-specific metadata unless explicitly needed.

---

## 21. Status Performance

Implementations may use:

- cached file mtimes
- file sizes
- content hashes
- filesystem watchers
- Git integration as optimization

But final status for modified files should be based on content hash when accuracy
matters.

---

## 22. Implementation Checklist

A conforming initial implementation should provide:

1. `kairo init` workspace creation.
2. `.kairo/` metadata directory.
3. `kairo.toml` creation.
4. Path normalization.
5. `.kairoignore` support.
6. Blob hashing.
7. Workspace blob cache.
8. Staging index.
9. `kairo status`.
10. `kairo add <path>`.
11. `kairo add .`.
12. `kairo rm <path>`.
13. `kairo commit -m`.
14. Statement generation from staged changes.
15. Actor signing integration.
16. Workspace head/frontier update.
17. Safe symlink/path handling.
18. Tests for ignored files, path traversal, staging, and commit.

---

End of `WORKSPACE.md`.
