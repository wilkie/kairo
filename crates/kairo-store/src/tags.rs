//! Per-object materialized index of `ObjectVersionTag` heads.
//!
//! Each file at `<root>/version_tags/<XX>/<YY>/<object-id>.json` holds the
//! current winning tag head for that object — one entry per
//! `(actor, version)` — materialized from the underlying
//! `ObjectVersionTag` statements stored in `statements/`. Resolution reads
//! one file per lookup; supersession on write keeps only the greatest
//! `(created_at, statement_id)` head per key.
//!
//! The index is a strict materialization of the underlying statements: if
//! it is lost or corrupt, it can be rebuilt by scanning all
//! `ObjectVersionTag` statements about an object. The MVP does not
//! implement rebuild; it relies on always going through
//! `put_object_version_tag`.
//!
//! Format (JSON, one file per object):
//!
//! ```json
//! {
//!   "<actor-id>": {
//!     "<semver>": {
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

/// On-disk representation of one tag head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VersionTagHeadEntry {
    pub statement_id: String,
    pub created_at: String,
}

/// On-disk representation of all tag heads for a single object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct VersionTagIndexFile {
    /// `actor-id -> (semver -> head)`.
    pub by_actor: BTreeMap<String, BTreeMap<String, VersionTagHeadEntry>>,
}

/// Public summary of a tag head — what callers see when listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionTagHead {
    pub actor: ActorId,
    pub object: ObjectId,
    pub version: String,
    pub statement_id: StatementId,
    pub created_at: Timestamp,
}

impl VersionTagIndexFile {
    /// Apply a head to the index. Returns `true` if the index was updated
    /// (i.e. the new head strictly supersedes whatever was there before).
    /// Same-key ties resolve on `statement_id`; equal entries are no-ops.
    pub(crate) fn upsert(
        &mut self,
        actor: &ActorId,
        version: &str,
        statement_id: &StatementId,
        created_at: Timestamp,
    ) -> bool {
        let by_version = self.by_actor.entry(actor.to_string()).or_default();

        let new_entry = VersionTagHeadEntry {
            statement_id: statement_id.to_string(),
            created_at: created_at.to_string(),
        };

        match by_version.get(version) {
            Some(existing) if !is_strictly_greater(statement_id, created_at, existing) => false,
            _ => {
                by_version.insert(version.to_owned(), new_entry);
                true
            }
        }
    }

    pub(crate) fn lookup(&self, actor: &ActorId, version: &str) -> Option<&VersionTagHeadEntry> {
        self.by_actor
            .get(actor.as_str())
            .and_then(|by_version| by_version.get(version))
    }

    pub(crate) fn into_heads(
        self,
        object: &ObjectId,
    ) -> Result<Vec<VersionTagHead>, StoreError> {
        let mut heads = Vec::new();
        for (actor_str, by_version) in self.by_actor {
            let actor = ActorId::new(actor_str.clone()).map_err(|error| StoreError::Corrupt {
                id: actor_str,
                reason: CorruptReason::Parse(format!("invalid actor id in tag index: {error}")),
            })?;
            for (version, entry) in by_version {
                let statement_id =
                    StatementId::new(entry.statement_id.clone()).map_err(|error| {
                        StoreError::Corrupt {
                            id: entry.statement_id.clone(),
                            reason: CorruptReason::Parse(format!(
                                "invalid statement id in tag index: {error}"
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
                                "invalid created_at in tag index: {error}"
                            )),
                        })?;
                heads.push(VersionTagHead {
                    actor: actor.clone(),
                    object: object.clone(),
                    version,
                    statement_id,
                    created_at,
                });
            }
        }
        Ok(heads)
    }
}

/// `(new_created_at, new_statement_id) > (existing.created_at, existing.statement_id)`
/// in lexicographic order. Equal values are NOT strictly greater (the
/// existing entry wins, so a no-op write does not flap the index).
fn is_strictly_greater(
    new_statement_id: &StatementId,
    new_created_at: Timestamp,
    existing: &VersionTagHeadEntry,
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
    fn empty_index_returns_no_head() {
        let index = VersionTagIndexFile::default();
        assert!(index.lookup(&actor(), "1.2.3").is_none());
    }

    #[test]
    fn upsert_returns_true_for_new_entry() {
        let mut index = VersionTagIndexFile::default();
        let updated = index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100));
        assert!(updated);
        assert!(index.lookup(&actor(), "1.2.3").is_some());
    }

    #[test]
    fn later_created_at_supersedes() {
        let mut index = VersionTagIndexFile::default();
        index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100));
        let updated = index.upsert(&actor(), "1.2.3", &statement_id_two(), timestamp(200));
        assert!(updated);
        assert_eq!(
            index.lookup(&actor(), "1.2.3").map(|e| e.statement_id.as_str()),
            Some(statement_id_two().as_str())
        );
    }

    #[test]
    fn earlier_created_at_does_not_supersede() {
        let mut index = VersionTagIndexFile::default();
        index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(200));
        let updated = index.upsert(&actor(), "1.2.3", &statement_id_two(), timestamp(100));
        assert!(!updated);
        assert_eq!(
            index.lookup(&actor(), "1.2.3").map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
    }

    #[test]
    fn equal_created_at_resolves_on_statement_id() {
        // statement_id_two() > statement_id_one() lexicographically.
        let mut index = VersionTagIndexFile::default();
        index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100));
        let updated = index.upsert(&actor(), "1.2.3", &statement_id_two(), timestamp(100));
        assert!(updated);
        assert_eq!(
            index.lookup(&actor(), "1.2.3").map(|e| e.statement_id.as_str()),
            Some(statement_id_two().as_str())
        );
    }

    #[test]
    fn equal_entry_is_noop() {
        let mut index = VersionTagIndexFile::default();
        index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100));
        let updated = index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100));
        assert!(!updated);
    }

    #[test]
    fn separate_versions_do_not_supersede_each_other() {
        let mut index = VersionTagIndexFile::default();
        index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100));
        let updated = index.upsert(&actor(), "1.2.4", &statement_id_two(), timestamp(50));
        assert!(updated);
        assert!(index.lookup(&actor(), "1.2.3").is_some());
        assert!(index.lookup(&actor(), "1.2.4").is_some());
    }

    #[test]
    fn into_heads_lists_all_versions() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = VersionTagIndexFile::default();
        index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100));
        index.upsert(&actor(), "1.2.4", &statement_id_two(), timestamp(200));
        let heads = index.into_heads(&object())?;
        assert_eq!(heads.len(), 2);
        Ok(())
    }
}
