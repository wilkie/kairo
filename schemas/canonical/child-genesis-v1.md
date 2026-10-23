# ChildGenesis v1 Canonical Encoding

## Type

```text
ChildGenesis
```

## Version

```text
1
```

## Domain Separator

```text
kairo.object.child.v1
```

## Derived ID

A child object's `ObjectId`:

```text
ObjectId = z<base58btc(multihash_sha2_256(domain || canonical_bytes))>
```

The domain differs from those of `ObjectGenesis`, `SourceGenesis`, and
`FileSourceGenesis`, so a child's id can never equal another kind of
object's.

## Purpose

A child object is a part of a native object's build that is an object of
its own (`specs/DECISIONS.md` §41 sub-decision 4): a target declares it
under `[[build.targets.children]]` with a `name`, a `root` in the
target's artifact, an install root, and the labels it provides. Its
genesis is the triple that names it, and nothing else, so every node
derives the same id from the parent's manifest. It has no creator,
timestamp, or nonce and is not signed: the parent's signed revision
declares it, so the parent's signer vouches for it.

A child has no history or branches of its own. It exists at every
revision of its parent whose manifest declares it, and its versions
are its parent's. Its artifact is the part of the parent's artifact
under `root`, re-rooted there (an `artifact-tree-v1.md` document with
its own id); its build result is its parent's.

## Field Order

| # | Field | Encoding |
|---|---|---|
| 1 | type marker | `string`: `ChildGenesis` |
| 2 | version | `u8`: `1` |
| 3 | `parent` | `string`: the parent's bare `ObjectId` |
| 4 | `target` | `string`: the declaring build target's name |
| 5 | `name` | `string`: the child's name, lowercase letters, digits, and `.`, `_`, `+`, `-`, starting with a letter or digit |

## Excluded Fields

The child's `root`, `install`, and `provides` are not part of its
identity: they are the parent manifest's, and may change between the
parent's revisions while the child stays the same object.

## Pseudocode

```text
canonical_bytes = string("ChildGenesis") || u8(1)
               || string(parent) || string(target) || string(name)
object_id = sha2_256_multihash_base58btc("kairo.object.child.v1" || canonical_bytes)
```
