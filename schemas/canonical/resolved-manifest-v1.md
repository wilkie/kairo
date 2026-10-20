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
| `provides` | `provides`; `version` when a requirement was given |
| `object` | `object`; one of `version`, `snapshot` |
| `source` | `source` (the canonical URL), `revision`; `path` when placed; `seed` when `true` |

`resolved`, by `kind`:

| `kind` | Fields |
|---|---|
| `artifact` | `object`, `snapshot`, `target`, `output`, `artifact` (blob id, or `null` when the build is in `builds_first`) |
| `content` | `object`, `snapshot` |
| `environment` | `contract`, `engine` (the contract label the engine implements, or `null` for the host in development mode) |
| `host-tool` | `provides`, `path`; `version` when the config states one |
| `source` | `source` (id), `url`, `revision`; `path` when placed; `seed` when `true` |

A `builds_first` entry is `{"object", "snapshot", "target", "plan"}`,
where `plan` is that build's own plan hash.

## Example

```json
{"schema":"kairo.resolved-manifest.v1","object":"zQmApp...","snapshot":"zQmSnap...","target":"release","environment":"none","host_dependent":true,"requirements":[{"requested":{"kind":"provides","provides":"tool:make","version":">=4"},"resolved":{"kind":"artifact","object":"zQmMake...","snapshot":"zQmMakeSnap...","target":"release","output":"bin","artifact":null}},{"requested":{"kind":"provides","provides":"environment:linux/x86-64"},"resolved":{"kind":"environment","contract":"environment:linux/x86-64","engine":null}}],"builds_first":[{"object":"zQmMake...","snapshot":"zQmMakeSnap...","target":"release","plan":"zQmPlan..."}]}
```

## Excluded

- which objects the resolver skipped and which candidates it didn't
  choose (diagnostics, printed but not hashed);
- the engine and mounting strategy (the execution record's);
- host facts beyond what `environment` and `host_dependent` imply.
