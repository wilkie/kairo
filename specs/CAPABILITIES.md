# CAPABILITIES.md

## Status

Draft specification.

This document defines the **actor capability model** used in Kairo for
delegated authority — the mechanism by which one actor authorizes another
actor to issue specific statement kinds on a scoped target on its behalf.

This is the distributed-systems sense of "capability": a transferable,
unforgeable token of authority. It is unrelated to runtime sandbox
capabilities (`SANDBOX.md`).

---

## 1. Purpose

Capabilities exist to make cross-actor authority claims load-bearing in the
statement graph. Phase 1 deliberately deferred them; several pending
features require them as a precondition:

- `ObjectVersionTag` cross-actor `supersedes` (recorded by Phase 1 but not
  honored by the resolver).
- `ObjectBranch v2` cross-actor `supersedes` (Phase 2 §12, deferred until
  this spec lands).
- Multi-maintainer flows on a single object.
- Federation policy where one node delegates to another (`POLICY.md`).

Without capabilities, every authority claim is bounded by the actor that
signed it. Capabilities widen that boundary in a controlled, auditable
way.

---

## 2. Relationship to other specs

- `STATEMENTS.md` defines the statement shape that capability grants and
  revocations follow.
- `ACTORS.md` §10–12 contains seed prose this document supersedes. After
  this spec settles, those sections should shrink to a forward reference.
- `OBJECT.md` defines the object root authority that anchors capability
  chains for object-scoped grants.
- `POLICY.md` defines local trust decisions; capability validity is
  semantic, but local policy may still refuse to honor a valid capability.
- `SANDBOX.md` is the unrelated runtime-permissions surface.

Important distinction:

```text
Capability authority is semantic.
Local trust is policy.
```

A capability may be cryptographically valid and semantically authorized
while local policy still refuses to act on it.

---

## 3. Term overload

Two unrelated concepts share the word "capability":

| Term               | Meaning                                                       | Spec         |
|--------------------|---------------------------------------------------------------|--------------|
| Actor capability   | Delegated authority for actor B to act on behalf of actor A   | this doc     |
| Sandbox capability | Runtime permission for an executing artifact (fs, net, …)     | `SANDBOX.md` |

When this spec says "capability" without qualification, it always means
*actor capability*.

---

## 4. Core model

A **capability** is a structured authorization issued by a *grantor* actor
to a *grantee* actor, naming:

- a **scope** (which target the authorization applies to),
- a **set of statement kinds** the grantee may issue on that scope,
- optional **constraints** (expiration, delegation depth, etc.).

A capability comes into being via an **`ActorCapabilityGrant`** statement,
signed by the grantor, and is retracted by an
**`ActorCapabilityRevocation`** statement signed by an actor with
revocation authority over the grant.

### 4.1 Capability shape

Illustrative Rust:

```rust
pub struct Capability {
    pub scope:           CapabilityScope,
    pub statement_kinds: Vec<StatementKind>,
    pub delegable:       bool,
    pub constraints:     Vec<CapabilityConstraint>,
}
```

### 4.2 `CapabilityScope`

```rust
pub enum CapabilityScope {
    /// The named object. Authorized statement kinds live in
    /// `Capability::statement_kinds`.
    Object(ObjectId),
    /// The grantor's own actor surface (e.g. metadata, key
    /// management). Authorized statement kinds live in
    /// `Capability::statement_kinds`.
    Actor(ActorId),
}
```

The MVP scope vocabulary is deliberately narrow — these two variants
are sufficient for the cross-actor supersedes use case. Kind narrowing
is *not* part of scope; it lives entirely in `Capability::statement_kinds`
(Decision E in §9). Wider scopes (snapshot, artifact, runtime, object
family) are reserved for later (see §10 and §11.G).

### 4.2a Statement-kind narrowing

`Capability::statement_kinds: Vec<StatementKind>` is the only mechanism
for narrowing authorization by statement type. Rules:

- **Non-empty.** An empty list is a shape violation; a grant authorizing
  zero kinds is degenerate and rejected at put-time.
