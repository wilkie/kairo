# ObjectManifest v1 Canonical Encoding

## Type

```text
ObjectManifest
```

## Version

```text
1
```

## Domain Separator

```text
kairo.object.manifest.v1
```

## Derived ID

The canonical `ObjectManifest` derives a `BlobId`. `ObjectRevision.manifest_hash`
points to this ID.

```text
BlobId = z<base58btc(multihash_sha2_256(domain || canonical_manifest_bytes))>
```

## Purpose

`ObjectManifest` is the normalized form of a revision's `kairo.toml`. It records
the manifest fields that affect Kairo's interpretation of the revision:
metadata, content kind, provides labels, dependencies, and build targets
(`specs/DECISIONS.md` §33 sub-decision 1).

The manifest hash is computed from typed, validated manifest data rather than
raw TOML bytes. Comments, whitespace, formatting, and TOML key order do not
affect the hash.

## Example TOML

```toml
[kairo]
schema = 1
object = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk"
kind = "software"
name = "Example"
summary = "Example object."

[content]
kind = "tree"

[[provides]]
provides = "tool:make"
version = "3.81"

[[dependencies]]
kind = "object"
object = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk"
version = "^4.1.0"

[[build.targets]]
name = "release"
command = ["make", "release"]
workdir = "."

[[build.targets.dependencies]]
kind = "provides"
provides = "tool:make"
version = ">=4, <5"

[[build.targets.outputs]]
name = "cli"
path = "dist/app"
artifact_path = "bin/app"
kind = "file"
```

## Canonical Fields

The top-level manifest is encoded in this exact order:

| Field | Encoding |
|---|---|
| type marker `"ObjectManifest"` | `string` |
| version `1` | `u8` |
| `kairo` | `KairoManifestSection` |
| `content` | `option<ContentSection>` |
| `provides` | `list<ProvideDeclaration>` |
| `dependencies` | `list<DependencyDeclaration>` |
| `build` | present only when there is at least one target: `string("build") \|\| list<BuildTarget>` |

Fields added after the first manifests were hashed are encoded only when
present, so a manifest that doesn't use them keeps its hash.

`KairoManifestSection` is encoded as:

| Field | Encoding |
|---|---|
| `schema` | `u32` |
| `object` | `option<string>` containing bare `ObjectId` payload |
| `kind` | `string` |
| `name` | `string` |
| `summary` | `option<string>` |
| `tags` | present only when there is at least one: `string("tags") \|\| list<string>` |

A tag is lowercase letters, digits, and `.`, `+`, `-`, starting with a
letter or digit; a list holds no repeats and keeps its order
(`DECISIONS.md` §42).

Every provides label, in `[[provides]]`, a child's `provides`, or a
`kind = "provides"` dependency, is `kind:specifier(:detail)*` with a
non-empty specifier and `kind` one of `tool`, `lib`, `doc`, `data`, and
`environment`; any other kind is refused (`DECISIONS.md` §42).

`ContentSection`: `string(kind)`.

`ProvideDeclaration`: `string(provides) || option(version, string)`, then
`string("artifact") || string(target) || string(output)` when the entry
names a build output (an artifact provider); `output` is `""` when the
entry names the target's whole artifact (`DECISIONS.md` §41).

`DependencyDeclaration` variants:

| Variant | Encoding |
|---|---|
| `Provides` | `string("provides") \|\| string(provides)`, then `string("version") \|\| string(version)` when a version requirement is given, then `placement` when placed |
| `Object` | `string("object") \|\| string(object_id) \|\| selector`, then `placement` when placed |
| `Source` | `string("source") \|\| string(canonical url) \|\| string(git revision) \|\| option<string>(path) \|\| u8(seed)`, then `placement options` when placed |
| `File` | `string("file") \|\| string(lowercase hex sha256) \|\| string(path)`, then `string("overlay")` when it overlays |

`placement` is `string("path") || string(path) || placement options`;
`placement options` are `string("from") || string(from)` when a subtree
is placed and `string("overlay")` when it overlays, each present only
when set, so a dependency without them hashes as it did before they
existed (`DECISIONS.md` §38 sub-decision 4).

A `Source` dependency (`DECISIONS.md` §37) names a source object by URL,
hashed in canonical form so its spellings don't change the hash, pinned
to an exact upstream commit (`git:sha1:<oid>` or `git:sha256:<oid>`); it
takes no version. `path`, when given, places the source's tree at that
relative path in the source tree; two of a target's placed dependencies
may not overlap. `seed` is `1` when the dependency is a seed.

A `File` dependency (`DECISIONS.md` §38) names a file source by the
SHA-256 of its bytes and places the file at `path`, a relative file path
in the source tree. No two placed dependencies, of any kind, share a
path; `from` and `overlay` require a `path`, and a file takes no
`from`.

