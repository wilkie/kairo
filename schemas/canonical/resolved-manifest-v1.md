# Resolved Manifest v1

## Type

```text
ResolvedManifest
```

## Version

```text
1
```

## Domain Separator

```text
kairo.resolved-manifest.v1
```

## Derived ID

The **plan hash**: a `BlobId` over the document's bytes.

```text
BlobId = z<base58btc(multihash_sha2_256(domain || document_bytes))>
```

The document is stored as a blob under that id, so `kairo build` and
`kairo reproduce` can cite a plan by hash and a reader can parse it
back.

## Purpose

A resolved manifest records what a build target's requirements were
pinned to (`specs/DECISIONS.md` §33 sub-decision 3, §34 sub-decision
5): the whole closure, down to the builds that have to happen first.
Build results cite it. It never names an isolation engine or a
mounting strategy, so one plan hash covers a build under any engine
that implements the same environment contract (§34 sub-decision 4).

## Encoding

Unlike the statement bodies, this document's canonical form **is its
JSON**: compact (no whitespace), UTF-8, with the fields in the order
below, and optional fields omitted when absent. `serde_json` with the
`kairo_resolver::ResolvedManifest` struct produces exactly this.

## Field Order

| # | Field | Value |
|---|---|---|
| 1 | `schema` | `"kairo.resolved-manifest.v1"` |
| 2 | `object` | bare object id |
| 3 | `snapshot` | bare snapshot id |
| 4 | `target` | the build target's name |
| 5 | `environment` | the contract label an engine will provide, or `"none"` |
| 6 | `host_dependent` | `true` when a host tool or the host standing in for a contract was used |
| 7 | `requirements` | list of requirement objects, in manifest order (top-level `[[dependencies]]`, then the target's) |
| 8 | `builds_first` | list of builds this plan needs first, dependencies before dependents |

A requirement is `{"requested": ..., "resolved": ...}`.

`requested`, by `kind`:

| `kind` | Fields |
|---|---|
| `provides` | `provides`; `version` when a requirement was given; `path` when placed |
| `object` | `object`; one of `version`, `snapshot`; `path` when placed |
| `source` | `source` (the canonical URL), `revision`; `path` when placed; `seed` when `true` |
| `file` | `sha256`, `path` |

Any placed requirement also has `from` when it places a subtree and
`overlay: true` when it overlays (`DECISIONS.md` §38 sub-decision 4);
both are omitted otherwise.

`resolved`, by `kind`:

| `kind` | Fields |
|---|---|
| `artifact` | `object`, `snapshot`, `target`, `output` (`""` for the whole artifact), `artifact` (blob id, or `null` when the build is in `builds_first`), `install` (the provider target's install root, or `null` for `/kairo/artifact/<id>`; `DECISIONS.md` §41), for a child object `root` (its part of the artifact, which is its parent's; absent otherwise), and `build_key` (the build key of the plan that makes it, when that plan could be made; `DECISIONS.md` §52) |
| `content` | `object`, `snapshot`; `install` (the object's `[content] install` root) and `root` (its `[content] root`), each only when the object declares it (`DECISIONS.md` §50); `tree`, what is under its root (`DECISIONS.md` §52) |
| `environment` | `contract`, `engine` (the contract label the engine implements, or `null` for the host in development mode) |
| `host-tool` | `provides`, `path`; `version` when the config states one |
| `source` | `source` (id), `url`, `revision`; `path` when placed; `seed` when `true` |
| `file` | `source` (the file source's id), `sha256`, `path` |

A `builds_first` entry is `{"object", "snapshot", "target", "plan"}`,
where `plan` is that build's own plan hash.

## Example

```json
{"schema":"kairo.resolved-manifest.v1","object":"zQmApp...","snapshot":"zQmSnap...","target":"release","environment":"none","host_dependent":true,"requirements":[{"requested":{"kind":"provides","provides":"tool:make","version":">=4"},"resolved":{"kind":"artifact","object":"zQmMake...","snapshot":"zQmMakeSnap...","target":"release","output":"bin","artifact":null}},{"requested":{"kind":"provides","provides":"environment:linux/x86-64"},"resolved":{"kind":"environment","contract":"environment:linux/x86-64","engine":null}}],"builds_first":[{"object":"zQmMake...","snapshot":"zQmMakeSnap...","target":"release","plan":"zQmPlan..."}]}
```

## Runtime requirements (`DECISIONS.md` §46)

After `builds_first` come `requires` and `runtime`, each present only
when non-empty, then two fields every plan has (`DECISIONS.md` §52):

- `source_date_epoch`: `SOURCE_DATE_EPOCH` in the guest, the target's
  or 315532800 (1980-01-01).
- `build_key`: a `BlobId` under the domain `kairo.build-key.v1` over the
  compact JSON of exactly what the guest sees and runs: the target's
  `command`, `workdir`, `artifact_root`, and canonically encoded
  `outputs`; `environment` and `host_dependent`; each requirement's
  `requested` and what it `resolved` to, an artifact by the `build_key`
  of the plan that makes it (or its id, when that plan can't be made)
  with its `output`, `install`, and `root`, and a content object by its
  `tree` and `install`; the runtime closure, each `provides` and what
  it resolved to; the target's tree, by the hash of its entries (mode,
  name, Git object id; `kairo.toml` left out of a whole tree) under the
  domain `kairo.tree-identity.v1`; and `source_date_epoch`. A content
  resolution's `tree` is the same hash, of the tree under its root.
  A held build whose plan has the same key is this plan's artifact.

`requires` and `runtime`:

- `requires`: what the artifact this plan builds, and each of its
  children, will require at run time. Each entry: `provides`; `version`
  (a requirement) or `pin` (`{ object, snapshot }`, one provider, for a
  conferred label without a version: the offer the build resolved); or,
  for a `one_of` entry, `one_of` (a list of `{ provides, version? }`)
  instead of `provides` (`DECISIONS.md` §51), an obligation on whatever
  builds with this artifact rather than something installed beside it;
  `child`, the child it applies to, or absent for the target's artifact
  and every child; `via`, either `declared` or `conferred by <label>
  from object:<id>`. A consumer whose requirement resolves to this
  artifact reads these from the plan the build result cites.
- `runtime`: the runtime closure of the build dependencies, installed
  in the build environment after the declared requirements. Each entry:
  `provides`, `version`, `pin` (the provider chosen), `via` (the labels
  it came through, the build dependency first), and `resolved` as a
  requirement's.

Both are part of the plan hash.

## Excluded

- which objects the resolver skipped and which candidates it didn't
  choose (diagnostics, printed but not hashed);
- the engine and mounting strategy (the execution record's);
- host facts beyond what `environment` and `host_dependent` imply.
