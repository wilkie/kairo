//! Per-object materialized index of `ObjectBranch` tips.
//!
//! Each file at `<root>/branches/<XX>/<YY>/<object-id>.json` holds the current
//! winning branch tips for that object — one entry per `(actor, name)` —
//! materialized from the underlying `ObjectBranch` statements stored in
//! `statements/`. Resolution reads one file per lookup; supersession on
//! write keeps only the greatest `(created_at, statement_id)` tip per key.
//!
//! The index is a strict materialization of the underlying statements: if
//! it is lost or corrupt, it can be rebuilt by scanning all `ObjectBranch`
//! statements about an object. The MVP does not implement rebuild; it
//! relies on always going through `put_object_branch`.
//!
//! Format (JSON, one file per object):
//!
//! ```json
//! {
//!   "<actor-id>": {
//!     "<branch-name>": {
//!       "statement_id": "<statement-id>",
//!       "created_at": "<RFC 3339>"
//!     }
//!   }
//! }
//! ```
//!
//! Multi-process safety is not yet enforced; concurrent writers can race
//! on read-modify-write. File locks land alongside the rest of the
//! multi-process MVP work.

use std::collections::BTreeMap;

use kairo_core::{ActorId, ObjectId, StatementId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};

/// On-disk representation of one branch tip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BranchTipEntry {
    pub statement_id: String,
    pub created_at: String,
}

/// On-disk representation of all tips for a single object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct BranchIndexFile {
    /// `actor-id -> (branch-name -> tip)`.
    pub by_actor: BTreeMap<String, BTreeMap<String, BranchTipEntry>>,
}

/// Public summary of a branch tip — what callers see when listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchTip {
    pub actor: ActorId,
    pub object: ObjectId,
    pub name: String,
    pub statement_id: StatementId,
    pub created_at: Timestamp,
}

impl BranchIndexFile {
    /// Apply a tip to the index. Returns `true` if the index was updated
    /// (i.e. the new tip strictly supersedes whatever was there before).
    /// Same-key ties resolve on `statement_id`; equal entries are no-ops.
    pub(crate) fn upsert(
        &mut self,
        actor: &ActorId,
        name: &str,
        statement_id: &StatementId,
        created_at: Timestamp,
    ) -> bool {
        let by_name = self.by_actor.entry(actor.to_string()).or_default();

        let new_entry = BranchTipEntry {
            statement_id: statement_id.to_string(),
            created_at: created_at.to_string(),
        };

        match by_name.get(name) {
            Some(existing) if !is_strictly_greater(statement_id, created_at, existing) => false,
            _ => {
                by_name.insert(name.to_owned(), new_entry);
                true
            }
        }
    }

    pub(crate) fn lookup(&self, actor: &ActorId, name: &str) -> Option<&BranchTipEntry> {
        self.by_actor
            .get(actor.as_str())
            .and_then(|by_name| by_name.get(name))
    }

    pub(crate) fn into_tips(self, object: &ObjectId) -> Result<Vec<BranchTip>, StoreError> {
        let mut tips = Vec::new();
        for (actor_str, by_name) in self.by_actor {
            let actor = ActorId::new(actor_str.clone()).map_err(|error| StoreError::Corrupt {
                id: actor_str,
                reason: CorruptReason::Parse(format!("invalid actor id in branch index: {error}")),
            })?;
            for (name, entry) in by_name {
                let statement_id =
                    StatementId::new(entry.statement_id.clone()).map_err(|error| {
                        StoreError::Corrupt {
                            id: entry.statement_id.clone(),
                            reason: CorruptReason::Parse(format!(
                                "invalid statement id in branch index: {error}"
                            )),
                        }
                    })?;
                let created_at: Timestamp =
                    entry
                        .created_at
                        .parse()
                        .map_err(|error| StoreError::Corrupt {
                            id: entry.statement_id,
                            reason: CorruptReason::Parse(format!(
                                "invalid created_at in branch index: {error}"
                            )),
                        })?;
                tips.push(BranchTip {
                    actor: actor.clone(),
                    object: object.clone(),
                    name,
                    statement_id,
                    created_at,
                });
            }
        }
        Ok(tips)
    }
}

