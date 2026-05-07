# PACKAGE.md

## Status

Draft specification.

This document defines the Kairo package format: a portable import/export bundle
for moving objects, snapshots, statements, blobs, execution records, and archival
closures between Kairo stores, nodes, users, and long-term archives.

This specification is intentionally prescriptive enough to guide implementation.

### MVP slice (current implementation)

`crates/kairo-bundle` and the `kairo bundle export | import` CLI ship a
strict subset of this spec:

- One package type only: an **object bundle** corresponding to §4.1
  (`Object package`) — ObjectGenesis + every known
  ObjectRevision/ObjectBranch/ObjectVersionTag for the root object,
  every signing actor's ActorGenesis, and every referenced blob
  (currently just the canonical manifest blob per revision).
- **`ActorTrust` statements are intentionally excluded** from object
  bundles. Trust is first-person; bundling opinions inside an object
  package would invite reading them as authority. A separate
  trust-bundle type may land later.
- **Directory format only** (§5.1). Tar, gzip, and the deterministic
  rules in §17 are not implemented; users tar/compress the directory
  with their own tooling if they need single-file transport.
- **No Git history in the bundle.** The manifest declares the Git
  commit ids the bundle's `ObjectRevision` statements name in a new
  `git_history` field (`{ "included": false, "expected_commits":
  [...] }`); recipients must obtain those commits separately to reach
  end-to-end `VALID`. A future bundle version flips
  `git_history.included = true`, adds a `git/` subdirectory carrying
  the Git pack, and ingests it into a `~/.kairo/git/` managed cache
  on import.
- **No bundle-level signature** (§24). Every signed statement inside
  is independently verifiable on its own bytes; the manifest is an
  inventory, not an authority claim.
- **Manifest schema** is the §7.1 shape minus the `closure`,
  `executions`, `provenance`, and `indexes` sections, plus
  `git_history`. `manifest.schema = "kairo.bundle.v1"` (note: the
  field name is intentionally narrower than `kairo.package.v1` —
  bundle is the MVP slice; the full package spec gets its own schema
  string when it lands).
- **Fixity is hard.** Importer re-derives every id from canonical
  bytes; mismatches abort the import. Idempotent re-import is a no-op
  at the byte level (same id + same bytes overwrites same content).

---

## 1. Purpose

A Kairo package is a portable bundle of Kairo data.

Packages are used for:

1. Exporting objects from a local store.
2. Importing objects into another local store.
3. Sharing snapshot closures.
4. Preserving reproducible build/run inputs.
5. Creating offline archives.
6. Moving object data without federation.
7. Publishing static archival bundles.
8. Preserving execution records and outputs.

A package is a transport and preservation container. It does not itself prove
semantic validity.

Core validation remains the responsibility of the core library.

---

## 2. Relationship to Other Specs

Packages interact with:

- `OBJECT.md` for object records.
- `STATEMENTS.md` for signed statement records.
- `BUILD.md` for build declarations referenced by snapshots.
- `PLANNER.md` for planner-related declarations.
- `CORE_LIBRARY.md` for validation semantics.
- `STORE.md` for import/export persistence.
- `DAEMON.md` for package import/export orchestration.
- `CLI.md` for `kairo import` and `kairo export`.
- `API.md` for import/export endpoints.
- `EXECUTOR.md` for execution records and generated outputs.
- `POLICY.md` for import, export, and publication permissions.

Packages must not define independent object validity semantics.

---

## 3. Design Principles

### 3.1 Data container, not authority

A package says:

```text
Here is data.
```

The core library decides:

```text
This data is valid, invalid, conflicted, or indeterminate.
```

### 3.2 Safe import

Importing a package must never execute code, build artifacts, launch runtimes,
start emulators, or run scripts.

### 3.3 Snapshot-first portability

The most important package type is the snapshot closure package: a bundle that
contains enough data to validate, build, run, or reproduce a selected snapshot.

### 3.4 Content-addressed blobs

