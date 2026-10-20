# Artifact Tree v1

## Type

```text
ArtifactTree
```

## Version

```text
1
```

## Domain Separator

```text
kairo.artifact-tree.v1
```

## Derived ID

The **artifact id**: a `BlobId` over the document's bytes.

```text
BlobId = z<base58btc(multihash_sha2_256(domain || document_bytes))>
```

The document is stored as a blob under that id. `BuildResult`
statements and resolved manifests cite artifacts by it.

## Purpose

An artifact is the declared outputs of one build (`BUILD.md` §5–§6,
`DECISIONS.md` §33 sub-decision 6): a tree of files, each a blob,
identified by content. Two builds with the same files produce the same
artifact id, whatever engine built them.

## Encoding

Canonical JSON: compact, UTF-8, fields in the order below, entries
sorted by `path` (byte order).

## Field Order

| # | Field | Value |
|---|---|---|
| 1 | `schema` | `"kairo.artifact-tree.v1"` |
| 2 | `entries` | list of entries |

An entry:

| # | Field | Value |
|---|---|---|
| 1 | `path` | `/`-separated relative path inside the artifact: the output's `artifact_path`, plus the file's path under it for a directory output |
| 2 | `blob` | the file's `BlobId` under the domain `kairo.artifact.blob.v1` |
| 3 | `executable` | whether the file had an executable bit |

Empty directories and symbolic links are not represented: an output
containing a symbolic link fails the build.

## Example

```json
{"schema":"kairo.artifact-tree.v1","entries":[{"path":"bin/app","blob":"zQmBin...","executable":true},{"path":"include/zlib.h","blob":"zQmHdr...","executable":false}]}
```

## Related

Execution records (`kairo.execution-record.v1`, unsigned, under
`<store>/executions/<id>.json`, `EXECUTOR.md` §15) name the artifact
they produced; their id is a `BlobId` over their bytes under
`kairo.execution-record.v1`, and they are evidence, never truth.
