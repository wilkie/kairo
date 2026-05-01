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
metadata, content kind, provided capabilities, and dependencies.

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

`KairoManifestSection` is encoded as:

| Field | Encoding |
|---|---|
| `schema` | `u32` |
| `object` | `option<string>` containing bare `ObjectId` payload |
| `kind` | `string` |
| `name` | `string` |
| `summary` | `option<string>` |

`ContentSection`: `string(kind)`.

`ProvideDeclaration`: `string(provides) || option(version, string)`.

`DependencyDeclaration` variants:

| Variant | Encoding |
|---|---|
| `Provides` | `string("provides") || string(provides)` |
| `Object` | `string("object") || string(object_id) || selector` |

`ObjectDependencySelector` variants:

| Variant | Encoding |
|---|---|
| `Version` | `string("version") || string(version)` |
| `Snapshot` | `string("snapshot") || string(snapshot_id)` |

## Ordering

TOML table key order is not significant. The canonical field order above is
always used.

The order of TOML arrays is significant:

- `[[provides]]` order is preserved.
- `[[dependencies]]` order is preserved.

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
  list(dependencies, canonical_dependency_declaration)

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

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
