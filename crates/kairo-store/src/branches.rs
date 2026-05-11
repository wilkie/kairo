//! Per-object materialized index of `ObjectBranch` advances.
//!
//! Each file at `<root>/branches/<XX>/<YY>/<object-id>.json` records every
//! known `ObjectBranch` for that object — one entry per
//! `(actor, name, statement_id)` — materialized from the underlying signed
//! statements stored in `statements/`. Resolution computes the current
//! head per `(actor, name)` from these entries on read using the shared
//! supersedes-chain rule in [`crate::chain::chain_head`].
//!
//! Cross-actor `supersedes` references are recorded on each entry but
//! the chain-head computation in this module only walks **same-actor**
//! edges. The authority-aware forward walk that honors cross-actor
//! supersedes when the successor's signer holds an `ObjectBranch`
//! capability on the object lives in
//! `FilesystemStore::walk_authorized_branch_chain` and is invoked by
//! the resolver path on top of this index. See
//! `specs/CAPABILITIES.md` §6.2.
//!
//! The index is a strict materialization of the underlying statements:
//! if it is lost or corrupt, it can be rebuilt by scanning all
//! `ObjectBranch` statements about an object. The store's
//! `rebuild_indexes()` (surfaced as `kairo store rebuild-indexes`)
//! does exactly this for every materialized index in one pass,
//! including this one.
//!
//! Format (JSON, one file per object):
//!
//! ```json
//! {
//!   "<actor-id>": {
//!     "<branch-name>": [
//!       {
//!         "statement_id": "<statement-id>",
//!         "created_at": "<RFC 3339>",
//!         "supersedes": "<statement-id>" | null
//!       },
//!       ...
//!     ]
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

use crate::chain::{chain_head, ChainEntry};
#[cfg(test)]
use crate::error::{CorruptReason, StoreError};

/// On-disk representation of one branch advance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BranchEntry {
    pub statement_id: String,
    pub created_at: String,
    pub supersedes: Option<String>,
}

impl ChainEntry for BranchEntry {
    fn statement_id(&self) -> &str {
        &self.statement_id
    }

    fn supersedes(&self) -> Option<&str> {
        self.supersedes.as_deref()
    }

    fn created_at(&self) -> &str {
        &self.created_at
    }
}

