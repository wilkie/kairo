# ActorGenesis v1 Canonical Encoding

## Type

```text
ActorGenesis
```

## Version

```text
1
```

## Domain Separator

```text
kairo.actor.genesis.v1
```

## Derived ID

The unsigned `ActorGenesis` body derives an `ActorId`.

```text
ActorId = z<base58btc(multihash_sha2_256(domain || canonical_bytes))>
```

## Purpose

`ActorGenesis` creates stable actor identity. The initial public key roots the
actor, but `ActorId` is not merely a hash of the public key. The genesis body
also includes actor kind, version/domain separation, and a nonce so distinct
actors may intentionally start with the same key without sharing identity.

Later actor statements may add, revoke, rotate, or delegate keys without
changing `ActorId`.

## Canonical Fields

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| type marker `"ActorGenesis"` | `string` |
| version `1` | `u8` |
| `actor_kind` | `string` |
| `initial_key` | `PublicKey` |
| `nonce` | `bytes`, exactly 32 bytes |

`PublicKey` is encoded as:

| Field | Encoding |
|---|---|
| `algorithm` | `string`, currently `ed25519` |
| `bytes` | raw public key bytes as `bytes` |

## Key IDs

`KeyId` is derived from canonical public key material using domain separator:

```text
kairo.actor.key.v1
```

In v1, `KeyId` uses the same SHA-256 multihash/base58btc payload format as other
Kairo-native IDs, but it is not an `ActorId`.

## Excluded Fields

The following must not be included in Actor ID canonical bytes:

- display name
- profile metadata
- current primary key
- later added keys
- key revocations
- delegation statements
- local trust decisions

## Rust-Equivalent Pseudocode

```text
canonical_public_key =
  string(algorithm) ||
  bytes(public_key)

canonical_actor_genesis =
  string("ActorGenesis") ||
  u8(1) ||
  string(actor_kind) ||
  canonical_public_key ||
  bytes(nonce)

actor_id =
  sha2_256_multihash_base58btc(
    "kairo.actor.genesis.v1" || canonical_actor_genesis
  )
```

## Notes

- Ed25519 is the only required signature algorithm in the current MVP.
- A valid signature proves control of a key, not local trust in the actor.
- Actor authority and key-active status are evaluated by later actor and policy
  rules.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
