# ObjectRevision v1 Canonical Encoding

## Type

```text
ObjectRevision
```

## Version

```text
1
```

## Domain Separator

```text
kairo.statement.v1
```

## Derived ID

`ObjectRevision` is an ordinary signed statement. Its `StatementId` is derived
from the unsigned statement envelope and body. The signature proves authorship
of those canonical bytes but is not part of the statement ID.

```text
StatementId = z<base58btc(multihash_sha2_256(domain || canonical_unsigned_statement_bytes))>
```

## Purpose

`ObjectRevision` records that an actor claims a storage revision belongs to a
specific Kairo Object lineage. For a Git-backed object, this lets a Kairo node
bind an immutable Git commit and its parent relationship to an Object without
trusting mutable Git branch or tag names.

The statement is also where the actor can attest that the referenced revision
is reachable through the declared history. Verification can still be local:
clients may check the manifest hash, parent links, actor authority, and any
policy requirements before trusting the claim.

## Example

```json
{
  "type": "ObjectRevision",
  "version": 1,
  "actor": "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t",
  "subject": "object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk",
  "body": {
    "object": "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk",
    "revision": "git:sha256:revision",
    "parents": ["git:sha256:parent"],
    "manifest_hash": "zQmfBE2w2UqKJhGZAxK4ZWb4JuCrvxnN9P4YKuvzfbPSvD5",
    "attests_reachable_history": true
  },
  "signature": {
    "actor": "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t",
    "key_id": "primary",
    "algorithm": "example",
    "bytes": "..."
  }
}
```

## Canonical Envelope Fields

The shared unsigned statement envelope is encoded before the body:

| Field | Encoding |
|---|---|
| type `"ObjectRevision"` | `string` |
| version `1` | `u8` |
| `actor` | `ActorId` payload as `string` |
| `subject` | internal Kairo reference as `string` |
| `body` | body fields below |

## Canonical Body Fields

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| `object` | `ObjectId` payload as `string` |
| `revision` | storage revision as `string` |
| `parents` | `list<string>` in storage parent order |
| `manifest_hash` | `BlobId` payload as `string` |
| `attests_reachable_history` | `u8`, `0` for false and `1` for true |

## Excluded Fields

The following must not be included in Statement ID canonical bytes:

- signature
- received-at timestamps
- source peer or federation route
- local trust decisions
- Git branch names
- Git tag names
- display labels

## Rust-Equivalent Pseudocode

```text
canonical_body =
  string(object) ||
  string(revision) ||
  list(parents, string) ||
  string(manifest_hash) ||
  u8(attests_reachable_history)

canonical_unsigned_statement =
  string("ObjectRevision") ||
  u8(1) ||
  string(actor) ||
  string(subject) ||
  canonical_body

statement_id =
  sha2_256_multihash_base58btc(
    "kairo.statement.v1" || canonical_unsigned_statement
  )
```

## Notes

- `object` is the bare `ObjectId` payload. The `subject` is the internal
  reference string, such as `object:<id>`.
- `parents` preserves the storage layer's parent order. Reordering parents
  changes the `StatementId`.
- `manifest_hash` identifies the canonical manifest for the revision.
- A valid signature proves only that the actor made the claim. It does not
  prove that the claim should be trusted.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
