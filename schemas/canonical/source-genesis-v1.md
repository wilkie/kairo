# SourceGenesis v1 Canonical Encoding

## Type

```text
SourceGenesis
```

## Version

```text
1
```

## Domain Separator

```text
kairo.source.genesis.v1
```

## Derived ID

A source object's `ObjectId`:

```text
ObjectId = z<base58btc(multihash_sha2_256(domain || canonical_bytes))>
```

The domain differs from `ObjectGenesis`'s, so a source id can never
equal a native object's.

## Purpose

A source object archives one external Git repository faithfully
(`specs/DECISIONS.md` §37). Its genesis holds only the repository's
canonical URL, so every node mirroring the same repository derives the
same id and their observations of it can be compared. It has no
creator, timestamp, or nonce, and is not signed: there is nothing in it
to vouch for. What actors sign about a source are `SourceObservation`
statements (`source-observation-v1.md`).

## Canonical URL

`SourceUrl::parse` (`crates/kairo-statement/src/source.rs`):

| Input | Rule |
|---|---|
| scheme | `https` or `git`; `ssh`, `git+ssh`, `ssh+git`, and the `user@host:path` form become `https`; `file` and anything else is refused |
| userinfo | dropped (`user:secret@`) |
| host | lowercased; IPv6 literals stay bracketed |
| port | dropped when it is the scheme's default (443, 9418); an `ssh` URL on a non-default port is refused, having no `https` form |
| path | one trailing `/` and then a trailing `.git` dropped; empty, `.`, or `..` segments refused; case kept |
| query, fragment | refused |

Examples, all one source:

```text
https://github.com/oriansj/stage0-posix
https://github.com/oriansj/stage0-posix.git/
HTTPS://GitHub.com/oriansj/stage0-posix.git
ssh://git@github.com/oriansj/stage0-posix.git
git@github.com:oriansj/stage0-posix
```

## Field Order

| # | Field | Encoding |
|---|---|---|
| 1 | type marker | `string`: `SourceGenesis` |
| 2 | version | `u8`: `1` |
| 3 | `url` | `string`: the canonical URL |

## JSON Shape

Stored as `<store>/sources/<XX>/<YY>/<id>.json`:

```json
{ "type": "SourceGenesis", "version": 1, "url": "https://github.com/oriansj/stage0-posix" }
```

A non-canonical `url` is refused on read, so one source has one stored
form, and the id is re-derived and checked against the file's name.

## Pseudocode

```text
canonical_bytes = string("SourceGenesis") || u8(1) || string(canonical_url)
object_id = sha2_256_multihash_base58btc("kairo.source.genesis.v1" || canonical_bytes)
```
