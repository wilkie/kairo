# THREAT_MODEL.md

## Status

Draft. The protocol mechanisms enumerated below are implemented as of
Phase 2 §14; the gaps called out in §6 are tracked in
`specs/PHASE_2.md` and §10 of this document. Section numbers in the
referenced specs reflect the state of those documents at draft time.

## 1. Purpose

This document is the consolidated security argument for Kairo. It
catalogs:

- the **assets** the protocol is responsible for protecting,
- the **adversaries** it is designed to defeat,
- the **attacks** those adversaries might attempt,
- the **mechanism** that defends each one (with cross-references to
  the spec that defines it),
- the **residual risk** that remains after the mechanism does its job,
- and the **explicit non-goals** — attacks the protocol does not try
  to defend against, and the operational disciplines that take over
  where the protocol stops.

Most of this content already exists, scattered across the spec set.
The "Why:" subsections of `specs/ACTORS.md` §5.5.1 (bricking risk),
§5.5.2 (cold-storage attestation), `specs/STATEMENTS.md` §6.1.1
(genesis signature asymmetry), the per-statement canonical specs'
"Excluded Fields" / "Notes" sections, and the property checklist in
`specs/ACTORS.md` §24 each cover one slice of the security argument.
This document assembles them so an outsider can audit the whole thing
in one place.

## 2. Trust Model

Kairo is a **first-person, content-addressed, federated** statement
graph. Every assertion in the system is one of:

- a **content-addressed body** whose hash is its identity (so any
  tampering produces a different ID), or
- a **signed envelope** wrapping a body and binding it to an actor.

There is no central authority. There is no node-wide notion of "true."
Trust is **per-truster**: every actor publishes their own
`ActorTrust` opinions about other actors, and verification reports
trust as one of three independent dimensions (signature, actor
resolution, trust — see `specs/ACTORS.md` §6.2). A statement that is
cryptographically valid is **not** automatically authoritative; a
statement from an untrusted actor is **not** automatically invalid.

Identities are stable: an `ActorId` is a hash over the
`ActorGenesis` body (including a nonce, the initial signing key, and
the attestation key set). Once minted, the `ActorId` cannot be
re-derived without producing a different identity. Continuity of the
actor's **operational** signing surface is provided by the rotation
chain (`ActorKeyRotation` / `ActorKeyRevocation` /
`ActorEmergencyKeyRotation` / `ActorEmergencyKeyRevocation`); the
**recovery** surface is the append-only attestation set
(`ActorAttestationKeyAdd`).

## 3. Assets

What the protocol is protecting, in rough decreasing order of
load-bearing-ness:

| Asset | Definition | Load-bearing for |
|---|---|---|
| `ActorId` | Stable identity hash over `ActorGenesis` body | Every signed statement, every trust opinion, every capability grant |
| Active signing key | The key currently authorized to sign operational statements for an actor (per the rotation chain) | All operational authorship: revisions, branches, tags, capability grants, trust |
| Attestation key set | Append-only set of cold-storage keys authorized to sign emergency key events | Recovery from active-key loss/compromise |
| Statement integrity | Signed bytes match canonical bytes match `StatementId` | Every assertion in the graph |
| Capability authority | The chain `(grantor, grantee, scope) → grant` and its delegation transitive closure | Cross-actor authority claims (`ObjectVersionTag`, `ObjectBranch supersedes`) |
| Object lineage | `ObjectId` + revision graph + branch/tag pointers | Identity and provenance of work products |
| First-person trust | An actor's `ActorTrust` opinions about other actors | Local verification of "should I act on this?" |

## 4. Adversaries

Each adversary is defined by their **capabilities** (what they can
observe and what they can mutate), not by their motivation.

### 4.1 Passive observer (network)

Sees federated traffic in flight. Cannot mutate it. Cannot inject new
statements into a peer's local store.

### 4.2 Malicious peer (federation)

A federation peer the local node has chosen to receive statements
from. Can present any signed statement they want, including:

- statements they observed legitimately and re-broadcast,
- statements they forged with keys they actually hold (e.g. they are
  themselves an actor),
- statements they refuse to relay (selective non-delivery),
- statements that fork an existing chain (presenting two leaves where
  there should be one).

