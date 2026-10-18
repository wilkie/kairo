# NodeDocument v1 Signing Bytes

## Type

```text
NodeDocument
```

## Version

```text
1
```

## Domain Separator

```text
kairo.fed.node-document.v1
```

Unlike a statement's domain, which goes into an id hash, this domain
is the first field of the signed bytes themselves
(`DECISIONS.md` §25 sub-decision 1). It keeps a node document
signature from verifying as any other signature by the node actor.

## Derived ID

None. The document has no id; it is served at
`/.well-known/kairo-node` and re-signed at every node startup.

## Purpose

The node signs its document with the node actor's active operational
key, binding "the node at `base_url` is `node_actor`". A pull fetches
the node actor's genesis and key events (always served,
`DECISIONS.md` §25 sub-decision 2) and checks that the signature is by
the key active at `signed_at` and not revoked then.

## Shared Primitive Encodings

`string`, `list<string>`, and `Timestamp`, from `README.md`.

## Field Order

| # | Field | Encoding |
|---|---|---|
| 1 | domain | `string`: `kairo.fed.node-document.v1` |
| 2 | `schema` | `string`: `kairo.fed.node.v1` |
| 3 | `node_actor` | `string`: bare actor id |
| 4 | `base_url` | `string`, exactly as in the document |
| 5 | `protocol_versions` | `list<string>`, in document order |
| 6 | `supports` | `list<string>`, in document order |
| 7 | `signed_at` | `Timestamp` |

## Excluded Fields

- `actor_genesis`: `node_actor` derives from it, and the client checks
  that before checking the signature, so signing the id binds the
  genesis.
- `signature`: the object of the signature.

## JSON Shape

```json
{
  "schema": "kairo.fed.node.v1",
  "node_actor": "z6MkNode...",
  "actor_genesis": { "...": "ActorGenesis JSON as stored" },
  "base_url": "https://archive.example.org",
  "protocol_versions": ["kairo-fed/1"],
  "supports": ["object-pull", "statement-batch"],
  "signed_at": "2026-10-18T12:00:00Z",
  "signature": {
    "key_id": "z...",
    "algorithm": "ed25519",
    "bytes": "<standard base64>"
  }
}
```

The signing actor is `node_actor`, so `signature` doesn't repeat it.

## Pseudocode

```rust
fn node_document_signing_bytes(document: &NodeDocument, signed_at: Timestamp) -> Vec<u8> {
    let mut out = Vec::new();
    encode_str(&mut out, "kairo.fed.node-document.v1");
    encode_str(&mut out, "kairo.fed.node.v1");
    encode_str(&mut out, document.node_actor.as_str());
    encode_str(&mut out, &document.base_url);
    encode_list(&mut out, &document.protocol_versions, encode_str);
    encode_list(&mut out, &document.supports, encode_str);
    signed_at.encode_canonical(&mut out);
    out
}
```

## Test Vectors

Covered by `crates/kairo-federation-core/src/tests.rs`: a signed
document verifies, and changing any signed field fails it.
