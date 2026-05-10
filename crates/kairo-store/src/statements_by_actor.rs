//! Per-actor materialized index of every signed statement that actor authored.
//!
//! Each file at `<root>/statements_by_actor/<XX>/<YY>/<actor-id>.json`
//! records every signed statement whose envelope is signed by that actor,
//! materialized from the underlying signed statements stored in
//! `statements/`. The index is maintained eagerly: every `put_*` for a
//! signed statement (whether `SignedStatement<_>` or `MultiSignedStatement<_>`)
//! appends an entry under the signing actor's file.
//!
//! ## Why a flat list (not the chain-precedence shape)
//!
//! Trust, branches, version-tags, and capabilities all carry head
//! semantics — there's a current "winner" computed from a `supersedes`
//! chain — so their indices store entries grouped by `(subject, name)`
//! and resolve a head on read. Per-actor statement listing is different:
//! we want **every** statement an actor signed (audit timeline), not
//! the head of any particular chain. So entries are a flat `Vec` and
//! resolution is "list all, sorted by `(created_at, statement_id)`".
//!
//! Multi-signer envelopes (the actor-key family that uses
//! `MultiSignedStatement`) still record exactly one entry under the
//! envelope's authoring `actor` — the same actor field every other
//! statement type uses. The cosigner attestations don't get their own
//! per-actor entries; this index is "statements **by** actor", not
//! "statements **involving** actor".
//!
//! `ObjectGenesis` is intentionally **not** indexed here. Its envelope
//! has a `created_by` field rather than the `actor` envelope field
//! every other statement type uses, and the `objects/` directory
//! already provides the per-creator scan path. Including it would mean
//! a divergent code path for a single statement kind; the audit page
//! can fold genesis in client-side via `actors/<id>/objects` when that
//! becomes important.
//!
//! The index is a strict materialization of the underlying statements:
//! if it is lost or corrupt, it can be rebuilt by scanning all signed
//! statements and grouping by `actor`. The store's
//! `rebuild_indexes()` (surfaced as `kairo store rebuild-indexes`)
//! does exactly this for every materialized index in one pass,
//! including this one.
//!
//! Format (JSON, one file per actor — the file is just an array):
//!
//! ```json
//! [
//!   {
//!     "statement_id": "<statement-id>",
//!     "kind": "ObjectBranch",
//!     "created_at": "<RFC 3339>"
//!   },
//!   ...
//! ]
//! ```
//!
//! Multi-process safety is enforced at the file-write layer via
//! `lock::with_index_lock` on the per-actor sidecar.

use kairo_core::{ActorId, StatementId, Timestamp};
use kairo_statement::StatementKind;
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};

/// On-disk representation of one statement entry under an actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StatementByActorEntry {
    pub statement_id: String,
    /// The `StatementKind::as_str()` discriminator. Stored as the
    /// canonical string so the index is stable across enum re-orderings
    /// in `kairo-statement`.
    pub kind: String,
    pub created_at: String,
}

/// On-disk representation of all statements signed by a single actor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct StatementByActorIndexFile {
    pub entries: Vec<StatementByActorEntry>,
}

/// Public summary of one statement under an actor — what callers see
/// when listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementByActor {
    pub actor: ActorId,
    pub statement_id: StatementId,
    pub kind: StatementKind,
    pub created_at: Timestamp,
}

impl StatementByActorIndexFile {
    /// Append an entry. Returns `true` if the entry was actually added
    /// (i.e. its `statement_id` was not already present); a duplicate
    /// `put` is a no-op so re-indexing during recovery / repeated
    /// imports is idempotent.
    pub(crate) fn upsert(
        &mut self,
        statement_id: &StatementId,
        kind: StatementKind,
        created_at: Timestamp,
    ) -> bool {
        let new_id = statement_id.to_string();
        if self.entries.iter().any(|e| e.statement_id == new_id) {
            return false;
        }
        self.entries.push(StatementByActorEntry {
            statement_id: new_id,
            kind: kind.as_str().to_owned(),
            created_at: created_at.to_string(),
        });
        true
    }

