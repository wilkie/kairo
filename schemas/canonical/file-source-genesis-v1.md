# FileSourceGenesis v1 Canonical Encoding

## Type

```text
FileSourceGenesis
```

## Version

```text
1
```

## Domain Separator

```text
kairo.source.file.genesis.v1
```

## Derived ID

A file source's `ObjectId`:

```text
ObjectId = z<base58btc(multihash_sha2_256(domain || canonical_bytes))>
```

The domain differs from `SourceGenesis`'s and `ObjectGenesis`'s, so a
file source's id can never equal a Git source's or a native object's.

## Purpose

A file source archives one external file, a distfile or a release
tarball (`specs/DECISIONS.md` §38). It is identified by the SHA-256 of
its bytes, and its genesis holds that hash and nothing else, so every
node holding the same bytes derives the same id however it got them.
It has no creator, timestamp, or nonce and is not signed. What actors
sign about a file source are `FileObservation` statements
(`file-observation-v1.md`): where each fetch of it came from.

## The Bytes

The file is a blob whose id is the plain SHA-256 multihash of its
bytes, with no domain separator:

```text
BlobId = z<base58btc(multihash_sha2_256(bytes))>
```

so the blob is found from the genesis alone, and a store refuses bytes
that don't hash to the genesis's digest, on write and on read.

## Field Order

| # | Field | Encoding |
|---|---|---|
| 1 | type marker | `string`: `FileSourceGenesis` |
| 2 | version | `u8`: `1` |
| 3 | `sha256` | `string`: 64 lowercase hex digits |

## JSON Shape

Stored as `<store>/file_sources/<XX>/<YY>/<id>.json`:

```json
{ "type": "FileSourceGenesis", "version": 1, "sha256": "183a40ea47ea49f8a1e3bd1b9d12e676374d64d63bc79e7bc1ae7d673dfdf25d" }
```

A digest in any other spelling (upper case, short) is refused, so one
file source has one stored form, and the id is re-derived and checked
against the file's name.

## Pseudocode

```text
canonical_bytes = string("FileSourceGenesis") || u8(1) || string(lowercase_hex(sha256))
object_id = sha2_256_multihash_base58btc("kairo.source.file.genesis.v1" || canonical_bytes)
```
