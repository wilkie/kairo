# ObjectGenesis v1 Canonical Encoding

## Type

```text
ObjectGenesis
```

## Version

```text
1
```

## Domain Separator

```text
kairo.object.genesis.v1
```

## Derived ID

The unsigned `ObjectGenesis` body derives an `ObjectId`.

```text
ObjectId = z<base58btc(multihash_sha2_256(domain || canonical_bytes))>
```

## Purpose

`ObjectGenesis` creates stable Object lineage identity. It is signed as a
statement, but the signature is not part of the Object ID hash. The unsigned
body is intentionally minimal so mutable descriptive facts can change without
changing Object identity.

## Canonical Fields

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| type marker `"ObjectGenesis"` | `string` |
| version `1` | `u8` |
| `object_kind` | `string` |
| `created_by` | `ActorId` payload as `string` |
| `nonce` | `bytes`, exactly 32 bytes |
| `initial_revision` | `option<string>` |

## Excluded Fields

The following must not be included in Object ID canonical bytes:

- signature
- signature envelope
- display name
- description
- tags
- categories
- current owner
- version names
- federation metadata
- received-at timestamps
- local trust decisions

## Rust-Equivalent Pseudocode

```text
canonical_bytes =
  string("ObjectGenesis") ||
  u8(1) ||
  string(object_kind) ||
  string(created_by) ||
  bytes(nonce) ||
  option(initial_revision, string)

object_id =
  sha2_256_multihash_base58btc(
    "kairo.object.genesis.v1" || canonical_bytes
  )
```

## Notes

- `created_by` is the bare `ActorId` payload.
- `initial_revision`, when present, is a storage revision string such as
  `git:sha256:<commit>`.
- The nonce must be 32 bytes.
- Timestamps are excluded from v1 identity material.
- Re-signing the same `ObjectGenesis` body must not change the Object ID.
- `created_by` records origin authority, not permanent ownership. Current
  ownership is represented by later authorized statements.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
