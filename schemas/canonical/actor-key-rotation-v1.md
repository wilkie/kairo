# ActorKeyRotation v1 Canonical Encoding

## Type

```text
ActorKeyRotation
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

`ActorKeyRotation` is an ordinary signed statement. Its `StatementId` is
derived from the unsigned statement envelope and body. The signature
proves authorship of those canonical bytes but is not part of the
statement ID.

```text
StatementId = z<base58btc(multihash_sha2_256(domain || canonical_unsigned_statement_bytes))>
```

## Purpose

`ActorKeyRotation` is a first-person statement by which an actor
declares "from this point forward, my active signing key is `next_key`."
It is the routine-hygiene mechanism that lets actors swap keys without
losing their stable `ActorId`.

The rotation is signed by the **currently active** signing key. The
genesis-initial key (declared by `ActorGenesis.initial_key`) is
implicitly active from the genesis's `created_at` onward; it is *not*
introduced by a separate key event. Every subsequent change to the
key set (rotation or revocation) is one signed statement in a per-
actor chain.

`ActorKeyRotation` shares the `ObjectVersionTag` body shape — every
non-genesis statement carrying an explicit `supersedes` chain edge —
with one tightening:

> **Cross-actor supersession is invalid for `ActorKeyRotation`.** A
> `supersedes` reference must resolve to a prior key event (rotation
> or revocation) signed by the **same envelope actor**. Key authority
> is first-person; no other actor can rotate someone else's keys, even
> with a covering capability.

## Resolution Rule

> The actor's current active signing key is the `next_key` of the
> chain leaf, where the chain spans every `ActorKeyRotation` and
> `ActorKeyRevocation` statement signed by `actor`. The chain leaf is
> the statement no other key-event statement supersedes.

If the chain leaf is an `ActorKeyRevocation`, the actor has **no
active signing key** until they publish a successor `ActorKeyRotation`.

If there are no key-event statements at all, the actor's active key
is `ActorGenesis.initial_key`.

Chain precedence is authoritative — a successor statement that
explicitly names its predecessor is unambiguously later, regardless
of `created_at`. `(envelope.created_at, statement_id)` is **only** a
fork tiebreak, applied when the chain has multiple leaves.

## Active-Key-At-Causal-Position

For verifying any signed statement `S` from `actor` whose envelope
declares `created_at = T`, the verifier resolves the active key as:

1. Look up the actor's key-event chain leaf as of time `T` — i.e.
   the leaf considering only key-event statements with `created_at ≤ T`.
2. If that leaf is an `ActorKeyRevocation`, the actor had no active
   key at `T`; the statement's signature does not verify.
3. Otherwise the active public key at `T` is the leaf's `next_key`
   (or `ActorGenesis.initial_key` if no key-event statements precede
   `T`).
4. The verifier then matches the statement's `signature.key_id`
   against the active key's derived `KeyId`. A mismatch is reported as
   `SignatureStatus::Invalid` even if the bytes happen to verify
   against some other historical key for that actor.

This is the rule called out as an MVP gap in `ACTORS.md` §6.1; with
the key chain in place, the verifier no longer hardcodes
`genesis.initial_key`.

## Chain Validation

Every `ActorKeyRotation` is one of two shapes:

- **First key event after genesis (`supersedes = null`):**
  - Signed by the genesis-initial key.
  - Rotates away from the genesis-initial key to `next_key`.
- **Successor (`supersedes != null`):**
  - Signed by the chain leaf's currently active key (i.e. the prior
    rotation's `next_key`, or the genesis-initial key if the prior
    leaf is not a rotation in shape).
  - `supersedes` references an existing `ActorKeyRotation` or
    `ActorKeyRevocation` for the **same envelope actor**.

A successor whose `supersedes` does not resolve in the local store is
`Indeterminate`, not invalid — same handling as missing predecessors
elsewhere. The successor remains a valid leaf; the chain just doesn't
extend backwards in history.

A successor whose `supersedes` resolves to a key-event statement signed
by a **different actor** is **invalid** — no cross-actor rotation.

Forks are not blocked (an actor signing two rotations both naming the
same `supersedes` from two devices). The resolver picks the head among
chain leaves via fork tiebreak; the fork is preserved as audit signal
and surfaced operationally because a forked key chain is almost always
a sign of compromise or split-brain.

## Example — First rotation

```json
{
  "type": "ActorKeyRotation",
  "version": 1,
  "actor": "zQm<actor>",
  "subject": "actor:zQm<actor>",
  "created_at": "2026-06-01T12:00:00Z",
  "body": {
    "next_key": {
      "algorithm": "ed25519",
      "bytes": "base64-of-32-byte-public-key"
    },
    "supersedes": null
  },
  "signature": {
    "actor": "zQm<actor>",
    "key_id": "zQm<genesis-key-id>",
    "algorithm": "ed25519",
    "bytes": "base64-signature-by-genesis-key"
  }
}
```

## Example — Subsequent rotation

```json
{
  "type": "ActorKeyRotation",
  "version": 1,
  "actor": "zQm<actor>",
  "subject": "actor:zQm<actor>",
  "created_at": "2026-09-15T08:30:00Z",
  "body": {
    "next_key": {
      "algorithm": "ed25519",
      "bytes": "base64-of-new-32-byte-public-key"
    },
    "supersedes": "zQm<prior-key-event-statement-id>"
  },
  "signature": {
    "actor": "zQm<actor>",
    "key_id": "zQm<prior-rotation-next-key-id>",
    "algorithm": "ed25519",
    "bytes": "base64-signature-by-prior-active-key"
  }
}
```

## Canonical Envelope Fields

The shared unsigned statement envelope is encoded before the body:

| Field | Encoding |
|---|---|
| type `"ActorKeyRotation"` | `string` |
| version `1` | `u8` |
| `actor` | `ActorId` payload as `string` |
| `subject` (`actor:<self-actor-id>`) | internal Kairo reference as `string` |
| `created_at` | `Timestamp` (`i64` epoch seconds) |
| `body` | body fields below |

## Canonical Body Fields

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| `next_key` | `PublicKey` (`string(algorithm) \|\| bytes(public_key)`) |
| `supersedes` | `option<string>` — `0x00` for the first rotation, `0x01 \|\| string(StatementId payload)` otherwise |

`PublicKey` follows the same canonical encoding as
`ActorGenesis.initial_key` (see `actor-genesis-v1.md`).

## Excluded Fields

The following must not be included in Statement ID canonical bytes:

- signature
- the derived `KeyId` of `next_key` (computable from `next_key`)
- received-at timestamps
- source peer or federation route
- whether this `ActorKeyRotation` statement currently wins resolution
- whether the referenced `supersedes` statement is locally available

## Rust-Equivalent Pseudocode

```text
canonical_public_key =
  string(algorithm) ||
  bytes(public_key)

