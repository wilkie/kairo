# Canonical Schemas

Canonical schema documents define exact bytes used for Kairo identity, hashing,
and signature inputs.

The Rust implementation of the shared primitive encodings lives in
`crates/kairo-core/src/canonical.rs`.

All canonical documents must define:

1. Type name.
2. Version.
3. Domain separator.
4. Derived ID type.
5. Shared primitive encodings used.
6. Field order.
7. Excluded fields.
8. Rust-equivalent pseudocode.
9. Test vectors once fixtures stabilize.

## Shared Primitive Encodings

Unless a statement-specific document says otherwise:

| Type | Encoding |
|---|---|
| `u8` | one byte |
| `u16` | unsigned big-endian |
| `u32` | unsigned big-endian |
| `i64` | signed big-endian, two's complement |
| `bytes` | `u32 length || bytes` |
| `string` | UTF-8 bytes encoded as `bytes` |
| `option<T>` | `0x00` for none, `0x01 || T` for some |
| `list<T>` | `u32 count || item...` |
| `Timestamp` | `i64` Unix epoch seconds |

Kairo-native IDs inside canonical bytes are encoded as their bare ID payload
strings, not as `object:<id>` or `kairo:object:<id>` references, unless a field
is explicitly a standalone reference.

## Documents

- `actor-genesis-v1.md`
- `object-manifest-v1.md`
- `object-genesis-v1.md`
- `object-revision-v1.md`
- `object-branch-v1.md`
- `object-version-tag-v1.md`
- `actor-trust-v1.md`
- `actor-capability-grant-v1.md`
- `actor-capability-revocation-v1.md`
- `actor-key-rotation-v1.md`
- `actor-key-revocation-v1.md`
- `snapshot-v1.md`