/// On-disk representation of all branch advances for a single object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct BranchIndexFile {
    /// `actor-id -> (branch-name -> entries[])`. Entries within a key
    /// are stored in insertion order; resolution computes the head
    /// from the chain on read.
    pub by_actor: BTreeMap<String, BTreeMap<String, Vec<BranchEntry>>>,
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
    /// Append an entry. Returns `true` if the entry was actually added
    /// (i.e. its `statement_id` was not already present); a duplicate
    /// `put` is a no-op.
    pub(crate) fn upsert(
        &mut self,
        actor: &ActorId,
        name: &str,
        statement_id: &StatementId,
        created_at: Timestamp,
        supersedes: Option<&StatementId>,
    ) -> bool {
        let by_name = self.by_actor.entry(actor.to_string()).or_default();
        let entries = by_name.entry(name.to_owned()).or_default();
        let new_id = statement_id.to_string();
        if entries.iter().any(|e| e.statement_id == new_id) {
            return false;
        }
        entries.push(BranchEntry {
            statement_id: new_id,
            created_at: created_at.to_string(),
            supersedes: supersedes.map(|s| s.to_string()),
        });
        true
    }

    /// Resolve the same-actor chain head for `(actor, name)`. The head
    /// is the supersedes-chain leaf among `actor`'s own entries: an
    /// entry whose `statement_id` does not appear in any same-actor
    /// sibling entry's `supersedes` field. If the chain has multiple
    /// leaves (a fork), pick the one with the greatest
    /// `(created_at, statement_id)`.
    ///
    /// Cross-actor `supersedes` references are not consulted by this
    /// method — that decision needs an authority check
    /// (`evaluate_capability`) which lives outside the per-object
    /// index. See `FilesystemStore::walk_authorized_branch_chain`.
    pub(crate) fn lookup_head(&self, actor: &ActorId, name: &str) -> Option<&BranchEntry> {
        let entries = self
            .by_actor
            .get(actor.as_str())
            .and_then(|by_name| by_name.get(name))?;
        chain_head(entries)
    }

    /// All entries (across actors) for a given branch `name`. Returned
    /// pairs are `(actor_id_str, entry)` — used by the cross-actor
    /// walker to find candidate successors that name the current
    /// chain leaf via `supersedes`.
    pub(crate) fn entries_for_name(&self, name: &str) -> Vec<(&str, &BranchEntry)> {
        let mut all = Vec::new();
        for (actor_str, by_name) in &self.by_actor {
            if let Some(entries) = by_name.get(name) {
                for entry in entries {
                    all.push((actor_str.as_str(), entry));
                }
            }
        }
        all
    }

    /// Resolve every `(actor, name)` chain head known about `object`,
    /// considering only same-actor `supersedes` edges. Cross-actor
    /// authority-aware enumeration is the
    /// `FilesystemStore::list_branches` path; this method is the raw
    /// same-actor projection used directly by tests.
    #[cfg(test)]
    pub(crate) fn into_tips(self, object: &ObjectId) -> Result<Vec<BranchTip>, StoreError> {
        let mut tips = Vec::new();
        for (actor_str, by_name) in &self.by_actor {
            let actor = ActorId::new(actor_str.clone()).map_err(|error| StoreError::Corrupt {
                id: actor_str.clone(),
                reason: CorruptReason::Parse(format!("invalid actor id in branch index: {error}")),
            })?;
            for (name, entries) in by_name {
                let Some(head_entry) = chain_head(entries) else {
                    continue;
                };
                let statement_id =
                    StatementId::new(head_entry.statement_id.clone()).map_err(|error| {
                        StoreError::Corrupt {
                            id: head_entry.statement_id.clone(),
                            reason: CorruptReason::Parse(format!(
                                "invalid statement id in branch index: {error}"
                            )),
                        }
                    })?;
                let created_at: Timestamp =
                    head_entry
                        .created_at
                        .parse()
                        .map_err(|error| StoreError::Corrupt {
                            id: head_entry.statement_id.clone(),
                            reason: CorruptReason::Parse(format!(
                                "invalid created_at in branch index: {error}"
                            )),
                        })?;
                tips.push(BranchTip {
                    actor: actor.clone(),
                    object: object.clone(),
                    name: name.clone(),
                    statement_id,
                    created_at,
                });
            }
        }
        Ok(tips)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
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

    // The chain-head algorithm itself (empty / single-leaf /
    // supersedes-leaf / fork tiebreak) is exercised in
    // `crate::chain::tests`. The tests below cover concerns
    // specific to this index's on-disk shape: upsert idempotency,
    // per-key isolation in the nested map, and the JSON-to-public-
    // summary decoder.

    #[test]
    fn duplicate_put_is_noop() {
        let mut index = BranchIndexFile::default();
        index.upsert(&actor(), "head", &statement_id_one(), timestamp(100), None);
        let added = index.upsert(&actor(), "head", &statement_id_one(), timestamp(100), None);
        assert!(!added);
    }

    #[test]
    fn separate_branch_names_are_independent() {
        let mut index = BranchIndexFile::default();
        index.upsert(&actor(), "head", &statement_id_one(), timestamp(100), None);
        index.upsert(
            &actor(),
            "release",
            &statement_id_two(),
            timestamp(50),
            None,
        );
        assert_eq!(
            index
                .lookup_head(&actor(), "head")
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
        assert_eq!(
            index
                .lookup_head(&actor(), "release")
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_two().as_str())
        );
    }

    #[test]
    fn into_tips_lists_all_branches() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = BranchIndexFile::default();
        index.upsert(&actor(), "head", &statement_id_one(), timestamp(100), None);
        index.upsert(
            &actor(),
            "release",
            &statement_id_two(),
            timestamp(200),
            None,
        );
        let tips = index.into_tips(&object())?;
        assert_eq!(tips.len(), 2);
        Ok(())
    }
}