- **Canonical-form ordering.** The list is sorted (by `StatementKind`
  enum discriminant) and deduplicated in canonical bytes. JSON
  interchange may be unordered; the canonical encoder normalizes.
- **Explicit enumeration only.** No wildcard variant. Adding a new
  statement kind to Kairo later does **not** retroactively expand
  existing grants — operators must issue a superseding grant to pick
  up new kinds. This prevents silent authority creep when the protocol
  grows.
- **Per-`(scope, kind)` shape validity.** Some `(scope, kind)` pairs
  are nonsensical (e.g. `Actor(A) + ObjectRevision`). The validator
  rejects these at put-time. Per-kind validity rules are defined
  alongside each statement-kind specification.

`StatementKind` here refers to the existing kind discriminant defined in
`kairo-statement` — no new vocabulary.

### 4.3 Constraints

```rust
pub enum CapabilityConstraint {
    ExpiresAt(i64),               // epoch seconds; grant invalid for
                                  // statements created strictly after.
    MaxDelegationDepth(u8),       // depth bound on re-grant chains.
    KeyPinned(KeyId),             // grant is bound to a specific grantor
                                  // signing key; revoking that key auto-
                                  // invalidates the grant. See §7.
}
```

Constraints are validated by core when semantic. Local policy may layer
additional constraints. Constraints intentionally start small; the open
universe (`requires_cosigner`, `frontier_pinned`, etc.) is deferred.

`KeyPinned` is the opt-in escape hatch for high-stakes grants where
auto-revocation on grantor key compromise matters more than survivability
across routine key rotations. It is *not* the default — see §7 and
Decision C in §9.

---

## 5. Statement types

### 5.1 `ActorCapabilityGrant`

A signed statement issued by the grantor:

```rust
pub struct ActorCapabilityGrantBody {
    pub grantor:    ActorId,            // signer of the envelope
    pub grantee:    ActorId,
    pub capability: Capability,
    pub created_at: i64,                // RFC 3339 UTC seconds in JSON;
                                        // i64 epoch seconds BE in canonical
    pub supersedes: Option<StatementId>, // chain edge to prior grant
                                         // on the same triple
}
```

`supersedes` is **first-person**: only the same `grantor` may supersede
their own prior grant. Cross-grantor supersession is invalid, matching
`ActorTrust`.

#### 5.1.1 The `(grantor, grantee, scope)` triple — single chain rule

A "triple" is `(grantor, grantee, scope)` where scope is whichever
`CapabilityScope` variant the grant uses. Each triple has **at most one
active chain** at any causal position:

- The **genesis grant** for a triple is just the first grant in its
  chain — `supersedes = None`. No special marker.
- Successor grants on the same triple **must** declare `supersedes`
  pointing at a prior chain member.
- A second `supersedes = None` grant on an existing triple is a shape
  violation, rejected at put-time (Decision G in §9).

Consequence: "what is the grantee's authority on this scope from this
grantor right now?" reads exactly one chain leaf. The leaf's
`statement_kinds` is the complete authorized set. There is no
union-of-multiple-grants resolution.

To extend or modify the authorized kinds, the grantor issues a
successor with `supersedes` set and the desired *complete* kind list.
For example, to add kind `c` to a grant currently covering `[a, b]`,
issue a successor with `statement_kinds = [a, b, c]`. Omitting a kind
in a successor implicitly revokes it (within that triple) at the
successor's causal position.

A self-grant (`grantor == grantee`) is structurally valid but
degenerate — it expresses "I authorize myself," which adds nothing to
the grantor's existing root-or-delegated authority. The validator does
not reject self-grants, but tooling should warn.

### 5.2 `ActorCapabilityRevocation`

```rust
pub struct ActorCapabilityRevocationBody {
    pub grantor:       ActorId,             // signer
    pub revoked_grant: StatementId,         // the ActorCapabilityGrant
                                            // being revoked
    pub created_at:    i64,
    pub retroactive:   bool,                // see §6.2
    pub reason:        Option<String>,
}
```