canonical_body =
  canonical_public_key(next_key) ||
  option(supersedes, string)

canonical_unsigned_statement =
  string("ActorKeyRotation") ||
  u8(1) ||
  string(actor) ||
  string(subject) ||
  i64_be(created_at_epoch_seconds) ||
  canonical_body

statement_id =
  sha2_256_multihash_base58btc(
    "kairo.statement.v1" || canonical_unsigned_statement
  )
```

## Notes

- `actor` (envelope) and `subject` both name the actor whose keys are
  being rotated. They must agree (`subject = "actor:" || actor`). They
  are encoded separately to keep the envelope shape uniform across
  statement types.
- `next_key` is the *new* active key — the one the actor will sign
  with after this rotation. The signature on this statement itself is
  produced by the *prior* active key (the genesis-initial key if
  `supersedes = null`, otherwise the chain leaf's current active key).
- The derived `KeyId` of `next_key` is not encoded in canonical bytes;
  it is computable from `next_key` via `kairo.actor.key.v1` domain
  separation. The signature envelope's `key_id` is recorded but does
  not affect the `StatementId`.
- `created_at` is the actor's self-claim of when the rotation took
  effect. Canonical bytes are `i64` Unix epoch seconds (big-endian);
  JSON interchange uses strict RFC 3339 UTC seconds with the literal
  `Z` suffix and no fractional seconds. The verifier uses this as the
  causal position when computing the active key for *other* statements
  signed by the same actor.
- A valid signature proves only that the prior active key authorized
  the rotation. It does not prove that `next_key` is held only by the
  legitimate operator — that is a key-management concern outside the
  protocol.
- Rotation does **not** auto-invalidate previously issued
  `ActorCapabilityGrant` statements: capabilities anchor on `ActorId`,
  not `KeyId` (`CAPABILITIES.md` §7). The opt-in
  `CapabilityConstraint::KeyPinned(KeyId)` opts a grant in to
  auto-invalidation on rotation/revocation of the named key.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
