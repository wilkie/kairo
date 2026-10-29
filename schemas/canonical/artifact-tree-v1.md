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

An entry is a file, a symbolic link (`DECISIONS.md` §39), a directory,
or a hard link (§54). A file:

| # | Field | Value |
|---|---|---|
| 1 | `path` | `/`-separated relative path inside the artifact: the output's `artifact_path`, plus the file's path under it for a directory output |
| 2 | `blob` | the file's `BlobId` under the domain `kairo.artifact.blob.v1` |
| 3 | `executable` | whether any of the file's execute bits is set |
| 4 | `mode` | optional: its permission bits, four octal digits (`"0444"`, `"4755"`), present only when they are neither `0644` nor `0755` |

A file without `mode` is `0755` if `executable`, else `0644`.

A link:

| # | Field | Value |
|---|---|---|
| 1 | `path` | as for a file |
| 2 | `link` | the link's target, as written |

A directory, present only when it is empty or its bits aren't `0755`;
every other directory is implied by the paths under it and is `0755`:

| # | Field | Value |
|---|---|---|
| 1 | `path` | as for a file |
| 2 | `directory` | `true` |
| 3 | `mode` | optional: as for a file, present only when not `0755` |

A hard link, a file that was the same inode as an earlier one:

| # | Field | Value |
|---|---|---|
| 1 | `path` | as for a file |
| 2 | `hardlink` | the path of the file entry it is |

A tree with none of `mode`, directories, or hard links encodes exactly
as before they existed, as one without links does.

Shape rules, checked when a build collects its outputs and whenever a
tree is loaded:

- paths are safe relative paths, strictly ascending;
- a link's target is non-empty and without NUL. A relative target,
  resolved lexically from the directory holding the link, never climbs
  above the artifact's root (`..` within it is fine; empty segments,
  from a trailing `/` or `//`, are ignored). An absolute target (more than `/`) names a path in the guest's
  filesystem and is kept as written. Either may dangle;
- no entry's path lies under a link's path, so materializing a tree
  never writes through one;
- a `mode` is four octal digits, and a file's agrees with its
  `executable`; neither a file's nor a directory's is the one it
  would have without it;
- a `hardlink` names a file entry earlier in the tree: not a link, a
  directory, or another hard link.

Collecting an output records links without following them; a
directory output that is itself a link fails the build, as does a link
whose target breaks these rules, or a device, FIFO, or socket. Files
that are one inode are recorded as one file and hard links to it, the
first by path the file. Owners and times are not recorded (§54
sub-decision 4). Materializing creates each link as a link and each
hard link as a hard link, and sets every file's and directory's bits
but setuid and setgid, which it never sets (§54 sub-decision 7). A
child's slice holds a file whose hard link lies outside it as a file
entry with that file's blob and mode. An object's own tree (§50) is
collected keeping only the executable bit, with no directory or hard
link entries (§54 sub-decision 6).

## Example

```json
{"schema":"kairo.artifact-tree.v1","entries":[{"path":"bin/app","blob":"zQmBin...","executable":true},{"path":"include/zlib.h","blob":"zQmHdr...","executable":false}]}
```

With a setuid program, a read-only file and its hard link, and an
empty sticky directory:

```json
{"schema":"kairo.artifact-tree.v1","entries":[{"path":"bin/passwd","blob":"zQmPwd...","executable":true,"mode":"4755"},{"path":"lib/perl5/strict.pm","blob":"zQmPm...","executable":false,"mode":"0444"},{"path":"lib/perl5/strict2.pm","hardlink":"lib/perl5/strict.pm"},{"path":"tmp","directory":true,"mode":"1777"}]}
```

## Related

Execution records (`kairo.execution-record.v1`, unsigned, under
`<store>/executions/<id>.json`, `EXECUTOR.md` §15) name the artifact
they produced; their id is a `BlobId` over their bytes under
`kairo.execution-record.v1`, and they are evidence, never truth.