Large or binary content must be stored content-addressed where possible.

### 3.5 Deterministic export

Exporting the same selected data with the same package options should produce
equivalent package contents. When canonical archive mode is requested, output
should be bitwise deterministic.

### 3.6 Partial packages are allowed

Packages may be complete or partial. Completeness must be declared.

Partial packages are valid packages, but may produce indeterminate validation.

---

## 4. Package Types

```rust
pub enum PackageType {
    Object,
    SnapshotClosure,
    ArchiveMirror,
    ExecutionRecord,
    Mixed,
}
```

### 4.1 Object package

An object package contains object records, statement records, and optionally blobs.

Use cases:

- Sharing source/history.
- Moving an object between stores.
- Preserving development state.

### 4.2 Snapshot closure package

A snapshot closure package contains a selected snapshot frontier and all available
data needed for a declared purpose.

Use cases:

- Reproducible runs.
- Scientific result preservation.
- Offline validation.
- Build/run transfer.

This is the recommended default package type for export.

### 4.3 Archive mirror package

An archive mirror package contains the full known object log and all known blobs
for one or more objects.

Use cases:

- Cold storage.
- Institutional archive.
- Offline mirror.
- Full audit.

### 4.4 Execution record package

An execution record package contains an execution record, related snapshot data,
logs, outputs, and environment metadata.

Use cases:

- Sharing “what was run.”
- Reproducible science.
- Build attestation.
- Result preservation.

### 4.5 Mixed package

A mixed package contains multiple package roots or data categories.

Use cases:

- Project-level export.
- Collection export.
- Multi-object dataset.

---

## 5. Canonical v1 Format

Kairo package v1 defines two equivalent representations:

1. Directory package.
2. Archive package.

### 5.1 Directory package

A directory package is the canonical unpacked representation.

```text
example.kairo-package/
  manifest.json
  objects/
  statements/
  snapshots/
  blobs/
  artifacts/
  executions/
  indexes/
  provenance/
```

### 5.2 Archive package

An archive package is a serialized transport form of the directory package.

Recommended extension:

```text
.kairo
```

Recommended v1 archive format:

```text
POSIX tar archive, optionally gzip-compressed
```

Recommended extensions:

```text
.kairo        canonical uncompressed tar or implementation-default archive
.kairo.tar    explicit tar
.kairo.tgz    gzip-compressed tar
```

Implementations may support zip, but tar should be the reference format for
deterministic export.

### 5.3 MIME type

Recommended provisional MIME type:

```text
application/vnd.kairo.package
```

---

## 6. Directory Layout

A v1 directory package should use this layout:

```text
manifest.json

objects/
  <object_id>.json

statements/
  <object_id>/
    <statement_id>.json

snapshots/
  <snapshot_id>.json

blobs/
  <algo>/
    <prefix>/
      <hash>

artifacts/
  <artifact_id>.json

executions/
  <execution_id>/
    record.json
    logs/
    outputs/

indexes/
  objects.json
  snapshots.json
  blobs.json

provenance/
  package.json
```

Only `manifest.json` is always required. Other directories appear as needed.

---

## 7. Manifest

Every package must contain `manifest.json` at the package root.

### 7.1 Manifest schema

Recommended shape:

```json
{
  "schema": "kairo.package.v1",
  "package_type": "snapshot_closure",
  "package_id": "z6MkPackage...",
  "created_at": "2026-04-30T00:00:00Z",
  "created_by": {
    "tool": "kairo",
    "version": "0.1.0"
  },
  "roots": {
    "objects": ["z6MkObject..."],
    "snapshots": ["z6MkSnapshot..."],
    "executions": []
  },
  "closure": {
    "purpose": "run",
    "status": "claimed_closed"
  },
  "contents": {
    "objects": [],
    "statements": [],
    "snapshots": [],
    "artifacts": [],
    "blobs": [],
    "executions": []
  },
  "indexes": {
    "objects": "indexes/objects.json",
    "snapshots": "indexes/snapshots.json",
    "blobs": "indexes/blobs.json"
  },
  "provenance": "provenance/package.json"
}
```