    /// Decode every entry as a public `StatementByActor`. Sorts the
    /// result by `(created_at, statement_id)` ascending — chronological
    /// audit order, with deterministic ties.
    pub(crate) fn into_summaries(
        self,
        actor: &ActorId,
    ) -> Result<Vec<StatementByActor>, StoreError> {
        let mut out = Vec::with_capacity(self.entries.len());
        for entry in self.entries {
            let statement_id = StatementId::new(entry.statement_id.clone()).map_err(|error| {
                StoreError::Corrupt {
                    id: entry.statement_id.clone(),
                    reason: CorruptReason::Parse(format!(
                        "invalid statement id in statements_by_actor index: {error}"
                    )),
                }
            })?;
            let kind = StatementKind::parse(&entry.kind).map_err(|error| StoreError::Corrupt {
                id: entry.statement_id.clone(),
                reason: CorruptReason::Parse(format!(
                    "invalid statement kind in statements_by_actor index: {error}"
                )),
            })?;
            let created_at: Timestamp =
                entry
                    .created_at
                    .parse()
                    .map_err(|error| StoreError::Corrupt {
                        id: entry.statement_id.clone(),
                        reason: CorruptReason::Parse(format!(
                            "invalid created_at in statements_by_actor index: {error}"
                        )),
                    })?;
            out.push(StatementByActor {
                actor: actor.clone(),
                statement_id,
                kind,
                created_at,
            });
        }
        out.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.statement_id.cmp(&b.statement_id))
        });
        Ok(out)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn actor() -> ActorId {
        ActorId::from_sha256_digest([1; 32])
    }

    fn statement_id_one() -> StatementId {
        StatementId::from_sha256_digest([0xAA; 32])
    }

    fn statement_id_two() -> StatementId {
        StatementId::from_sha256_digest([0xBB; 32])
    }

    fn statement_id_three() -> StatementId {
        StatementId::from_sha256_digest([0xCC; 32])
    }

    fn timestamp(seconds: i64) -> Timestamp {
        Timestamp::from_seconds(seconds)
    }

    #[test]
    fn empty_index_summarizes_to_empty() -> Result<(), Box<dyn std::error::Error>> {
        let index = StatementByActorIndexFile::default();
        let summaries = index.into_summaries(&actor())?;
        assert!(summaries.is_empty());
        Ok(())
    }

    #[test]
    fn single_entry_round_trips() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = StatementByActorIndexFile::default();
        let added = index.upsert(
            &statement_id_one(),
            StatementKind::ObjectBranch,
            timestamp(100),
        );
        assert!(added);
        let summaries = index.into_summaries(&actor())?;
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].statement_id, statement_id_one());
        assert_eq!(summaries[0].kind, StatementKind::ObjectBranch);
        assert_eq!(summaries[0].created_at, timestamp(100));
        Ok(())
    }

    #[test]
    fn duplicate_put_is_noop() {
        let mut index = StatementByActorIndexFile::default();
        index.upsert(
            &statement_id_one(),
            StatementKind::ObjectBranch,
            timestamp(100),
        );
        let added = index.upsert(
            &statement_id_one(),
            StatementKind::ObjectBranch,
            timestamp(100),
        );
        assert!(!added);
        assert_eq!(index.entries.len(), 1);
    }

    #[test]
    fn into_summaries_sorts_by_created_at_then_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut index = StatementByActorIndexFile::default();
        // Insert out of chronological order, with a same-timestamp pair.
        index.upsert(
            &statement_id_two(),
            StatementKind::ActorTrust,
            timestamp(200),
        );
        index.upsert(
            &statement_id_one(),
            StatementKind::ObjectBranch,
            timestamp(100),
        );
        index.upsert(
            &statement_id_three(),
            StatementKind::ActorCapabilityGrant,
            timestamp(200),
        );

        let summaries = index.into_summaries(&actor())?;
        assert_eq!(summaries.len(), 3);
        assert_eq!(summaries[0].statement_id, statement_id_one());
        // Same created_at — lex-smaller statement_id wins.
        assert_eq!(summaries[1].statement_id, statement_id_two());
        assert_eq!(summaries[2].statement_id, statement_id_three());
        Ok(())
    }

    #[test]
    fn corrupt_kind_surfaces_as_corrupt_error() {
        let mut index = StatementByActorIndexFile::default();
        index.entries.push(StatementByActorEntry {
            statement_id: statement_id_one().to_string(),
            kind: "NotARealKind".to_owned(),
            created_at: timestamp(100).to_string(),
        });
        let err = index.into_summaries(&actor()).expect_err("should fail");
        assert!(matches!(err, StoreError::Corrupt { .. }));
    }
}
