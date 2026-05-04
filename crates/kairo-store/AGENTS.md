# AGENTS.md — kairo-store

## Adding a new statement type to the store

For every new `StatementBody` defined in `kairo-statement`, the store
needs:

1. **Trait method** on `StatementStore` (in `src/lib.rs`): `put_<type>`
   and `get_<type>` taking and returning `SignedStatement<<Type>Body>`.
   Do **not** collapse into a generic `put<T>` — type-specific methods
   keep error reporting precise and let materialized indices live
   alongside.

2. **`FilesystemStore` impl** of those methods. Pattern:
   - Derive id from the signed statement, serialize via the JSON
     DTO, atomic-write under `STATEMENTS_DIR`.
   - On read, parse → `to_statement()` → re-derive id → return
     `Corrupt::HashMismatch` on disagreement (this is the fixity
     check; it is never optional).

3. **Materialized index** if the statement type has a per-key
   resolver concept (latest-wins, chain leaf, etc.):
   - New module `src/<type>.rs` defining `<Type>IndexFile` (often a
     `serde(transparent)` BTreeMap, optionally with sibling sub-maps
     when the type carries multiple lookup axes — see
     `src/capabilities.rs`'s `grants` + `revocations` pair) and
     `<Type>Head` (public summary).
   - Write path: `put_<type>` calls a private `upsert_<type>_index`
     helper that read-modify-writes the per-shard JSON.
   - Read path: a new `<Type>Resolver` trait + impl on
     `FilesystemStore`.
   - **Choose the shard key deliberately.** Per-object indices
     (branches, version_tags) shard on `object_id`; per-truster
     indices (trust) shard on the *trusted actor* so federation
     aggregation queries are O(1); per-grantor capability indices
     (`actor_capability`) shard on the signer because the grantor
     owns revocation authority. Document the choice in the module's
     top-level `//!` doc.
   - **Multiple indices per statement type** are sometimes required:
     `ActorCapabilityGrant` writes both the per-grantor index *and*
     the per-object reverse index (`actor_capability_by_object`)
     atomically inside `put_actor_capability_grant`, because the
     §6.1 capability evaluator's hot query keys on object, not
     grantor. Pattern: one `put_*` method, multiple
     `upsert_*_index` helpers; document each index in its own
     module-level `//!` doc.
   - Honor chain precedence (supersedes-leaf is authoritative; fork
     tiebreak only on `(created_at, statement_id)`) — see
     `src/tags.rs`, `src/trust.rs`, and `src/capabilities.rs` for
     the pattern.
   - **Cross-actor `supersedes`** semantics live above the index, not
     in it. The per-actor / same-actor chain head is the index's
     job; honoring cross-actor edges requires an authority oracle
     (`evaluate_capability`). See
     `FilesystemStore::walk_authorized_tag_chain` for the pattern:
     compute same-actor leaf in the index module, then walk forward
     through authorized cross-actor sup edges in `lib.rs` where the
     resolver has `&self`.

4. **Tests** in `src/lib.rs`'s `tests` module covering at minimum:
   round-trip; latest/head resolution; chain precedence overriding
   timestamp tiebreak; missing record returns `None` (not error);
   sharded path layout matches the documented scheme; tampered file
   returns `Corrupt::HashMismatch`.

## Fixity is never optional

Every `get_*` method must re-derive the record's id from its parsed
body and compare to the requested id. If they disagree the call
returns `StoreError::Corrupt { reason: HashMismatch { expected,
actual } }`. Silent recovery would defeat the entire point of
content-addressing. See `specs/GLOSSARY.md` "Fixity."

## Materialized indices are derived state

If an index file is lost or wrong, it is correct-by-construction to
rebuild from `statements/`. The MVP does not implement rebuild — the
write paths (`put_*`) are the only thing keeping indices in sync.
Don't add code that bypasses `put_*` and writes directly to
`STATEMENTS_DIR`; the indices will silently drift.

## Sharding rules

`shard::shard_path` takes `<root>/<type_dir>/<XX>/<YY>/<id><suffix>`
where `<XX>` and `<YY>` are positions 3-4 and 5-6 of the id payload
(after the `zQm` prefix). Don't invent a different scheme for new
record types — uniformity is the point.
