//! Per-actor materialized index of every object that actor created.
//!
//! Each file at `<root>/objects_by_actor/<XX>/<YY>/<actor-id>.json`
//! records every `ObjectGenesis` whose body's `created_by` names
//! that actor, materialized from the genesis statements stored
//! under `objects/`. The index is maintained eagerly: every
//! `put_object_genesis` appends an entry under the creator's file.
//!
//! ## Why a separate index from `statements_by_actor`
//!
//! The sibling `statements_by_actor` index keys off the
//! envelope-level `actor` field that every signed statement type
//! carries. `ObjectGenesis` is the one shape that *doesn't* carry
//! an envelope `actor`: it's identity-deriving (the body's bytes
//! derive the `ObjectId`), and the creator is in the body's
//! `created_by` field. Forcing it into `statements_by_actor` would
//! mean a divergent code path for one statement kind; spinning a
//! second index keeps each one shape-uniform.
//!
//! The two indices answer different questions:
//!
//! - `statements_by_actor` — "what has this actor signed?" (audit
//!   timeline of every envelope whose signer is this actor).
//! - `objects_by_actor` — "what has this actor created?" (audit
//!   list of every object whose `ObjectGenesis.created_by` is
//!   this actor).
//!
//! The inspector renders both on the actor page; together they
//! answer "what is this actor responsible for in the store?".
//!
//! ## On-disk shape
//!
//! Entries are a flat `Vec` (no chain semantics — `ObjectGenesis`
//! is immutable once written). Resolution sorts by `(created_at,
//! object_id)` ascending so the inspector renders chronologically
//! with deterministic ties.
//!
//! `object_kind` is denormalized into the entry so the inspector
//! can render the kind tag without a per-row genesis fetch. The
//! kind is fixed at genesis time, so denormalization is safe.
//!
//! The index is a strict materialization of the underlying genesis
//! statements: if it is lost or corrupt, it can be rebuilt by
//! scanning `objects/` and grouping by `created_by`. The store's
//! `rebuild_indexes()` (surfaced as `kairo store rebuild-indexes`)
//! does exactly this for every materialized index in one pass,
//! including this one.
//!
//! Format (JSON, one file per actor — the file is just an array):
//!
//! ```json
//! [
//!   {
//!     "object_id": "<object-id>",
//!     "object_kind": "kairo/object",
//!     "created_at": "<RFC 3339>"
//!   },
//!   ...
//! ]
//! ```
//!
//! Multi-process safety is enforced at the file-write layer via
//! `lock::with_index_lock` on the per-actor sidecar.

use kairo_core::{ActorId, ObjectId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};

/// On-disk representation of one object entry under an actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ObjectByActorEntry {
    pub object_id: String,
    /// The `ObjectKind::as_str()` discriminator (e.g.
    /// `"kairo/object"`). Denormalized from the genesis body so
    /// the inspector can render the kind without a follow-up
    /// fetch.
    pub object_kind: String,
    pub created_at: String,
}

/// On-disk representation of all objects created by a single actor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct ObjectByActorIndexFile {
    pub entries: Vec<ObjectByActorEntry>,
}

/// Public summary of one object under an actor — what callers see
/// when listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectByActor {
    pub actor: ActorId,
    pub object_id: ObjectId,
    pub object_kind: String,
    pub created_at: Timestamp,
}

impl ObjectByActorIndexFile {
    /// Append an entry. Returns `true` if the entry was actually added
    /// (i.e. its `object_id` was not already present); a duplicate
    /// `put` is a no-op so re-indexing during recovery / repeated
    /// imports is idempotent.
    pub(crate) fn upsert(
        &mut self,
        object_id: &ObjectId,
        object_kind: &str,
        created_at: Timestamp,
    ) -> bool {
        let new_id = object_id.to_string();
        if self.entries.iter().any(|e| e.object_id == new_id) {
            return false;
        }
        self.entries.push(ObjectByActorEntry {
            object_id: new_id,
            object_kind: object_kind.to_owned(),
            created_at: created_at.to_string(),
        });
        true
    }

    /// Decode every entry as a public `ObjectByActor`. Sorts the
    /// result by `(created_at, object_id)` ascending — chronological
    /// audit order, with deterministic ties.
    pub(crate) fn into_summaries(self, actor: &ActorId) -> Result<Vec<ObjectByActor>, StoreError> {
        let mut out = Vec::with_capacity(self.entries.len());
        for entry in self.entries {
            let object_id =
                ObjectId::new(entry.object_id.clone()).map_err(|error| StoreError::Corrupt {
                    id: entry.object_id.clone(),
                    reason: CorruptReason::Parse(format!(
                        "invalid object id in objects_by_actor index: {error}"
                    )),
                })?;
            let created_at: Timestamp =
                entry
                    .created_at
                    .parse()
                    .map_err(|error| StoreError::Corrupt {
                        id: entry.object_id.clone(),
                        reason: CorruptReason::Parse(format!(
                            "invalid created_at in objects_by_actor index: {error}"
                        )),
                    })?;
            out.push(ObjectByActor {
                actor: actor.clone(),
                object_id,
                object_kind: entry.object_kind,
                created_at,
            });
        }
        out.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.object_id.cmp(&b.object_id))
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

    fn object_id_one() -> ObjectId {
        ObjectId::from_sha256_digest([0xAA; 32])
    }

    fn object_id_two() -> ObjectId {
        ObjectId::from_sha256_digest([0xBB; 32])
    }

    fn object_id_three() -> ObjectId {
        ObjectId::from_sha256_digest([0xCC; 32])
    }

    fn timestamp(seconds: i64) -> Timestamp {
        Timestamp::from_seconds(seconds)
    }

    #[test]
    fn empty_index_summarizes_to_empty() -> Result<(), Box<dyn std::error::Error>> {
        let index = ObjectByActorIndexFile::default();
        let summaries = index.into_summaries(&actor())?;
        assert!(summaries.is_empty());
        Ok(())
    }

    #[test]
    fn single_entry_round_trips() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = ObjectByActorIndexFile::default();
        let added = index.upsert(&object_id_one(), "kairo/object", timestamp(100));
        assert!(added);
        let summaries = index.into_summaries(&actor())?;
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].object_id, object_id_one());
        assert_eq!(summaries[0].object_kind, "kairo/object");
        assert_eq!(summaries[0].created_at, timestamp(100));
        Ok(())
    }

    #[test]
    fn duplicate_put_is_noop() {
        let mut index = ObjectByActorIndexFile::default();
        index.upsert(&object_id_one(), "kairo/object", timestamp(100));
        let added = index.upsert(&object_id_one(), "kairo/object", timestamp(100));
        assert!(!added);
        assert_eq!(index.entries.len(), 1);
    }

    #[test]
    fn into_summaries_sorts_by_created_at_then_object_id() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut index = ObjectByActorIndexFile::default();
        // Insert out of chronological order, with a same-timestamp pair.
        index.upsert(&object_id_two(), "kairo/object", timestamp(200));
        index.upsert(&object_id_one(), "kairo/object", timestamp(100));
        index.upsert(&object_id_three(), "kairo/software", timestamp(200));

        let summaries = index.into_summaries(&actor())?;
        assert_eq!(summaries.len(), 3);
        assert_eq!(summaries[0].object_id, object_id_one());
        // Same created_at — lex-smaller object_id wins.
        assert_eq!(summaries[1].object_id, object_id_two());
        assert_eq!(summaries[2].object_id, object_id_three());
        Ok(())
    }
}