/// `(new_created_at, new_statement_id) > (existing.created_at, existing.statement_id)`
/// in lexicographic order. Equal values are NOT strictly greater (the
/// existing entry wins, so a no-op write does not flap the index).
fn is_strictly_greater(
    new_statement_id: &StatementId,
    new_created_at: Timestamp,
    existing: &BranchTipEntry,
) -> bool {
    let existing_created_at = existing.created_at.parse::<Timestamp>();
    match existing_created_at {
        Ok(existing_created_at) => {
            if new_created_at > existing_created_at {
                return true;
            }
            if new_created_at < existing_created_at {
                return false;
            }
            new_statement_id.as_str() > existing.statement_id.as_str()
        }
        // Existing entry is corrupt — treat the new write as winning so we
        // self-heal on the next put. A subsequent read will surface the
        // corruption via the index parser.
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor() -> ActorId {
        ActorId::from_sha256_digest([1; 32])
    }

    fn object() -> ObjectId {
        ObjectId::from_sha256_digest([2; 32])
    }

    fn statement_id_one() -> StatementId {
        StatementId::from_sha256_digest([0xAA; 32])
    }

    fn statement_id_two() -> StatementId {
        StatementId::from_sha256_digest([0xBB; 32])
    }

    fn timestamp(seconds: i64) -> Timestamp {
        Timestamp::from_seconds(seconds)
    }

    #[test]
    fn empty_index_returns_no_tip() {
        let index = BranchIndexFile::default();
        assert!(index.lookup(&actor(), "head").is_none());
    }

    #[test]
    fn upsert_returns_true_for_new_entry() {
        let mut index = BranchIndexFile::default();
        let updated = index.upsert(&actor(), "head", &statement_id_one(), timestamp(100));
        assert!(updated);
        assert!(index.lookup(&actor(), "head").is_some());
    }

    #[test]
    fn later_created_at_supersedes() {
        let mut index = BranchIndexFile::default();
        index.upsert(&actor(), "head", &statement_id_one(), timestamp(100));
        let updated = index.upsert(&actor(), "head", &statement_id_two(), timestamp(200));
        assert!(updated);
        assert_eq!(
            index
                .lookup(&actor(), "head")
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_two().as_str())
        );
    }

    #[test]
    fn earlier_created_at_does_not_supersede() {
        let mut index = BranchIndexFile::default();
        index.upsert(&actor(), "head", &statement_id_one(), timestamp(200));
        let updated = index.upsert(&actor(), "head", &statement_id_two(), timestamp(100));
        assert!(!updated);
        assert_eq!(
            index
                .lookup(&actor(), "head")
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
    }

    #[test]
    fn equal_created_at_resolves_on_statement_id() {
        // statement_id_two() > statement_id_one() lexicographically.
        let mut index = BranchIndexFile::default();
        index.upsert(&actor(), "head", &statement_id_one(), timestamp(100));
        let updated = index.upsert(&actor(), "head", &statement_id_two(), timestamp(100));
        assert!(updated);
        assert_eq!(
            index
                .lookup(&actor(), "head")
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_two().as_str())
        );
    }

    #[test]
    fn equal_entry_is_noop() {
        let mut index = BranchIndexFile::default();
        index.upsert(&actor(), "head", &statement_id_one(), timestamp(100));
        let updated = index.upsert(&actor(), "head", &statement_id_one(), timestamp(100));
        assert!(!updated);
    }

    #[test]
    fn separate_branch_names_do_not_supersede_each_other() {
        let mut index = BranchIndexFile::default();
        index.upsert(&actor(), "head", &statement_id_one(), timestamp(100));
        let updated = index.upsert(&actor(), "release", &statement_id_two(), timestamp(50));
        assert!(updated);
        assert!(index.lookup(&actor(), "head").is_some());
        assert!(index.lookup(&actor(), "release").is_some());
    }

    #[test]
    fn into_tips_lists_all_branches() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = BranchIndexFile::default();
        index.upsert(&actor(), "head", &statement_id_one(), timestamp(100));
        index.upsert(&actor(), "release", &statement_id_two(), timestamp(200));
        let tips = index.into_tips(&object())?;
        assert_eq!(tips.len(), 2);
        Ok(())
    }
}