By default, only the original grantor may revoke a grant. Multi-actor
revocation paths (e.g. an object root authority revoking a grant chain
issued underneath it) are out of scope for the MVP.

### 5.3 Sharding and indexing

Following the `ActorTrust` precedent and the
`kairo-store/AGENTS.md` "uniformity" rule, signed grants and
revocations live in the shared statements directory
(`statements/<XX>/<YY>/<statement-id>.json`), and the per-grantor
materialized index lives at:

```text
actor_capability/<XX>/<YY>/<grantor-id>.json
```

The index file nests `grantee → scope → chain entries`, with a sibling
`revocations` map keyed by the revoked grant's StatementId. Per-grantor
sharding matches the duty model: the grantor maintains and revokes the
grants they issue (Decision A in §9), and the dominant write-path
query — "for this `(grantor, grantee, scope)` triple, what is in
effect?" — is O(1) once the file is loaded.

The cross-cutting "for this `(object, grantee)`, is there any covering
capability?" query — the §6.1 capability evaluator's hot path — is
satisfied via a per-object materialized reverse index at:

```text
actor_capability_by_object/<XX>/<YY>/<object-id>.json
```

The reverse index nests `grantee → grantor → chain entries[]` and is
written at put-time alongside the per-grantor index (object-scoped
grants only — actor-scoped grants get their own reverse index when
actor-surface kinds land per §4.3).

Canonical encoding follows the project convention
(`schemas/canonical/`) — domain-tagged binary, signed envelope as defined
in `STATEMENTS.md`. The canonical-form spec lives in
`schemas/canonical/actor-capability-grant-v1.md` and
`schemas/canonical/actor-capability-revocation-v1.md`, written when this
spec settles.

---

## 6. Resolution

### 6.1 Capability-at-causal-position

The resolver answers: "does grantee `B` have authority to issue
statements of kind `K` against target `T` at causal position `P`?"

```rust
pub enum ResolutionTarget {
    Object { id: ObjectId, kind: StatementKind },
    Actor  { id: ActorId,  kind: StatementKind },
}

pub fn evaluate_capability(
    grantee: &ActorId,
    target:  &ResolutionTarget,
    at:      CausalPosition,
) -> CapabilityEvaluation;
```

`evaluate_capability` returns `Held` iff there exists a chain-leaf
`ActorCapabilityGrant` `G` such that:

1. `G.grantee = grantee`.
2. `G.capability.scope` matches `target` — i.e. `Object(target.id)` for
   an `Object` target, or `Actor(target.id)` for an `Actor` target.
3. `target.kind ∈ G.capability.statement_kinds`.
4. `G` has not been revoked at `P`, **or** the revocation is non-
   retroactive and `P` is causally strictly before the revocation's
   `created_at`.
5. `G`'s grantor `A` held the appropriate authority to issue `G`:
   - For `CapabilityScope::Object(O)`: `A` must be in `O`'s root
     authority **or** must itself hold a capability on `O` (recursive
     `evaluate_capability` call) with `delegable = true` covering
     `target.kind`.
   - For `CapabilityScope::Actor(A)`: `A` is acting on its own surface
     and no further authority check is needed.
6. All capability constraints (expiration, delegation depth) are
   satisfied at `P`.