### 7.2 Required manifest fields

A manifest must include:

1. `schema`
2. `package_type`
3. `package_id`
4. `created_by`
5. `roots`
6. `contents`

`created_at` is recommended but not semantically authoritative.

### 7.3 Manifest semantics

The manifest is an inventory and navigation aid.

The manifest does not prove package validity.

The manifest does not prove object validity.

The manifest may be signed in a future version, but package signatures are
separate from object statement signatures.

---

## 8. Closure Metadata

Packages that contain snapshots must declare closure metadata.

```json
{
  "closure": {
    "purpose": "run",
    "status": "claimed_closed",
    "snapshot_id": "z6MkSnapshot...",
    "includes_blobs": true,
    "includes_dependencies": true
  }
}
```

### 8.1 Closure status values

```text
claimed_closed
partial
full_object_log
unknown
```

Definitions:

- `claimed_closed`: exporter claims the package includes required data for the declared purpose.
- `partial`: exporter knows required data is missing.
- `full_object_log`: package includes full known object log for root object(s).
- `unknown`: exporter does not know closure completeness.

Core must verify closure sufficiency during validation.

### 8.2 Snapshot purpose values

```text
inspect
build
run
reproduce
archive_mirror
```

These match `CORE_LIBRARY.md`.

---

## 9. Object Records

Object records are stored as:

```text
objects/<object_id>.json
```

The serialized form must preserve all required object fields and unknown fields
needed for round-tripping where supported.

Object records must not be rewritten in a way that changes their canonical identity.

---

## 10. Statement Records

Statements are stored as:

```text
statements/<object_id>/<statement_id>.json
```

Each statement record must preserve:

1. Statement ID.
2. Object ID.
3. Actor ID.
4. Actor sequence.
5. Previous actor statement reference.
6. Causal parents.
7. Statement kind/body.
8. Body hash.
9. Signature.
10. Version/feature information.

Statement signatures must remain byte-for-byte valid after package import/export.

Exporters must not normalize statement payloads in a way that changes signature
verification.

---

## 11. Snapshot Records

Snapshots are stored as:

```text
snapshots/<snapshot_id>.json
```

A snapshot record should include:

```json
{
  "snapshot_id": "z6MkSnapshot...",
  "object_id": "z6MkObject...",
  "frontier": [
    {
      "actor_id": "z6MkActor...",
      "statement_id": "z6MkStatement...",
      "actor_seq": 12
    }
  ],
  "purpose": "run",
  "dependencies": []
}
```

Snapshot records are selectors and closure descriptors. They do not by themselves
prove validity.

---

## 12. Blob Storage

Blobs are stored content-addressed.

Recommended path:

```text
blobs/<algorithm>/<first-two-hash-chars>/<full-hash>
```

Example:

```text
blobs/sha256/ab/abcdef1234...
```

### 12.1 Blob metadata

Blob metadata may be stored in the manifest or separate index files.

Recommended blob entry:

```json
{
  "blob_id": "z6MkBlob...",
  "algorithm": "sha256",
  "hash": "abcdef...",
  "path": "blobs/sha256/ab/abcdef...",
  "size": 12345,
  "media_type": "application/octet-stream"
}
```

### 12.2 Blob integrity

Importers must verify blob hashes before marking blobs usable.

If a blob hash does not match, import must record an error and must not treat the
blob as valid.

---

## 13. Artifact Records

Artifact records are stored as:

```text
artifacts/<artifact_id>.json
```

Artifact records describe logical artifacts and their blob references.

Artifacts may represent:

- Files
- Directories
- Disk images
- Build outputs
- Source bundles
- Data files
- Media
- Runtime assets

Artifact records must preserve expected blob hashes.

---

## 14. Execution Records

Execution records are stored under:

```text
executions/<execution_id>/
  record.json
  logs/
  outputs/
```

Execution records may include:

