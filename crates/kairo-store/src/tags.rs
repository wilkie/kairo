//! Per-object materialized index of `ObjectVersionTag` entries.
//!
//! Each file at `<root>/version_tags/<XX>/<YY>/<object-id>.json` records
//! every known `ObjectVersionTag` for that object — one entry per
//! `(actor, version, statement_id)` — materialized from the underlying
//! signed statements stored in `statements/`. Resolution computes the
//! current head per `(actor, version)` from these entries on read; the
//! index does not pre-pick a winner because chain-precedence (a
//! statement that names another via `supersedes`) overrides
//! timestamp-only ordering, so the head can change as new entries
//! arrive.
//!
//! Cross-actor `supersedes` references are recorded on each entry but
//! the chain-head computation in this module only walks **same-actor**
//! edges. The authority-aware forward walk that honors cross-actor
//! supersedes when the successor's signer holds an `ObjectVersionTag`
//! capability on the object lives in
//! `FilesystemStore::walk_authorized_tag_chain` and is invoked by the
//! resolver path on top of this index. See
//! `specs/CAPABILITIES.md` §6.2.
//!
//! The index is a strict materialization of the underlying statements:
//! if it is lost or corrupt, it can be rebuilt by scanning all
//! `ObjectVersionTag` statements about an object. The MVP does not
//! implement rebuild; it relies on always going through
//! `put_object_version_tag`.
//!
//! Format (JSON, one file per object):
//!
//! ```json
//! {
//!   "<actor-id>": {
//!     "<semver>": [
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
//! Multi-process safety is not yet enforced; concurrent writers can
//! race on read-modify-write. File locks land alongside the rest of
//! the multi-process MVP work.

use std::collections::{BTreeMap, HashSet};

use kairo_core::{ActorId, ObjectId, StatementId, Timestamp};
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::error::{CorruptReason, StoreError};

/// On-disk representation of one tag entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VersionTagEntry {
    pub statement_id: String,
    pub created_at: String,
    pub supersedes: Option<String>,
}

/// On-disk representation of all tag entries for a single object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct VersionTagIndexFile {
    /// `actor-id -> (semver -> entries[])`. Entries within a key are
    /// stored in insertion order; resolution computes the head from
    /// the chain on read.
    pub by_actor: BTreeMap<String, BTreeMap<String, Vec<VersionTagEntry>>>,
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
    /// Append an entry. Returns `true` if the entry was actually added
    /// (i.e. its `statement_id` was not already present); a duplicate
    /// `put` is a no-op.
    pub(crate) fn upsert(
        &mut self,
        actor: &ActorId,
        version: &str,
        statement_id: &StatementId,
        created_at: Timestamp,
        supersedes: Option<&StatementId>,
    ) -> bool {
        let by_version = self.by_actor.entry(actor.to_string()).or_default();
        let entries = by_version.entry(version.to_owned()).or_default();
        let new_id = statement_id.to_string();
        if entries.iter().any(|e| e.statement_id == new_id) {
            return false;
        }
        entries.push(VersionTagEntry {
            statement_id: new_id,
            created_at: created_at.to_string(),
            supersedes: supersedes.map(|s| s.to_string()),
        });
        true
    }

    /// Resolve the same-actor chain head for `(actor, version)`. The
    /// head is the supersedes-chain leaf among `actor`'s own entries:
    /// an entry whose `statement_id` does not appear in any
    /// **same-actor** sibling entry's `supersedes` field. If the
    /// chain has multiple leaves (a fork), pick the one with the
    /// greatest `(created_at, statement_id)`.
    ///
    /// Cross-actor `supersedes` references are not consulted by this
    /// method — that decision needs an authority check
    /// (`evaluate_capability`) which lives outside the per-object
    /// index. The Step 6 resolver flip combines this same-actor leaf
    /// with the authority-aware forward walk in
    /// `FilesystemStore::walk_authorized_tag_chain` to honor
    /// cross-actor supersedes per `specs/CAPABILITIES.md` §6.2.
    pub(crate) fn lookup_head(&self, actor: &ActorId, version: &str) -> Option<&VersionTagEntry> {
        let entries = self
            .by_actor
            .get(actor.as_str())
            .and_then(|by_version| by_version.get(version))?;
        chain_head(entries)
    }

    /// All entries (across actors) for a given `version`. Returned
    /// pairs are `(actor_id_str, entry)` — used by the cross-actor
    /// walker to find candidate successors that name the current
    /// chain leaf via `supersedes`.
    pub(crate) fn entries_for_version(&self, version: &str) -> Vec<(&str, &VersionTagEntry)> {
        let mut all = Vec::new();
        for (actor_str, by_version) in &self.by_actor {
            if let Some(entries) = by_version.get(version) {
                for entry in entries {
                    all.push((actor_str.as_str(), entry));
                }
            }
        }
        all
    }

    /// Resolve every `(actor, version)` chain head known about
    /// `object`, considering only same-actor `supersedes` edges.
    /// Cross-actor authority-aware enumeration is the
    /// `FilesystemStore::list_version_tags` path; this method is the
    /// raw same-actor projection used directly by tests.
    #[cfg(test)]
    pub(crate) fn into_heads(self, object: &ObjectId) -> Result<Vec<VersionTagHead>, StoreError> {
        let mut heads = Vec::new();
        for (actor_str, by_version) in &self.by_actor {
            let actor = ActorId::new(actor_str.clone()).map_err(|error| StoreError::Corrupt {
                id: actor_str.clone(),
                reason: CorruptReason::Parse(format!("invalid actor id in tag index: {error}")),
            })?;
            for (version, entries) in by_version {
                let Some(head_entry) = chain_head(entries) else {
                    continue;
                };
                let statement_id =
                    StatementId::new(head_entry.statement_id.clone()).map_err(|error| {
                        StoreError::Corrupt {
                            id: head_entry.statement_id.clone(),
                            reason: CorruptReason::Parse(format!(
                                "invalid statement id in tag index: {error}"
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
                                "invalid created_at in tag index: {error}"
                            )),
                        })?;
                heads.push(VersionTagHead {
                    actor: actor.clone(),
                    object: object.clone(),
                    version: version.clone(),
                    statement_id,
                    created_at,
                });
            }
        }
        Ok(heads)
    }
}