`ObjectDependencySelector` variants:

| Variant | Encoding |
|---|---|
| `Version` | `string("version") || string(version)` |
| `Snapshot` | `string("snapshot") || string(snapshot_id)` |

Version requirements (object dependency `version`, provides dependency
`version`) use Cargo's requirement syntax (`">=4, <5"`, `"^4.1.0"`) and are
encoded as written.

`BuildTarget` is encoded as:

| Field | Encoding |
|---|---|
| `name` | `string`, matching `[a-z][a-z0-9-]*`, unique in the manifest |
| `command` | `list<string>`, non-empty, first element non-empty |
| `workdir` | `string`; `"."` when not given |
| `dependencies` | `list<DependencyDeclaration>` |
| `outputs` | `list<BuildOutput>`, possibly empty |
| `artifact_root` | present only when not `"."`: `string("artifact_root") \|\| string(root)`, a relative path under `/kairo/package` |
| `install` | present only when not `"/"`: `string("install") \|\| option<string>(install)`, an absolute guest path, or none for `install = false` |
| `children` | present only when there is at least one: `string("children") \|\| list<ChildDeclaration>` |

`artifact_root` and `install` come from `[build.targets.artifact]`
(`DECISIONS.md` §41 sub-decision 3); a target without one, or with the
defaults spelled out, encodes as it did before they existed.

`ChildDeclaration`, one `[[build.targets.children]]` entry
(`DECISIONS.md` §41 sub-decision 4), is encoded as:

| Field | Encoding |
|---|---|
| `name` | `string`, lowercase letters, digits, and `.`, `_`, `+`, `-`, starting with a letter or digit; unique in the target |
| `root` | `string`, a relative path in the target's artifact |
| `install` | `option<string>`: an absolute guest path, `/` when not given, or none for `install = false` |
| `provides` | `list<string(provides) \|\| option<string>(version)>` |
| `summary` | present only when given: `string("summary") \|\| string(summary)` |
| `tags` | present only when there is at least one: `string("tags") \|\| list<string>` |

```toml
[[build.targets.children]]
name = "bash"
root = "bash-5.2.15"
[[build.targets.children.provides]]
provides = "tool:bash"
version = "5.2.15"
```

Each child is an object whose id derives from `(this object, target,
name)` (`child-genesis-v1.md`); its artifact is the part of the
target's artifact under `root`. A provides entry's artifact is always
the child's own, so it names no target or output.

`BuildOutput` is encoded as:

| Field | Encoding |
|---|---|
| `name` | `string`, unique in the target |
| `path` | `string`, relative to the target's `workdir` |
| `artifact_path` | `string`, unique in the target |
| `kind` | `string`: `"file"` or `"directory"` |

`workdir` (other than `"."`), `path`, and `artifact_path` must be
`/`-separated relative paths with no empty, `.`, or `..` components and no
backslashes. A dependency's placement `path` may instead be absolute
(`/` and such a path), naming a path in the guest's `/`.

## Ordering

TOML table key order is not significant. The canonical field order above is
always used.

The order of TOML arrays is significant:

- `[[provides]]` order is preserved.
- `[[dependencies]]` order is preserved.
- `[[build.targets]]`, each target's `command`, `[[build.targets.dependencies]]`,
  `[[build.targets.outputs]]`, and `[[build.targets.children]]` order is
  preserved, as is each child's `provides`.

## Excluded Fields

The following must not be included in manifest hash bytes:

- TOML comments
- TOML whitespace and formatting
- TOML key order
- local workspace metadata from `.kairo/`
- Git commit ID
- signed statements
- federation metadata
- trust decisions

## Rust-Equivalent Pseudocode

```text
canonical_manifest =
  string("ObjectManifest") ||
  u8(1) ||
  canonical_kairo_section ||
  option(content, canonical_content_section) ||
  list(provides, canonical_provide_declaration) ||
  list(dependencies, canonical_dependency_declaration) ||
  (if build_targets non-empty:
     string("build") || list(build_targets, canonical_build_target))

manifest_hash =
  sha2_256_multihash_base58btc(
    "kairo.object.manifest.v1" || canonical_manifest
  )
```

## Notes

- `object`, when present, is a consistency check against signed statements. It
  is not the source of Object authority.
- Object dependencies must specify exactly one selector: `version` or
  `snapshot`.
- `manifest_hash` in `ObjectRevision` should equal the `BlobId` derived by this
  document.
- If `[kairo].object` is present, revision validation requires it to match the
  `ObjectRevision.object` field. If it is absent, the signed revision statement
  remains the source of the object binding.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
