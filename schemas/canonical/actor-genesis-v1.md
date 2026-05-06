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

The genesis also declares one or more **attestation keys** plus an
**attestation threshold** — a separate authority surface that signs
only emergency key events (`ActorEmergencyKeyRotation`,
`ActorEmergencyKeyRevocation`, `ActorAttestationKeyAdd`,
`ActorAttestationKeyRevocation`, `ActorAttestationThresholdChange`).
Attestation keys and the threshold are part of the canonical genesis
bytes (and therefore of the `ActorId`); an attacker cannot swap them
out without producing a different actor. They exist so an operator can
recover from active-key loss or compromise without losing identity
continuity. See `ACTORS.md` §5.5.2 / §5.5.3 and
`schemas/canonical/actor-emergency-key-rotation-v1.md`.

Later actor statements may add, revoke, rotate, or delegate keys without
changing `ActorId`. The attestation set may be grown via
`ActorAttestationKeyAdd`, shrunk via `ActorAttestationKeyRevocation`,
and the threshold changed via `ActorAttestationThresholdChange` —
all subject to the rule that the resulting set size is ≥ the
resulting threshold.

## Canonical Fields

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| type marker `"ActorGenesis"` | `string` |
| version `1` | `u8` |
| `actor_kind` | `string` |
| `initial_key` | `PublicKey` |
| `attestation_keys` | `list<PublicKey>` — non-empty, sorted by raw public-key bytes ascending, deduplicated |
| `attestation_threshold` | `u8`, `1 ≤ threshold ≤ |attestation_keys|` |
| `created_at` | `Timestamp` (`i64` epoch seconds) |
| `nonce` | `bytes`, exactly 32 bytes |

`attestation_keys` MUST contain at least one entry. The canonical encoder
sorts entries by raw public-key bytes ascending and rejects duplicates at
body construction time so two operators producing the "same" genesis
content (modulo input ordering) derive the same `ActorId`.

`attestation_threshold` is the M of the M-of-N quorum required to sign
any attestation-surface emergency event (`ACTORS.md` §5.5.3). It MUST
satisfy `1 ≤ attestation_threshold ≤ attestation_keys.len()`. There is
no default and no JSON sugar — the threshold is always written
explicitly. Solo operators set `attestation_threshold = 1`; any value
above 1 requires that many distinct attestation signatures on every
emergency event.

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
- current primary signing key (after rotation; only `initial_key` is part of the genesis)
- later-added attestation keys (only the genesis-declared set is part of the
  `ActorId`; later additions arrive via `ActorAttestationKeyAdd` statements)
- later threshold changes (only the genesis-declared
  `attestation_threshold` is part of the `ActorId`; subsequent
  changes arrive via `ActorAttestationThresholdChange` statements)
- key rotations
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
  canonical_public_key(initial_key) ||
  list(sorted_dedup_by_bytes(attestation_keys), canonical_public_key) ||
  u8(attestation_threshold) ||
  i64_be(created_at_epoch_seconds) ||
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
- `created_at` is the actor's self-claim of when the genesis statement was made.
  It is not a trusted observation. Canonical bytes are the `i64` Unix epoch
  seconds (big-endian); JSON interchange uses strict RFC 3339 UTC seconds with
  the literal `Z` suffix and no fractional seconds.
- `initial_key` and the `attestation_keys` set MUST be disjoint. Reusing a
  signing key as its own attestation key collapses the recovery surface
  (compromise of the operational key would also compromise recovery), and
  the body validator rejects it at construction time.
- Attestation keys are operator-presented public keys. Kairo never stores
  the matching private material — the operator is expected to hold the
  private halves in cold storage (YubiKey, air-gapped device, hardware
  wallet, encrypted seed in a safe). The CLI MAY offer a generate-and-print
  convenience that emits the seed once and forgets it, but storage is
  always external.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
