# Inventory v1 Signing Bytes

## Type

```text
Inventory
```

## Version

```text
1
```

## Domain Separator

```text
kairo.fed.inventory.v1
```

As for the node document (`node-document-v1.md`), the domain is the
first field of the signed bytes, so an inventory signature can't stand
in for any other signature by the node actor.

## Derived ID

None. A node serves its newest inventory at
`/federation/v1/inventory`; receivers keep the newest per node as a
hint (`DECISIONS.md` §29).

## Shared Primitive Encodings

`string`, `list<string>`, and `Timestamp`, from `README.md`.

## Field Order

| # | Field | Encoding |
|---|---|---|
| 1 | domain | `string`: `kairo.fed.inventory.v1` |
| 2 | `schema` | `string`: `kairo.fed.inventory.v1` |
| 3 | `node_actor` | `string`: bare actor id |
| 4 | `base_url` | `string`, exactly as in the document |
| 5 | `objects` | `list<string>`: bare object ids, strictly ascending |
| 6 | `signed_at` | `Timestamp` |

`objects` must be strictly ascending, so one inventory has one
encoding; a receiver refuses any other order.

## Excluded Fields

- `signature`: the object of the signature.

## JSON Shape

```json
{
  "schema": "kairo.fed.inventory.v1",
  "node_actor": "z6MkNode...",
  "base_url": "https://archive.example.org",
  "objects": ["z6MkObjectA...", "z6MkObjectB..."],
  "signed_at": "2026-10-18T12:00:00Z",
  "signature": { "key_id": "z...", "algorithm": "ed25519", "bytes": "<base64>" }
}
```

## Pseudocode

```rust
fn inventory_signing_bytes(inventory: &Inventory, signed_at: Timestamp) -> Vec<u8> {
    let mut out = Vec::new();
    encode_str(&mut out, "kairo.fed.inventory.v1");
    encode_str(&mut out, "kairo.fed.inventory.v1");
    encode_str(&mut out, inventory.node_actor.as_str());
    encode_str(&mut out, &inventory.base_url);
    encode_list(&mut out, &inventory.objects, encode_str);
    signed_at.encode_canonical(&mut out);
    out
}
```

## Test Vectors

Covered by `crates/kairo-federation-core/src/tests.rs`.