Cannot forge signatures for actors whose private keys they do not
hold.

### 4.3 Compromised active signing key

Adversary has obtained the **operational** signing key for some
actor X. They can sign new operational statements as X. They can sign
a routine `ActorKeyRotation` and rotate to a key they control. They
can sign a routine `ActorKeyRevocation` of any key.

They **cannot** sign emergency key events — those require the
attestation surface (`ACTORS.md` §6.1, §5.5.2). They **cannot** add
attestation keys — `ActorAttestationKeyAdd` is signed by an existing
attestation key, never by the operational surface
(`schemas/canonical/actor-attestation-key-add-v1.md` "Authority to
Add").

### 4.4 Compromised attestation key

Adversary has obtained one of actor X's **attestation** keys. They
can sign:

- `ActorEmergencyKeyRotation` — rotate the active key to one they
  control, locking the legitimate operator out of operational signing.
- `ActorEmergencyKeyRevocation` — revoke any key the actor has held
  (operational or implicit-genesis), retroactively if they choose.
- `ActorAttestationKeyAdd` — add their own attestation key,
  permanently joining the attestation set (v1: no in-protocol
  removal).

They **cannot** revoke or remove existing attestation keys — there is
no `ActorAttestationKeyRevocation` in v1 (`ACTORS.md` §5.5.2). The
legitimate operator's other attestation keys, if any, remain valid.

### 4.5 Compromised both surfaces

Adversary holds both an active signing key and at least one
attestation key for actor X. Equivalent to having full control of
the actor's identity. The protocol has no in-protocol remediation;
recovery is social (§7).

### 4.6 Malicious relay (federation infrastructure)

Operates a federation hop the local node depends on. Strict superset
of §4.2's selective-non-delivery and equivocation-amplification
capabilities, plus they can drop statements from specific actors
(censorship), reorder delivery to amplify forks, or stall delivery
to bias the verifier toward Indeterminate.

### 4.7 Hostile local environment

Has shell access (or worse, root) on the machine running Kairo. Can
read `~/.kairo/keys/*` (mode 0600 on Unix; weaker on platforms
without POSIX perms). Can read or modify any file in `~/.kairo/`. Can
intercept the operator's keystrokes during seed entry.

### 4.8 Hostile actor in the social graph

A legitimate actor in the protocol who behaves adversarially: lies
about trust, issues capabilities they intend to abuse, equivocates by
publishing contradictory statements from two devices, or attempts
sybil flooding (creating many actors cheaply).

### 4.9 Cryptographic adversary

Has unbounded compute against ed25519 in a future where ed25519 is
broken; or has access to a quantum computer. Out of scope for v1.

## 5. Defended-against attacks

Each row maps an attack to the protocol mechanism that defends it.
"Residual risk" is what remains after the mechanism does its job.

### 5.1 Forgery of operational statements

| | |
|---|---|
| **Adversary** | §4.2, §4.6 (peers/relays) |
| **Attack** | Present a statement claiming actor X authored some body, without holding X's active signing key. |
| **Mechanism** | The verifier resolves X's active key at the statement's `created_at` per the rotation chain (`ACTORS.md` §6.1) and checks the signature bytes against it. A forged signature fails byte verification; a signature with a stale `key_id` (e.g. a rotated-out key) is rejected as `KeyMismatch` even if the bytes happen to verify. |
| **Residual risk** | Adversary must compromise X's active key to succeed. See §5.10. |

### 5.2 Tampering with statement bodies

| | |
|---|---|
| **Adversary** | §4.2, §4.6 |
| **Attack** | Modify a body before relaying it. |
| **Mechanism** | `StatementId = hash(canonical_bytes)`. Any tampering produces a different ID, which fails the content-address fixity check on read (`STATEMENTS.md` §2). The signature is over the canonical bytes too, so even unsigned-but-content-addressed bodies (`ActorGenesis`) cannot be tampered with without producing a different `ActorId`. |
| **Residual risk** | None at the protocol layer. |

### 5.3 Substitution of `ActorGenesis` initial key or attestation set

| | |
|---|---|
| **Adversary** | §4.2 (peer claiming to introduce actor X) |
| **Attack** | Publish a forged `ActorGenesis` with a key set the adversary controls, claiming it is X's genesis. |
| **Mechanism** | `ActorId` is the hash of the `ActorGenesis` body, including `initial_key`, `attestation_keys`, `actor_kind`, `nonce`, and `created_at` (`schemas/canonical/actor-genesis-v1.md`). The forged body produces a different `ActorId` and is therefore a different actor; it cannot impersonate X. |
| **Residual risk** | Forged actors can flood the namespace cheaply (sybil; see §6.4). |

### 5.4 Replay of an old statement

| | |
|---|---|
| **Adversary** | §4.2, §4.6 |
| **Attack** | Re-broadcast a legitimately-signed statement to confuse ordering. |
| **Mechanism** | `created_at` is part of canonical bytes; the same body at a different time has a different `StatementId`. Resolution is deterministic per chain (chain leaf wins on `supersedes`; fork tiebreak on `(created_at, statement_id)` descending). Replays of the same `StatementId` are idempotent; replays of "the same content but earlier" are simply older statements that lose chain precedence. |
| **Residual risk** | None at the protocol layer. |

### 5.5 Stale-key signing after rotation

| | |
|---|---|
| **Adversary** | §4.3 with a previously-rotated-out key still in their possession |
| **Attack** | Sign a new statement with a key that was the active key in the past but has since been rotated away from. |
| **Mechanism** | "Active key at causal position T" — the verifier resolves the active key as of the statement's `created_at` per the rotation chain leaf (`ACTORS.md` §5.5). A statement signed by a key that is not the active key at T fails as `KeyMismatch`, regardless of whether that key was once legitimate. |
| **Residual risk** | Statements signed by the formerly-active key with a fraudulent earlier `created_at` would be valid against the active key at *that* time. The protocol cannot prove `created_at` is honest — it's the actor's self-claim. Trusting `created_at` is a local-policy choice. |

### 5.6 Compromised key signing prior bytes

| | |
|---|---|
| **Adversary** | §4.3 |
| **Attack** | After compromising X's active key, sign new statements that are now indistinguishable from X's legitimate prior authorship. |
| **Mechanism** | `ActorKeyRevocation` with `retroactive = true` invalidates **all** statements signed by the named key, regardless of when they were signed (`ACTORS.md` §5.5, `schemas/canonical/actor-key-revocation-v1.md`). This cascades: capability grants signed by a retroactively-revoked key flip to invalid, taking their dependent statements with them. |
| **Residual risk** | Anything the adversary signed before the operator noticed the compromise has *already* federated. Retroactive revocation invalidates it locally and at honest peers, but a malicious peer (§4.2) can choose not to honor retroactive revocations. Local policy may also refuse to honor very-old retroactive revocations (`CAPABILITIES.md` §6.3). The audit trail still records that the bytes existed. |

### 5.7 Capability scope abuse

| | |
|---|---|
| **Adversary** | §4.8 (a grantee abusing a grant) |
| **Attack** | Use a capability grant beyond its intended scope: sign for the wrong object, sign a kind not in `statement_kinds`, exceed the delegation depth, sign past the expiry. |
| **Mechanism** | `evaluate_capability` walks the chain leaf for `(grantor, grantee, scope)` and checks every constraint: `ExpiresAt`, `MaxDelegationDepth`, `KeyPinned`, plus per-`(scope, kind)` validity (`CAPABILITIES.md` §6.1, §7). `KeyPinned` constraints auto-invalidate the grant when the pinned key is revoked, regardless of `retroactive`. |
| **Residual risk** | The grantor must actually use the constraints; an unconstrained `delegable` grant transitively trusts every grantee's grantee. |

### 5.8 Cross-actor abuse of first-person events

| | |
|---|---|
| **Adversary** | §4.8 holding a covering capability |
| **Attack** | Use a delegated capability to rotate, revoke, or chain-supersede statements that belong to a *different* actor. |
| **Mechanism** | Cross-actor `supersedes` is **invalid** for `ActorKeyRotation`, `ActorKeyRevocation`, `ActorEmergencyKeyRotation`, `ActorEmergencyKeyRevocation`, `ActorAttestationKeyAdd`, and `ActorTrust`. Capabilities cannot delegate first-person speech acts. |
| **Residual risk** | None at the protocol layer for these statement kinds. |

### 5.9 Bricking via revocation of the only operational key

| | |
|---|---|
| **Adversary** | An operator making a mistake, or §4.7 with brief shell access trying to lock the operator out. |
| **Attack** | Sign `ActorKeyRevocation` of the only active key, with the active key itself, leaving the actor with no operational signing surface. |
| **Mechanism** | The CLI (`kairo actor revoke-key`) refuses to revoke the only active key without `--brick-actor` (`ACTORS.md` §5.5.1). The protocol layer makes no such check; direct callers of the body must enforce it. Even if bricking happens, recovery is possible from the attestation surface (`ActorEmergencyKeyRotation` introduces a fresh active key). |
| **Residual risk** | If bricking happens **and** all attestation keys are also lost or compromised, see §7 (social recovery). |

### 5.10 Compromised active key, attestation surface intact

| | |
|---|---|
| **Adversary** | §4.3 |
| **Attack** | Operate as the actor until the operator notices. Possibly publish an `ActorKeyRotation` to a key the adversary controls, locking the operator out of operational signing. |
| **Mechanism** | Operator pulls an attestation seed from cold storage and runs `kairo actor recover-key sign` (or the prepare/import flow). This signs an `ActorEmergencyKeyRotation` with the attestation key, introducing a fresh active key. The operator typically follows with an `ActorEmergencyKeyRevocation` (retroactive) of the compromised key. The active-key chain spans both routine and emergency rotations, so the chain leaf becomes the operator's fresh key. |
| **Residual risk** | The window between compromise and recovery. Statements the adversary signed in that window have already federated; retroactive revocation invalidates them on honest peers but a malicious peer (§4.2) can choose not to honor it. The audit trail still records that the adversary held the key. |

### 5.11 Compromised attestation key, active surface intact

| | |
|---|---|
| **Adversary** | §4.4 |
| **Attack** | Sign an `ActorEmergencyKeyRotation` to an active key the adversary controls. The operator is now locked out of operational signing. The adversary can also `ActorAttestationKeyAdd` their own attestation key, which becomes a permanent fixture of the attestation set in v1. |
| **Mechanism** | **There is no in-protocol recovery in shipped v1.** The legitimate operator's remaining attestation keys (if any) cannot revoke the compromised one — `ActorAttestationKeyRevocation` is designed (single-key, attestation-signed, must leave a non-empty set, no `retroactive` flag) but not yet implemented; see Phase 2 §14 follow-on. Until it ships, the only recourse is publishing a fresh `ActorGenesis` (different `ActorId`) and re-establishing identity socially (§7). |
| **Residual risk** | The compromised attestation key remains valid for emergency operations forever in shipped v1. Even after `ActorAttestationKeyRevocation` ships, recovery-surface symmetry means a compromised attestation key can revoke the operator's legitimate attestation keys before being revoked itself — so closing this gap reduces the attack window from "permanent" to "race against the operator's monitoring". **This is the single largest gap in the v1 threat model** and is tracked as a Phase 2 §14 follow-on. |
| **Reduces further with M-of-N attestation thresholds (Phase 2 §14 follow-on, design locked).** When `attestation_threshold > 1`, single-key compromise no longer authorizes any emergency event — the attacker needs ≥ threshold *distinct* attestation keys to sign anything on the recovery surface. Compromise of a single key drops to a detection event (the next legitimate emergency operation will fail with sub-threshold count, surfacing the gap), not an immediate takeover. The attack only succeeds against actors configured at threshold = 1. |

### 5.12 Adversary adds a malicious attestation key

| | |
|---|---|
| **Adversary** | §4.4 (this attack reduces to compromising an attestation key first; see below) |
| **Attack** | Publish an `ActorAttestationKeyAdd` registering a key the adversary controls, so the adversary retains attestation-surface authority even if the original compromised key is later understood to be compromised. |
| **Mechanism for prevention** | `ActorAttestationKeyAdd` must be signed by an existing attestation key (`schemas/canonical/actor-attestation-key-add-v1.md` "Authority to Add"). The operational signing surface **cannot** add attestation keys, even if it is the only key the operator currently holds. So this attack reduces to §4.4 (the adversary already needed to compromise an attestation key to perform it). |
| **Mechanism for detection** | An unexpected `ActorAttestationKeyAdd` appears in `kairo actor key-history` (both surfaces of which the operator should monitor). The signing `key_id` on the add reveals which attestation key was used. |
| **Mechanism for response** | None in shipped v1. The newly-added attestation key joins the append-only set permanently. Same recourse as §5.11: fresh `ActorGenesis` + social recovery. The Phase 2 §14 follow-on `ActorAttestationKeyRevocation` (design locked) will let the operator revoke the malicious add by signing with any other attestation key, subject to the non-empty-set rule. |
| **Residual risk** | In shipped v1 this attack turns a one-time attestation-key compromise into a permanent authority for the adversary. The operator's monitoring is purely informational; there is no removal mechanism. **This is the same gap as §5.11 and is the strongest motivation for `ActorAttestationKeyRevocation` as Phase 2 §14 follow-on work.** |
| **Closed by M-of-N attestation thresholds (Phase 2 §14 follow-on, design locked).** `ActorAttestationKeyAdd` requires ≥ threshold distinct attestation signatures, same as every other emergency event. With `attestation_threshold > 1`, a single compromised attestation key cannot inject an adversary-controlled key. The attack reduces to "compromise threshold attestation keys," which is the §5.11 escalation pattern at higher cost. |

### 5.13 Equivocation (forked chain)

| | |
|---|---|
| **Adversary** | §4.3 (signing from two devices) or §4.6 (relay manufacturing the appearance of a fork) |
| **Attack** | Sign two `ActorKeyRotation` statements (or two `ObjectVersionTag` statements, or two `ActorTrust` statements) at the same chain position, presenting two valid leaves. |
| **Mechanism** | Forks are not blocked. The resolver picks a leaf via fork tiebreak (`(created_at, statement_id)` descending) but **preserves both** for audit. A forked rotation chain is almost always a sign of compromise or split-brain operator and should be surfaced operationally (`schemas/canonical/actor-key-rotation-v1.md` "Chain Validation"). |
| **Residual risk** | The chosen tiebreak is deterministic; honest peers reach the same conclusion. The legitimate operator may still need to act on the audit signal — Kairo doesn't decide for them which leaf is theirs. |

### 5.14 Genesis-asymmetry impersonation

| | |
|---|---|
| **Adversary** | §4.2 publishing a forged `ActorGenesis` |
| **Attack** | Since `ActorGenesis` is unsigned, can the adversary create one in someone else's name? |
| **Mechanism** | The `ActorId` is derived from the body, including its public key, nonce, kind, and timestamp. A body with someone else's stolen public key produces a different `ActorId` than the legitimate actor's (different nonce / kind / timestamp), and that `ActorId` is inert because the adversary doesn't hold the private key to sign anything as it (`STATEMENTS.md` §6.1.1). There is no impersonation vector to close. `ObjectGenesis` is signed (it asserts external authority — "actor X created object Y") and verified against actor X's resolved key. |
| **Residual risk** | None at the protocol layer. The asymmetry is intentional. |

### 5.15 Selective non-delivery / Indeterminate flooding

| | |
|---|---|
| **Adversary** | §4.2, §4.6 |
| **Attack** | A federation peer withholds specific statements (the ones that would invalidate something the adversary wants the local node to accept). The local resolver returns `Indeterminate` rather than `Invalid`. |
| **Mechanism** | The verification report distinguishes `Invalid` (rule was checked and failed) from `Indeterminate` (predecessor data is missing) and from `ResolverUnavailable` (transient). Trust is per-truster; a node can choose not to federate from peers that consistently produce Indeterminate. Multi-peer redundancy is the operational defense — out of scope for v1. |
| **Residual risk** | If the operator only federates from one peer, that peer's selective withholding is undetectable from the protocol. Future federation work (`PHASE_2.md` §4) addresses multi-peer reconciliation. |

## 6. Known gaps and explicit non-goals

These are attacks the v1 protocol **does not defend against**, by
design or by deferral. Listing them here is itself a defense — it
makes the attack surface auditable.

### 6.1 No in-protocol recovery from attestation-key compromise

See §5.11 and §5.12. Shipped v1 has no `ActorAttestationKeyRevocation`.
The attestation set is append-only. A compromised attestation key
remains authoritative until the actor is retired.

**Design locked, implementation deferred (Phase 2 §14 follow-on).**
The shape is:

- **Body:** `{ revoked_key: KeyId, reason: Option<String> }`. Single-key
  revocation, no batch.
- **Surface:** attestation. Signed by any current attestation key,
  including the key being revoked itself.
- **Non-empty-set rule:** revocation invalid if it would leave the
  attestation set empty. Operators with only one attestation key must
  `ActorAttestationKeyAdd` before revoking. Symmetric with §5.9.
- **No `retroactive` flag.** Asymmetric with `ActorKeyRevocation` by
  design: attestation keys never sign consequential statements
  directly — they only sign emergency events that introduce or
  modify operational keys. Cleanup of damage done with a compromised
  attestation key is therefore a routine `ActorKeyRevocation
  { retroactive: true }` against the malicious operational key the
  emergency event introduced. The attestation revocation only stops
  the bleeding; historical damage gets unwound at the operational
  layer where it accrued.

**Recovery-surface symmetry remains under threshold = 1.** Any power
the attestation surface gives the operator, it gives an attacker who
holds the key. A compromised attestation key can revoke legitimate
attestation keys before being revoked itself (subject to the
non-empty-set rule). The primitive reduces the attack window from
"permanent" to "race against the operator's monitoring"; it does not
close the "all attestation keys compromised" scenario, which remains
social recovery (§7).

**M-of-N attestation thresholds (Phase 2 §14 follow-on, design locked).**
Symmetry above is the reason a single attestation key is a single point
of failure. The threshold follow-on raises the cost of recovery-surface
compromise from "one key" to "k of n keys," matching TUF root, DNSSEC
KSK ceremonies, and modern multisig cold-storage practice. The locked
design adds:

- `ActorGenesis.attestation_threshold: u8` (required, no default).
- All five attestation-surface emergency types carry
  `signatures: Vec<Signature>`; the verifier requires ≥ threshold
  distinct signatures from the attestation set at `created_at`.
- New `ActorAttestationThresholdChange` emergency type with
  asymmetric authority: raises require `max(current, new)` distinct
  signatures (so an attacker just-barely at threshold cannot
  consolidate by lowering); lowers require `current` signatures.
- The §5.5.2 set-size guard generalizes to "resulting set size
  ≥ resulting threshold."
- M-of-M plus a single lost key bricks recovery (`current` sigs are
  no longer reachable). Operator hygiene: use M-of-N with N > M.

After thresholds ship, single-key compromise of any attestation key
becomes a *detection event* (next legitimate emergency operation
fails sub-threshold and surfaces the gap), not an immediate takeover.
Recovery-surface symmetry survives only when the attacker holds
≥ threshold keys — which is the same escalation pattern as
"all attestation keys compromised" at proportional cost.

### 6.2 No quantum / post-quantum signature support

Ed25519 is the only required algorithm. A future quantum adversary
breaks every signature in the system. Algorithm rotation (a parallel
"v2 active surface" alongside the ed25519 surface, with a flag-day
migration) is not in scope for v1.

### 6.3 No defense against the hostile local environment

The keystore is plain JSON files at `~/.kairo/keys/<actor-id>.json`,
mode 0600 on Unix, no perms enforcement on platforms without POSIX
perms. Anything with read access to the user's home directory has the
operator's keys (`ACTORS.md` §18). Passphrase-encrypted keystores are
documented as future work in `project_keystore_design.md`. Hostile
root is undefended.

The attestation surface mitigates this somewhat — the seed never
enters Kairo's process memory in the operator-presented path, so a
hostile local user cannot steal it from `~/.kairo/`. They could still
keylog the operator's seed entry into `kairo actor recover-key sign`
or substitute the binary. Defense requires OS-level attestation,
which is out of scope.

### 6.4 No sybil resistance

Creating an `ActorGenesis` is computationally trivial. There is no
proof-of-work, no required identity attestation, no fee. A node can
mint millions of actors. Trust is per-truster: a node-wide "this
actor exists" is meaningless; what matters is whether actors the
local node trusts have published opinions about an actor. Sybil
resistance is therefore an emergent property of the trust graph,
not a protocol guarantee.

### 6.5 No defense against denial-of-service

Federation rate limits, network availability, disk-fill attacks,
storage exhaustion via malicious bundles — all out of scope. A
malicious peer can flood a node with garbage statements that all
fail verification but consume CPU and disk. Operational concern,
not a protocol concern.

### 6.6 No defense against traffic analysis

Federation reveals who federates with whom, what objects they're
interested in, when statements appear. End-to-end transport encryption
prevents eavesdropping on body bytes (assuming the federation transport
uses TLS), but the metadata is observable to any on-path adversary or
relay. Tor-level anonymity is out of scope.

### 6.7 No defense against timing / power side channels

Ed25519 implementations are constant-time in practice, but Kairo does
not audit or guarantee that the operator's signing environment is
side-channel-free. An adversary with physical access to the operator's
device may extract keys through power analysis or timing. Out of scope.

### 6.8 `created_at` is unverified

Every signed body carries a `created_at` (`STATEMENTS.md` §2,
`schemas/canonical/*-v1.md`). The protocol uses it as the causal
position for active-key resolution and for ordering chain leaves.
**It is not a trusted observation** — the actor can claim any
timestamp. The system is robust to this in the sense that "lying about
`created_at` cannot violate signature verification" (the active key at
the claimed `T` is what's checked), but it can be used to game fork
tiebreaks or to predate retroactive revocations. Local policy may
choose to reject statements whose `created_at` is grossly out of band
(very far in the past or future), but that's a policy choice, not a
protocol guarantee.

### 6.9 No multi-process safety in the local store

`~/.kairo/` is not protected against concurrent writers. Two processes
performing `put_*` simultaneously can corrupt index files. Phase 2 §6
tracks file locks; until then, single-process operation is the
operational discipline.

### 6.10 No bundle-level signature

Bundles (`PACKAGE.md`) are validated statement-by-statement on import
but the bundle itself is not signed. A malicious peer can swap one
bundle for another (with different statements but a valid manifest).
Phase 2 §11 tracks bundle-level signatures.

## 7. Social recovery

When an attack leaves an actor with no in-protocol path back to their
identity, recovery is **social**: the actor publishes a fresh
`ActorGenesis` (different `ActorId`, since the canonical bytes are
different), and the actor's peers update their world view to point at
the new identity.

Social recovery is the **only** remediation when:

1. **All attestation keys are compromised or lost** (§5.11, §5.12).
   The active key may still be safe, but there is no in-protocol way
   to re-establish a clean recovery surface.
2. **An attestation key is compromised and its holder cannot be
   removed** (v1 gap §6.1). The legitimate operator has lost the
   ability to definitively claim "I am the only authorized signer."
3. **Active key + attestation key both compromised** (§4.5). The
   adversary has full control of the actor.
4. **Bricking** (§5.9) **plus loss of all attestation keys**. No
   in-protocol signing surface remains.

What social recovery requires:

- The legitimate operator publishes a new `ActorGenesis` with fresh
  signing and attestation keys.
- Peers who trusted the old `ActorId` publish new `ActorTrust`
  statements about the new `ActorId`. Trust is per-truster, so this
  is a per-peer operation. Peer-friendly UX (e.g. "this user's old
  identity says it has been replaced by this new one, signed by both
  keys") would help bootstrap, but is **not in scope** for v1 — the
  v1 mechanism is "publish new identity, ask peers to re-trust out
  of band."
- Capability grants previously issued by the old actor are inert
  for the new identity; the new actor must re-issue them.
- Object lineage previously authored by the old actor remains valid
  in the historical record; the new actor cannot "claim" the old
  actor's history but can publish revisions and tags from the new
  identity going forward.
- Branches and tags previously published by the old actor remain
  resolvable as historical chain leaves; the new actor publishes
  fresh chain leaves under their new identity, which become the
  current heads going forward.

The protocol does not define a "this is my new identity" linkage
statement signed by both old and new keys. Such a statement would be
a useful affordance (so peers can mechanically migrate trust), but
it is **not** part of v1 — the existence of such a statement signed
by a compromised old key would be an attack vector (the adversary
could "migrate" to a new identity they control). v2 designs should
consider whether such a statement can be made safely (perhaps signed
by an attestation key the adversary doesn't hold, with an
out-of-band confirmation step).

The takeaway: **social recovery is not a fallback the protocol owns
— it is a fallback the operator and their peers own**. Kairo's
contribution is making it cheap to mint a fresh identity (no
proof-of-work) and making it clear in the audit trail when an old
identity has been retired (the old `ActorId` simply stops issuing
statements).

## 8. Operator monitoring

Several attacks above (§5.10, §5.11, §5.12) leave audit signals that
the legitimate operator can detect, but only if they are looking. The
protocol's defense in those scenarios is partly cryptographic and
partly **operational**:

- An unexpected `ActorEmergencyKeyRotation` in `kairo actor key-history`
  signals a possible attestation-key compromise.
- An unexpected `ActorAttestationKeyAdd` in `kairo actor key-history`
  signals the same; the signing `key_id` reveals which existing
  attestation key was used.
- An unexpected `ActorKeyRotation` (routine) signals a possible
  active-key compromise.
- A forked rotation chain — two leaves at the same supersedes
  position — almost always indicates compromise or split-brain
  operator.

The CLI surface for inspecting these is `kairo actor key-history`.
Operators are expected to monitor it on a cadence appropriate to
their threat profile. Protocol-level alerting (push notifications,
peer attestation that "your key just rotated") is out of scope for
v1.

## 9. Cross-references

The mechanisms cited above are defined in:

- **Identity & key chain:** `specs/ACTORS.md` §3, §5, §6.1
- **Cold-storage attestation:** `specs/ACTORS.md` §5.5.2,
  `schemas/canonical/actor-emergency-key-rotation-v1.md`,
  `schemas/canonical/actor-emergency-key-revocation-v1.md`,
  `schemas/canonical/actor-attestation-key-add-v1.md`
- **Statement integrity:** `specs/STATEMENTS.md` §2, §3, §6
- **Genesis signature asymmetry:** `specs/STATEMENTS.md` §6.1.1
- **Capability model:** `specs/CAPABILITIES.md` §6, §7, §8
- **Trust model (per-truster):** `specs/ACTORS.md` §6.2,
  `schemas/canonical/actor-trust-v1.md`
- **Property checklist:** `specs/ACTORS.md` §24
- **Future work:** `specs/PHASE_2.md` §6 (file locks),
  §11 (bundle signature), §13 (this document; threat model
  hardening), §14 (cold-storage attestation;
  `ActorAttestationKeyRevocation` as follow-on)

## 10. Implementation status

What of this threat model is mechanically defended today (Phase 2 §14
shipped):

- §5.1 forgery, §5.2 tampering, §5.3 genesis substitution, §5.4
  replay, §5.5 stale-key, §5.6 retroactive revocation, §5.7
  capability scope, §5.8 cross-actor first-person events, §5.9
  bricking guard at the CLI, §5.10 active-key compromise recovery,
  §5.13 fork preservation, §5.14 genesis-asymmetry impersonation:
  all defended.
- §5.11 attestation-key compromise, §5.12 malicious attestation-key
  add: defended **only by detection** in shipped v1, not by
  in-protocol response. `ActorAttestationKeyRevocation` is designed
  but not yet implemented (§6.1, Phase 2 §14 follow-on); until it
  ships, recovery is social (§7).
- §5.15 selective non-delivery: detection is partial (`Indeterminate`
  vs `Invalid`); reconciliation across multiple peers is Phase 2 §4.
- §6.1 through §6.10: explicit non-goals or deferred work; not
  defended in v1.

The single most load-bearing follow-on is `ActorAttestationKeyRevocation`
(closing §6.1 / §5.11 / §5.12), tracked as Phase 2 §14 follow-on.
The design is locked (single-key, attestation-signed, non-empty-set
rule, no `retroactive` flag — see §6.1); spec / impl / CLI slices
are queued.