1. Execution ID.
2. Task ID.
3. Executor ID/version.
4. Plan hash.
5. Snapshot reference.
6. Purpose.
7. Input artifact hashes.
8. Output artifact hashes.
9. Environment metadata.
10. Granted capabilities.
11. Logs.
12. Diagnostics.

Execution records are evidence of execution. They do not modify object state unless
incorporated later through valid statements.

---

## 15. Index Files

Index files are optional but recommended for performance.

Recommended indexes:

```text
indexes/objects.json
indexes/snapshots.json
indexes/blobs.json
indexes/statements.json
```

Indexes must not be authoritative.

Importers may ignore indexes and reconstruct package contents by scanning files.

If indexes disagree with actual files, actual file contents and core validation
take precedence.

---

## 16. Provenance Metadata

Packages may include provenance metadata:

```text
provenance/package.json
```

Recommended shape:

```json
{
  "exported_from": {
    "node_id": "node_...",
    "store_id": "store_..."
  },
  "exported_by": {
    "actor_id": "z6MkActor..."
  },
  "source": "local",
  "federation_peers": [],
  "notes": null
}
```

Provenance is informational unless separately signed and trusted by policy.

Provenance must not replace statement signatures or core validation.

---

## 17. Deterministic Export

Exporters should support deterministic export mode.

In deterministic mode:

1. File paths must be canonical.
2. Directory entries must be sorted lexicographically.
3. JSON must use canonical serialization.
4. Archive file modification times must be normalized.
5. File owner/group metadata must be normalized or omitted.
6. Permissions must be normalized.
7. Compression settings must be deterministic or compression disabled.
8. Manifest content order must be deterministic.

Recommended normalized timestamp for archive entries:

```text
1970-01-01T00:00:00Z
```

or another explicitly specified constant.

Deterministic export improves reproducibility, caching, and package verification.

---

## 18. Import Semantics

Importing a package must follow this process:

1. Open package.
2. Read manifest.
3. Validate package schema version.
4. Scan package contents.
5. Verify package inventory consistency.
6. Verify blob hashes.
7. Ingest object records.
8. Ingest statement records.
9. Ingest blob records/content.
10. Ingest snapshot records.
11. Ingest execution records if present.
12. Record provenance.
13. Optionally request core validation.
14. Never execute imported content.

### 18.1 Import success vs validation success

Import success means data was ingested.

It does not mean:

- Object is valid.
- Snapshot is valid.
- Actor authority is trusted.
- Artifacts are safe to run.
- Package provenance is trusted.

### 18.2 Duplicate data

Importing duplicate objects, statements, or blobs should be idempotent.

If duplicate IDs map to different content, import must report a conflict or
integrity error.

### 18.3 Partial imports

If a package is partially corrupt, implementations may support partial import only
when safe and explicitly requested.

Default behavior should fail the import if required manifest entries or blob hashes
do not verify.

---

## 19. Export Semantics

Exporting a package must follow this process:

1. Resolve requested object/snapshot/execution reference.
2. Determine package type.
3. Determine closure purpose.
4. Ask core/daemon for required closure where applicable.
5. Collect object records.
6. Collect statement records.
7. Collect snapshot records.
8. Collect artifact records.
9. Collect blobs if requested or required.
10. Collect dependency snapshots if requested or required.
11. Collect execution records if requested.
12. Write canonical package directory.
13. Write manifest.
14. Optionally write indexes.
15. Optionally archive/compress.

### 19.1 Export options

Recommended options:

```text
--type object|snapshot-closure|archive-mirror|execution-record|mixed
--purpose inspect|build|run|reproduce|archive-mirror
--include-blobs
--include-dependencies
--include-executions
--full-log
--deterministic
--compress
```

### 19.2 Snapshot closure export

For a snapshot closure package, exporter should include all data needed for the
declared purpose.

If data is missing, exporter may either:

1. Fail export.
2. Export a partial package marked `partial`.

Default behavior should fail for requested complete closure exports unless
`--allow-partial` is supplied.

