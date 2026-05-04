# AGENTS.md — kairo-statement

## Adding a new statement body type

Touch every line below — partial implementations break determinism or
break the JSON round-trip and tests will catch it late, not early.

1. **Define the body** in `src/lib.rs`:
   - Public struct with private fields and accessors.
   - `impl StatementBody` with a unique `TYPE` constant and
     `VERSION = 1`.
   - `impl CanonicalEncode` matching the spec in
     `schemas/canonical/<type>-v1.md` byte-for-byte. Use
     `encode_str`, `encode_option`, `encode_list` from
     `kairo_core::canonical`; never `serde_json::to_vec` for
     canonical bytes.
   - Validation in a `new` constructor for shape rules that the
     canonical encoding cannot enforce on its own (e.g. "withdraw
     requires `supersedes`").

2. **Add a JSON DTO** in `src/json.rs`:
   - `<Type>StatementJson` and `<Type>BodyJson` structs with
     `to_statement` / `from_statement` and `to_body` / `from_body`.
   - Wire any new shape errors through the `StatementJsonError` enum
     (Display + Error impls).

3. **Write the tests** in `src/lib.rs` and `src/json.rs` `tests`
   modules. The minimum bar:
   - `same_<type>_body_produces_same_statement_id` (canonical
     determinism).
   - `<some_field>_changes_statement_id` (every signed field
     contributes).
   - `<type>_signature_does_not_change_statement_id` (envelope
     identity is over the body, not the signature).
   - `verifies_<type>_ed25519_signature` and
     `rejects_<type>_signature_after_body_change` (sign / tamper /
     re-verify round-trip).
   - `parses_<type>_json` (positive), `rejects_<type>_with_*` for
     each shape error, `json_key_order_does_not_affect_<type>_statement_id`
     (canonical bytes are independent of JSON key order),
     `<type>_round_trips_through_from_statement`.

4. **Author both schema docs** before/alongside the code:
   `schemas/canonical/<type>-v1.md` (domain separator, field order,
   pseudocode) and `schemas/json/<type>-v1.schema.json`. The
   canonical doc is authoritative — the JSON schema is interchange
   only.

5. **Update `STATEMENTS.md` §4** with the new subsection and
   `GLOSSARY.md` if the type introduces new vocabulary.

## Canonical-bytes invariants (do not break)

- Field order in `encode_canonical` must match the canonical schema
  doc. Reordering changes every existing `StatementId`.
- Optional fields use `encode_option` (`0x00` for none, `0x01 || T`
  for some). Never write the contents directly conditional on
  `Option::is_some` — that loses the discriminator byte.
- `Timestamp` is `i64` epoch seconds in canonical bytes and RFC 3339
  UTC seconds in JSON; both must round-trip.
- The envelope domain separator is `kairo.statement.v1`. The body
  domain is type-specific (`kairo.statement.<type>.v1`) — see each
  schema doc.

## When in doubt

- The Rust impl follows the canonical schema, not the other way
  around. If they disagree, the schema doc wins and the impl is the
  bug.
- Cross-actor authority claims (one actor's statement superseding
  another's) are governed by the capability model in
  `specs/CAPABILITIES.md`. The `evaluate_capability` resolver in
  `src/verify.rs` is the authority oracle; consult it (don't reinvent
  it) when a new statement type needs cross-actor supersedes
  semantics. Per Decision B in `specs/CAPABILITIES.md` §9,
  `ActorTrust` is the one statement that explicitly stays
  first-person — cross-actor supersedes is invalid for it even with
  capabilities.