Recursion termination: capability chains may delegate, bounded by
`MaxDelegationDepth`. Cycles are detected by tracking the `(grantor,
grantee, scope)` triples already visited on the current resolution
path; a cycle returns `GrantorLacksAuthority` (the recursive call
cannot complete, so `A`'s purported authority is unproven).

The evaluation function returns:

```rust
pub enum CapabilityEvaluation {
    Held,                      // B is authorized for (target.kind, target.id) at P
    NotHeld,                   // no covering grant exists
    Revoked(StatementId),      // covering grant was revoked
    Expired(StatementId),      // covering grant expired
    DelegationTooDeep,         // chain exceeds MaxDelegationDepth
    GrantorLacksAuthority,     // grantor could not have issued the grant
                               // (also returned for cycles)
}
```

### 6.2 Cross-actor supersedes — the load-bearing case

`ObjectVersionTag.supersedes` may name a tag from a different actor on
the same `(object, version)`. The Phase 1 resolver records this edge but
does not honor it. Capability resolution flips that:

> A cross-actor supersedes edge from successor `S'` (signed by actor `B`)
> to predecessor `S` (signed by actor `A`) is honored iff `B` holds a
> capability covering `ObjectVersionTag` statements on `S`'s object at
> the causal position of `S'.created_at`.

Implementation: `kairo-store::FilesystemStore::latest_version_tag` and
`list_version_tags` start from the same-actor chain leaf and walk
forward through any authorized cross-actor supersedes edges using
`evaluate_capability` (`specs/CAPABILITIES.md` §6.1). Same-actor sup
is automatic; cross-actor sup is honored only when the successor's
signer holds an `ObjectVersionTag` capability on the object at the
successor's `created_at`.

The same rule applies to `ObjectBranch v2` cross-actor supersedes when
that schema bump lands (Phase 2 §12).

For `ActorTrust`, cross-actor supersedes remain **invalid** even with
capabilities — see Decision B in §9.

### 6.3 Retroactive revocation

By default, revocation invalidates statements with `created_at` strictly
after the revocation. `ActorCapabilityRevocation.retroactive = true`
invalidates the grant from inception, retroactively invalidating all
statements issued under it. This is destructive — it propagates to every
cross-actor supersedes edge that depended on the grant.

Retroactive revocation should be reserved for cases like grant fraud or
grantee key compromise. Local policy may refuse to honor retroactive
revocations issued long after the fact.

---

## 7. Key rotation interaction

Capabilities anchor on `ActorId` by default, not on signing keys
(Decision C in §9). Consequences for routine rotation:

- A grant signed by a key that later rotates remains valid: the
  signature was valid at the grant's causal position, and key-active-at-
  causal-position is the rule for all signed statements (`ACTORS.md`
  §6.1).
- The grantee continues to exercise the capability with their current
  active key after rotation.

This is deliberate: key rotation is hygiene we want operators to do
freely, and invalidating outstanding grants on rotation would discourage
it.

### 7.1 Grantor key compromise — cleanup runbook

When a grantor key is revoked due to compromise (governed by
`ACTORS.md` §12.3 and §13), grants signed by the compromised key are
**not** auto-invalidated. The legitimate operator must:

1. Enumerate `ActorCapabilityGrant` statements signed by the
   compromised key (audit query: per-grantor shard, filter by signing
   `KeyId`).
2. For each grant the attacker may have issued during the compromise
   window — or any grant the operator no longer endorses post-incident
   — issue an `ActorCapabilityRevocation` with `retroactive = true`.
3. Re-issue any legitimately-needed grants under the new active key.

The audit query is cheap (grants are sharded by grantor; the signing
`KeyId` is in the envelope), so the operational cost is "remember to do
it." Surfacing this as part of the key-revocation UX is tracked as
future work in §11.E.

### 7.2 The `KeyPinned` opt-in

For grants where auto-revocation on grantor key compromise matters more
than survivability across rotations, the grantor may attach a
`CapabilityConstraint::KeyPinned(KeyId)` (see §4.3). A pinned grant
becomes invalid the moment the named key is revoked — no separate
`ActorCapabilityRevocation` is needed. Pinning trades the rotation-
friendly default for stronger compromise hygiene; pick it per-grant for
high-stakes delegations.

This means the capability spec does **not** introduce per-key grants as a
default; per-key behavior is opt-in via `KeyPinned`.

---

## 8. MVP slice

For the first implementation pass:

1. `ActorCapabilityGrant` and `ActorCapabilityRevocation` statement types
   with canonical encoding (paired
   `schemas/canonical/actor-capability-*-v1.md` files).
2. Per-grantor sharding; per-`(grantor, grantee)` listing.
3. `evaluate_capability(grantee, scope, kind, at_position) ->
   CapabilityEvaluation`.
4. `ObjectVersionTag` resolver: honor cross-actor supersedes when
   capability evaluation succeeds (the Phase 1 deferred resolver flip).
5. CLI: `kairo capability grant`, `kairo capability revoke`, `kairo
   capability list`.
6. Materialized cross-cutting index for `(object, grantee)` lookups,
   built on the same put-time path as the trust indices.

Deferred to subsequent iterations:

- `ObjectBranch v2` schema bump (Phase 2 §12).
- Multi-cosigner / threshold capabilities.
- Snapshot-, artifact-, runtime-, build-scoped grants.
- Capability bundle type for federation transport.
- Conditional or data-dependent constraints.
- Time-delayed grant activation.

---

## 9. Decisions

A through G are **locked**. Rationale and considered alternatives are
kept here for the record so future readers can see what was traded
against what.

### Decision A — Statement form: first-person vs. object-scoped — **Locked: first-person**

**Question.** Does `ActorCapabilityGrant` shard primarily by *grantor*
(like `ActorTrust`) or by *object scope* (like `ObjectBranch` /
`ObjectVersionTag`)?

**Decision.** First-person (sharded by grantor). §5.3 reflects this.

**Why.** The grantor holds responsibility for maintaining and revoking
the grants they issue, so per-grantor locality matches the duty model.
The grant is a first-person speech act ("I authorize…"). The same
grantor may issue grants spanning multiple objects in one session. Trust
precedent already shipped with this shape, and the per-grantor shard
naturally hosts revocation chains. The "for a given object, who holds
capabilities?" query is served by a materialized cross-cutting index
that is part of the MVP slice (§8 item 6).

**Alternative considered.** Per-object sharding. Better cache locality
for the "validate this object's branch chain" query, but worse for
"what has actor A delegated lately?" and forces grants that span
multiple objects to be split into multiple statements.

### Decision B — Cross-actor supersedes for `ActorTrust` — **Locked: stays invalid**

**Question.** With capabilities available, should `ActorTrust` cross-
actor supersedes become valid (A grants B the right to publish trust on
A's behalf)?

**Decision.** **No.** Trust remains strictly first-person.

**Why.** `ActorTrust` is intentionally a first-person *opinion*. Letting
another actor speak for A's opinions blurs the semantic — "A trusts X"
ceases to be A's claim. Operationally, the use case (one actor managing
trust on behalf of an organization) is better served by an organizational
actor with multi-sig governance (`ACTORS.md` §14), not by trust
delegation. Indirect (transitive) trust as a tiebreaker — "A trusts X,
X trusts Y, so A kinda trusts Y" — is a separate, useful concept that
operates as a derived computation over the existing first-person trust
graph; it does not require cross-actor supersedes and is tracked
separately (§11.F).

**Alternative considered.** Allow it. Symmetric with branch / version-
tag supersedes once capabilities land. Loses the "first-person opinion"
guarantee.

### Decision C — Key rotation: grant binding — **Locked: ActorId default + opt-in `KeyPinned`**

**Question.** Does the grantor's signing key — the key that signed the
grant — stay load-bearing across rotation, or do grants bind to the
grantor's `ActorId`?

**Decision.** Grants bind to the grantor's `ActorId` by default. The
grantor may opt in to per-key behavior on a specific grant via
`CapabilityConstraint::KeyPinned(KeyId)` (§4.3). §7 reflects this.

**Why.** Key rotation is hygiene we want operators to do freely.
ActorId binding makes routine rotation cheap (no grant re-issuance) and
matches how every other statement type treats actors. The cost is that
key compromise no longer auto-invalidates outstanding grants — the
operator must enumerate grants signed by the compromised key and issue
retroactive revocations as part of the cleanup runbook (§7.1). For
high-stakes grants where compromise hygiene matters more than rotation
cost, `KeyPinned` provides the opposite behavior on demand without
changing the v1 statement shape.

**Alternative considered.** Bind to `KeyId` as the default. Stronger
compromise hygiene (pinned grants auto-die when the key is revoked),
but operationally painful for routine rotations and likely to discourage
good rotation discipline.

### Decision D — Spec-first vs. implement-alongside — **Locked: spec-first canonical bytes, then minimal implementation alongside**

**Question.** Lock the statement-type canonical encoding before any
implementation, or implement the MVP slice (§8) alongside the spec to
catch shape problems early?

**Decision.** **Spec-first for canonical encoding; minimal
implementation alongside.** Lock the canonical bytes via
`schemas/canonical/actor-capability-grant-v1.md` and
`schemas/canonical/actor-capability-revocation-v1.md` first, then
implement the MVP slice (§8) — types, store paths, cross-cutting index,
resolver, `ObjectVersionTag` cross-actor `supersedes` flip, and CLI —
in the same Phase 2 chunk.

**Sequencing within the chunk:**

1. Canonical-form specs for both statement types.
2. `kairo-statement` body types and canonical encoding (with round-trip
   tests).
3. `kairo-store` put / get / list under per-grantor sharding.
4. Cross-cutting `(object, grantee)` materialized index, written at
   put-time.
5. `evaluate_capability` resolver, mirroring `evaluate_trust`.
6. `ObjectVersionTag` resolver: honor cross-actor `supersedes` when
   capability evaluation succeeds. End-to-end integration test.
7. CLI: `kairo capability grant / revoke / list`.

**Why.** Statement schemas are content-addressed: every existing
`StatementId` re-derives if canonical bytes change. Today there is no
deployed installation and no installed base of stores, so the *cost* of
getting the canonical bytes wrong is purely the cost of edit-and-redo
within the workspace — not a migration. Even so, treating canonical
bytes as the first thing to lock is the right discipline: it forces us
to settle the wire shape with full attention before downstream code
depends on specific encodings, and it builds the habit we need before
deployment. The rest of the system (resolver flip, CLI, store layout)
remains free to evolve during implementation.

**Note on freshness.** Because nothing is deployed yet, **any** part of
this spec — including v1 canonical bytes — can be revisited without
external migration cost. Future contributors should not read "v1" as
"already shipped to users"; it means "first version we settled on
internally." The v2-vs-v1 cost framing in this document anticipates
post-deployment churn.

**Alternative considered.** Pure spec-first (no code until the doc is
final): slower, spec drift relative to implementation reality. Pure
implement-first (start coding, write the spec from what works): fastest
to a prototype but loses the canonical-byte discipline we want to build.

### Decision E — Kind-narrowing representation — **Locked: kinds always live in `Capability::statement_kinds`**

**Question.** With the original draft having both a kind-aware scope
variant (`ObjectStatementKind(O, K)`) and a `statement_kinds: Vec<...>`
field, there are two ways to express "narrow to kind K." Pick one
representation.

**Decision.** Drop `ObjectStatementKind` from `CapabilityScope`. Kind
narrowing always lives in `Capability::statement_kinds`. §4.2 and
§4.2a reflect this.

**Why.** The two-field model invited bugs (which field is load-bearing
when scope is already kind-specific?) and added validation surface for
no expressivity gain. Single-source-of-truth for kind narrowing keeps
the resolver flat and makes future scope variants (`Snapshot`,
`Artifact`, `ObjectFamily`) compose without parallel `*StatementKind`
variants. The trade is losing compile-time structural distinction
between "narrow grant intended to be one kind" and "broad grant that
happens to currently list one kind" — a cosmetic loss.

**Alternative considered.** E2 — drop `statement_kinds`, encode kinds
in scope variants like `ObjectStatementKinds(O, Vec<StatementKind>)`.
Equally expressive, but requires parallel kind-aware variants for every
future scope. E3 — keep both with explicit interaction rules. The
current draft before this decision; awkward and bug-prone.

### Decision F — `evaluate_capability` query shape — **Locked: `ResolutionTarget` discriminated union**

**Question.** What does the resolver function take as input?

**Decision.** A `ResolutionTarget` discriminated union pairing a target
identifier with a single statement kind. §6.1 reflects this.

**Why.** Cleanly separates "do you have authority to do X?" from "is
this statement valid?" — callers can ask hypothetical questions
without constructing statements. Avoids the redundancy in alternatives
that took both `scope` and `kind` arguments. Pairs naturally with the
E1 representation: scope target lives in `ResolutionTarget`, kind lives
alongside it, no two-fields-must-agree validation.

**Alternative considered.** F2 — `evaluate_capability(grantee,
&SignedStatement, at)`, where the resolver inspects the statement's
kind and target. Tighter binding to actual usage but harder to ask
hypothetical "could B do X?" questions without constructing X.

### Decision G — Same-triple grants — **Locked: chain or shape violation**

**Question.** When two `ActorCapabilityGrant` statements name the same
`(grantor, grantee, scope)` triple and neither supersedes the other,
what happens?

**Decision.** Shape violation, rejected at put-time. Each triple has at
most one active chain; the second genesis grant is invalid. Successor
grants must declare `supersedes`. §5.1.1 reflects this.

**Why.** Single chain leaf per triple makes "what is the grantee's
authority?" a one-statement read. Set-union semantics across multiple
grants would complicate the resolver, complicate revocation
(revoking which grant kills which authority?), and obscure operator
intent. Rejecting at put-time is kinder than silently picking a winner.

The trade — per-kind constraints cannot vary within a single triple
(one grant carries one constraint set across all its kinds) — is a
real limitation but a fair one for the maintainer-delegation cases this
spec targets. Documented in §11.G as a known trade-off.

**Alternative considered.** G2 — both valid, set-union resolution.
Friendlier but obscures intent and complicates revocation. G3 — both
valid, `(created_at, statement_id)` lex tiebreak picks one. Mirrors
`ObjectBranch v1` precedent but introduces the same same-second-
collision wart that motivated `ObjectVersionTag`'s `supersedes` chain.

---

## 10. Out of scope for MVP

- Multi-cosigner / threshold capabilities (`ACTORS.md` §14 governance).
- Snapshot-, artifact-, runtime-, build-scoped capabilities.
- Capability bundle type for federation transport.
- Time-delayed grant activation (`activates_at`).
- Conditional capabilities (data- or frontier-dependent constraints).
- Capability-derived UI affordances in the web client.

Each is a known follow-up; none change the v1 statement shape if added
later as new constraint variants or new scope variants. Adding a new
`CapabilityConstraint` variant or `CapabilityScope` variant is a v2 work
item under Phase 2 §12 if it cannot be expressed as a downstream layer.

---

## 11. Notes on related deferred work

### 11.A `ActorTrust` cross-actor supersedes

Decision B keeps trust strictly first-person. If revisited, the spec
change is a v2 of `ActorTrust` lifting the cross-actor restriction, gated
on capability evaluation per §6.2.

### 11.B `ObjectBranch v2`

Phase 2 §12 bumps `ObjectBranch` to add `supersedes`. Cross-actor edges
in v2 follow the same capability evaluation rules as
`ObjectVersionTag.supersedes` in §6.2. Designing `ObjectBranch v2`
together with this spec — rather than after — is what Phase 2 §12 calls
for.

### 11.C Schema migration

This spec adds two new statement kinds; it does not modify existing
canonical encodings. No `StatementId` re-derivation is required. The
`ObjectVersionTag` resolver gains a new code path (cross-actor edges
become honored when backed by capability), but the on-disk statement
bytes are unchanged.

### 11.D `ACTORS.md` §10–12 cleanup

After this spec settles, `ACTORS.md` §10–12 should shrink to a one-line
forward reference into `CAPABILITIES.md`. Left as a follow-up edit so
the seed prose stays available during review.

### 11.E Key-revocation audit UX

Per §7.1, grantor key compromise requires the operator to enumerate
grants signed by the revoked key and decide which to retroactively
revoke. The CLI / web client should surface this as part of the key-
revocation flow — e.g., `kairo actor revoke-key <key-id>` could list
grants signed by that key and prompt the operator to confirm or
retroactively revoke each. This is a UX layer over the existing
statement model and is deferred until the key-rotation/revocation work
in Phase 2 §10 lands.

### 11.F Indirect (transitive) trust evaluation

Decision B keeps `ActorTrust` strictly first-person. Indirect trust —
"A trusts X, X trusts Y, so A kinda trusts Y" — is nonetheless a useful
signal, especially as a tiebreaker when two equally-valid statements
disagree and local policy needs a soft preference. Indirect trust is a
*derived computation* over the existing first-person trust graph: it
does not require new statement types, new authority paths, or any
change to `CAPABILITIES.md`. It belongs in trust evaluation (probably
as an extension to `STATEMENTS.md` §6, or as its own short
`TRUST_EVALUATION.md` if the algorithm grows enough to deserve one).
Tracked here because the question naturally surfaces while reading
Decision B.

### 11.G Per-kind constraint variance within a triple — known trade-off

Decision G enforces single chain leaf per `(grantor, grantee, scope)`
triple, with `Capability::statement_kinds` carrying the complete
authorized kind set. A consequence: all kinds within one grant share
the grant's constraint set. If a grantor wants `[a, b]` to expire next
year and `[c]` to expire next week, the only options are:

- Two separate triples (different scopes — e.g. different objects), if
  applicable.
- Accept the strictest constraint over the union.
- Accept the weakest constraint over the union.

In the maintainer-delegation cases this spec targets, constraints
typically describe "the relationship" (this delegate, this period) and
apply uniformly — so this rarely bites. If it becomes a real pain
point, two future relaxations are available without breaking v1
canonical bytes:

- Relax Decision G to allow disjoint-kind same-triple grants
  (multi-leaf union per triple). Adds resolver complexity and
  complicates revocation semantics.
- Add a `CapabilityConstraint::PerKind { kind, constraint }` variant.
  Keeps the single-leaf model intact; constraints become per-kind
  overrides.

Both are Phase 3+ concerns; neither is needed for the cross-actor
`supersedes` MVP demonstration.

### 11.H Future scope lattice (broader-covers-narrower)

The MVP scope vocabulary (`Object`, `Actor`) is flat — variants are
disjoint targets, no nesting. When future scope variants land
(`Snapshot(O, S)`, `Artifact(O, A)`, etc.), the natural rule is
**broader-covers-narrower**: `Object(O)` automatically authorizes
covered kinds on any `Snapshot(O, ...)` or `Artifact(O, ...)`. Operator
intuition is "if I can do anything to O, I can do anything to its
parts."

This is a Phase 3+ decision, not v1 work — but worth flagging now so
that the eventual scope-vocabulary expansion considers the lattice
rule from the start rather than retrofitting it.

### 11.I Kind taxonomy / hierarchy

If statement kinds ever split or hierarchically refine (e.g. a
hypothetical `ObjectVersionTagRevocation` distinct from
`ObjectVersionTag`), we may want a "kind implies kind" rule so that
authority over a parent kind covers child kinds. Today every kind is
flat and atomic, so the question doesn't arise. Worth flagging for the
same reason as §11.H — the lattice should be designed alongside the
taxonomy split when it happens.

### 11.J Bundle import of capability statements

When a bundle containing `ActorCapabilityGrant` or
`ActorCapabilityRevocation` statements is imported, the importer
processes them like any other statement: fixity check, signature
verification, store under the per-grantor shard. Importing a grant
**does not** auto-trust the grantor, and capability validity is
recomputed locally via `evaluate_capability` against the local node's
view of root authority and trust. A grant that is semantically valid
on the producing node may evaluate as `GrantorLacksAuthority` locally
if the importer has not yet observed the grantor's own authority chain.
This is the standard "capability validity is semantic; local trust is
policy" rule applied to federated bundle import.

---

End of CAPABILITIES.md