/// Pick the chain head from a set of entries. Returns `None` if
/// `entries` is empty. Otherwise:
///   - Mark each entry as "superseded" if any sibling's `supersedes`
///     names it.
///   - The leaves are entries that are not superseded.
///   - If exactly one leaf, that's the head.
///   - If multiple leaves (a fork — same actor signed two tags both
///     pointing at the same predecessor, or two genesis tags), tiebreak
///     on `(created_at, statement_id)` descending.
fn chain_head(entries: &[VersionTagEntry]) -> Option<&VersionTagEntry> {
    if entries.is_empty() {
        return None;
    }
    let superseded: HashSet<&str> = entries
        .iter()
        .filter_map(|e| e.supersedes.as_deref())
        .collect();
    let mut best: Option<&VersionTagEntry> = None;
    for entry in entries {
        if superseded.contains(entry.statement_id.as_str()) {
            continue;
        }
        match best {
            None => best = Some(entry),
            Some(current) if entry_greater_than(entry, current) => best = Some(entry),
            _ => {}
        }
    }
    best
}

fn entry_greater_than(candidate: &VersionTagEntry, current: &VersionTagEntry) -> bool {
    match (
        candidate.created_at.parse::<Timestamp>(),
        current.created_at.parse::<Timestamp>(),
    ) {
        (Ok(a), Ok(b)) => {
            if a > b {
                return true;
            }
            if a < b {
                return false;
            }
            candidate.statement_id > current.statement_id
        }
        // Corrupt timestamp on either side — fall back to lexicographic
        // statement id compare so we still produce a deterministic
        // answer; a subsequent index read surfaces the corruption.
        _ => candidate.statement_id > current.statement_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor() -> ActorId {
        ActorId::from_sha256_digest([1; 32])
    }

    fn other_actor() -> ActorId {
        ActorId::from_sha256_digest([2; 32])
    }

    fn object() -> ObjectId {
        ObjectId::from_sha256_digest([3; 32])
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
    fn empty_index_returns_no_head() {
        let index = VersionTagIndexFile::default();
        assert!(index.lookup_head(&actor(), "1.2.3").is_none());
    }

    #[test]
    fn single_entry_is_head() {
        let mut index = VersionTagIndexFile::default();
        let added = index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100), None);
        assert!(added);
        assert_eq!(
            index
                .lookup_head(&actor(), "1.2.3")
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
    }

    #[test]
    fn duplicate_put_is_noop() {
        let mut index = VersionTagIndexFile::default();
        index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100), None);
        let added = index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100), None);
        assert!(!added);
    }

    #[test]
    fn supersedes_chain_picks_leaf_regardless_of_timestamp_tiebreak() {
        // Bind (statement_id_two, lex-greater) signed first;
        // revoke (statement_id_one, lex-smaller) signed second and
        // explicitly supersedes the bind. With pure timestamp+id
        // tiebreak, statement_id_two would win on lex order; with
        // chain-precedence, statement_id_one wins because it's the
        // chain leaf.
        let mut index = VersionTagIndexFile::default();
        index.upsert(&actor(), "1.2.3", &statement_id_two(), timestamp(100), None);
        index.upsert(
            &actor(),
            "1.2.3",
            &statement_id_one(),
            timestamp(100),
            Some(&statement_id_two()),
        );
        assert_eq!(
            index
                .lookup_head(&actor(), "1.2.3")
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
    }

    #[test]
    fn fork_tiebreaks_on_created_at_then_statement_id() {
        // Two successor tags, both supersede the genesis (a fork). Both
        // are leaves. Pick by (created_at, statement_id) — the later
        // timestamp wins; here both at t=200, so lex compare resolves:
        // statement_id_three > statement_id_two.
        let mut index = VersionTagIndexFile::default();
        index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100), None);
        index.upsert(
            &actor(),
            "1.2.3",
            &statement_id_two(),
            timestamp(200),
            Some(&statement_id_one()),
        );
        index.upsert(
            &actor(),
            "1.2.3",
            &statement_id_three(),
            timestamp(200),
            Some(&statement_id_one()),
        );
        assert_eq!(
            index
                .lookup_head(&actor(), "1.2.3")
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_three().as_str())
        );
    }

    #[test]
    fn fork_later_timestamp_wins_over_earlier() {
        let mut index = VersionTagIndexFile::default();
        index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100), None);
        index.upsert(
            &actor(),
            "1.2.3",
            &statement_id_two(),
            timestamp(300),
            Some(&statement_id_one()),
        );
        index.upsert(
            &actor(),
            "1.2.3",
            &statement_id_three(),
            timestamp(200),
            Some(&statement_id_one()),
        );
        assert_eq!(
            index
                .lookup_head(&actor(), "1.2.3")
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_two().as_str())
        );
    }

    #[test]
    fn cross_actor_supersedes_does_not_suppress_same_actor_leaf() {
        // Actor B signs a tag whose supersedes points at actor A's tag.
        // Per-actor MVP resolver only considers same-actor entries —
        // A's tag remains A's leaf; B's tag is B's leaf.
        let mut index = VersionTagIndexFile::default();
        index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100), None);
        index.upsert(
            &other_actor(),
            "1.2.3",
            &statement_id_two(),
            timestamp(200),
            Some(&statement_id_one()),
        );
        assert_eq!(
            index
                .lookup_head(&actor(), "1.2.3")
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
        assert_eq!(
            index
                .lookup_head(&other_actor(), "1.2.3")
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_two().as_str())
        );
    }

    #[test]
    fn missing_predecessor_does_not_break_leaf_computation() {
        // Successor's supersedes references a statement_id not present
        // in the index (predecessor not yet imported, or signed by a
        // different actor). The successor is still a leaf; nothing
        // else exists to suppress it.
        let mut index = VersionTagIndexFile::default();
        index.upsert(
            &actor(),
            "1.2.3",
            &statement_id_one(),
            timestamp(100),
            Some(&statement_id_three()),
        );
        assert_eq!(
            index
                .lookup_head(&actor(), "1.2.3")
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
    }

    #[test]
    fn separate_versions_are_independent() {
        let mut index = VersionTagIndexFile::default();
        index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100), None);
        index.upsert(&actor(), "1.2.4", &statement_id_two(), timestamp(50), None);
        assert_eq!(
            index
                .lookup_head(&actor(), "1.2.3")
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
        assert_eq!(
            index
                .lookup_head(&actor(), "1.2.4")
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_two().as_str())
        );
    }

    #[test]
    fn into_heads_lists_each_actor_version_head() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = VersionTagIndexFile::default();
        index.upsert(&actor(), "1.2.3", &statement_id_one(), timestamp(100), None);
        index.upsert(
            &actor(),
            "1.2.3",
            &statement_id_two(),
            timestamp(200),
            Some(&statement_id_one()),
        );
        index.upsert(
            &other_actor(),
            "1.2.3",
            &statement_id_three(),
            timestamp(50),
            None,
        );

        let heads = index.into_heads(&object())?;
        assert_eq!(heads.len(), 2);
        // First actor's head is the chain leaf (statement_id_two), not
        // the genesis (statement_id_one).
        let actor_head = heads.iter().find(|h| h.actor == actor());
        assert!(matches!(actor_head, Some(h) if h.statement_id == statement_id_two()));
        let other_head = heads.iter().find(|h| h.actor == other_actor());
        assert!(matches!(other_head, Some(h) if h.statement_id == statement_id_three()));
        Ok(())
    }
}