---

## 20. Package Validation

Package validation is different from core snapshot validation.

### 20.1 Package structural validation

Checks:

1. Manifest exists.
2. Schema version is supported.
3. Referenced files exist.
4. JSON records parse.
5. Blob hashes match.
6. Inventory is internally consistent.
7. Paths are safe.

### 20.2 Core validation

After structural validation, imported snapshots may be validated by the core
library.

Core validation determines:

- Valid
- Invalid
- Conflicted
- Indeterminate

Package validation must not be confused with snapshot validation.

---

## 21. Security Requirements

Package importers must:

1. Treat all package content as untrusted.
2. Never execute package content during import.
3. Sanitize paths.
4. Reject absolute paths in package entries.
5. Reject path traversal (`..`) entries.
6. Verify blob hashes.
7. Preserve statement signatures exactly.
8. Avoid overwriting unrelated local files.
9. Avoid trusting provenance without policy.
10. Avoid trusting indexes over content.
11. Enforce size/resource limits.
12. Handle malformed archives safely.

Package exporters must:

1. Avoid including private data unless requested.
2. Respect policy for publication/export.
3. Avoid leaking local filesystem paths unnecessarily.
4. Mark partial closure honestly.
5. Preserve signatures and hashes.

---

## 22. Size and Resource Limits

Importers should enforce configurable limits:

- Maximum archive size.
- Maximum unpacked size.
- Maximum file count.
- Maximum single blob size.
- Maximum manifest size.
- Maximum path length.
- Maximum nesting depth.

When limits are exceeded, import should fail safely.

---

## 23. Compatibility

Packages must declare schema version:

```json
{
  "schema": "kairo.package.v1"
}
```

Importers must reject unsupported required features.

Optional future fields may be ignored if not understood.

Feature flags may be declared:

```json
{
  "features": {
    "required": [],
    "optional": []
  }
}
```

If a required feature is unknown, import must fail safely.

---

## 24. Signing Packages

Package-level signatures are optional in v1.

Important distinction:

1. Statement signatures prove statement authorship.
2. Package signatures prove package assembly provenance.

A package signature must not replace statement validation.

Future package signature files may be stored as:

```text
signatures/
  package.sig
```

or referenced from the manifest.

---

## 25. CLI Mapping

`CLI.md` commands:

```text
kairo import <package>
kairo export <ref> --output <package>
```

Recommended examples:

```text
kairo export object:z6MkObject...:snapshot:z6MkSnapshot... --type snapshot-closure --purpose run --include-blobs --output game.kairo

kairo export object:z6MkObject... --type archive-mirror --full-log --include-blobs --output object-mirror.kairo

kairo import game.kairo --verify
```

---

## 26. API Mapping

`API.md` endpoints:

```text
POST /api/v1/import
POST /api/v1/export
```

Import/export may be long-running daemon tasks.

API responses must distinguish:

- Package structural success/failure.
- Store ingestion success/failure.
- Core validation result.
- Policy decision.

---

## 27. Store Mapping

On import, the store should ingest:

- Objects into object storage.
- Statements into statement storage.
- Blobs into blob storage.
- Snapshots into snapshot metadata/cache.
- Execution records into execution history if supported.
- Provenance into local metadata.

The store must not treat imported data as trusted solely because it came from a
package.

---

## 28. Implementation Checklist

A conforming initial implementation should provide:

1. Directory package reader.
2. Tar-based package reader.
3. Directory package writer.
4. Tar-based package writer.
5. Manifest parser.
6. Manifest writer.
7. Package structural validator.
8. Blob hash verifier.
9. Safe path validation.
10. Object record import/export.
11. Statement record import/export.
12. Snapshot record import/export.
13. Blob import/export.
14. Snapshot closure package export.
15. Archive mirror package export.
16. Idempotent import behavior.
17. Partial package detection.
18. Deterministic export mode.
19. CLI import/export integration.
20. API import/export integration.

---

End of `PACKAGE.md`.
