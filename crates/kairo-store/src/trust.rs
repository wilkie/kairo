//! Per-trusted-actor materialized index of `ActorTrust` entries.
//!
//! Each file at `<root>/trust/<XX>/<YY>/<trusted-actor-id>.json` records
//! every known `ActorTrust` statement *about* that trusted actor — one
//! entry per `(by_actor, statement_id)` — materialized from the
//! underlying signed statements stored in `statements/`. Resolution
//! computes the current head per `by_actor` from these entries on read;
//! the index does not pre-pick a winner because chain-precedence (a
//! statement that names another via `supersedes`) overrides
//! timestamp-only ordering, so the head can change as new entries
//! arrive.
//!
//! Sharding is on `trusted_actor` (the subject) rather than on the
//! truster, because aggregation queries — "what does the world say
//! about Y?" — drive federation: when a peer publishes new trust
//! statements, you want O(1) access to the current set of opinions
//! about a given actor. The first-person query "does X trust Y?"
//! stays O(1) too: open Y's file, look up X.
//!
//! The "cross-actor `supersedes` is invalid" rule still holds
//! structurally: the chain resolver only walks entries under a single
//! `by_actor` key, so a `supersedes` reference can only ever collide
//! with same-actor entries.
//!
//! The index is a strict materialization of the underlying statements:
//! if it is lost or corrupt, it can be rebuilt by scanning all
//! `ActorTrust` statements about a trusted actor. The MVP does not
//! implement rebuild; it relies on always going through
//! `put_actor_trust`.
//!
//! Format (JSON, one file per trusted actor):
//!
//! ```json
//! {
//!   "<by-actor-id>": [
//!     {
//!       "statement_id": "<statement-id>",
//!       "created_at": "<RFC 3339>",
//!       "decision": "trusted" | "untrusted" | null,
//!       "supersedes": "<statement-id>" | null
//!     },
//!     ...
//!   ],
//!   "<another-by-actor-id>": [...]
//! }
//! ```
//!
//! Multi-process safety is not yet enforced; concurrent writers can
//! race on read-modify-write. File locks land alongside the rest of
//! the multi-process MVP work.

use std::collections::{BTreeMap, HashSet};

use kairo_core::{ActorId, StatementId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};

/// On-disk representation of one trust entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TrustEntry {
    pub statement_id: String,
    pub created_at: String,
    /// `"trusted"`, `"untrusted"`, or `None` (withdrawal).
    pub decision: Option<String>,
    pub supersedes: Option<String>,
}

/// On-disk representation of all trust entries about a single trusted
/// actor, keyed by truster.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct TrustIndexFile {
    /// `by-actor-id (truster) -> entries[]`. Entries within a key are
    /// stored in insertion order; resolution computes the head from
    /// the chain on read.
    pub by_truster: BTreeMap<String, Vec<TrustEntry>>,
}

/// Public summary of a trust head — what callers see when listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustHead {
    pub by_actor: ActorId,
    pub trusted_actor: ActorId,
    pub statement_id: StatementId,
    pub created_at: Timestamp,
    /// `Some("trusted")`, `Some("untrusted")`, or `None` (withdrawal).
    pub decision: Option<String>,
}

impl TrustIndexFile {
    /// Append an entry under `by_actor`. Returns `true` if the entry
    /// was actually added (i.e. its `statement_id` was not already
    /// present); a duplicate `put` is a no-op.
    pub(crate) fn upsert(
        &mut self,
        by_actor: &ActorId,
        statement_id: &StatementId,
        created_at: Timestamp,
        decision: Option<&str>,
        supersedes: Option<&StatementId>,
    ) -> bool {
        let entries = self.by_truster.entry(by_actor.to_string()).or_default();
        let new_id = statement_id.to_string();
        if entries.iter().any(|e| e.statement_id == new_id) {
            return false;
        }
        entries.push(TrustEntry {
            statement_id: new_id,
            created_at: created_at.to_string(),
            decision: decision.map(|d| d.to_owned()),
            supersedes: supersedes.map(|s| s.to_string()),
        });
        true
    }

    /// Resolve the head for `by_actor`. The head is the
    /// supersedes-chain leaf among that truster's entries. If multiple
    /// leaves exist (a fork), pick the one with the greatest
    /// `(created_at, statement_id)`.
    ///
    /// Per-truster keying inside the file structurally enforces that
    /// `supersedes` only ever resolves against same-actor entries,
    /// matching the canonical schema's "cross-actor supersedes is
    /// invalid" rule.
    pub(crate) fn lookup_head(&self, by_actor: &ActorId) -> Option<&TrustEntry> {
        let entries = self.by_truster.get(by_actor.as_str())?;
        chain_head(entries)
    }

    /// Resolve all `(by_actor, head)` pairs known about
    /// `trusted_actor`. The `trusted_actor` is the file context — it's
    /// passed in so the returned heads can name it.
    pub(crate) fn into_heads(
        self,
        trusted_actor: &ActorId,
    ) -> Result<Vec<TrustHead>, StoreError> {
        let mut heads = Vec::new();
        for (truster_str, entries) in &self.by_truster {
            let by_actor =
                ActorId::new(truster_str.clone()).map_err(|error| StoreError::Corrupt {
                    id: truster_str.clone(),
                    reason: CorruptReason::Parse(format!(
                        "invalid truster actor id in trust index: {error}"
                    )),
                })?;
            let Some(head_entry) = chain_head(entries) else {
                continue;
            };
            let statement_id =
                StatementId::new(head_entry.statement_id.clone()).map_err(|error| {
                    StoreError::Corrupt {
                        id: head_entry.statement_id.clone(),
                        reason: CorruptReason::Parse(format!(
                            "invalid statement id in trust index: {error}"
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
                            "invalid created_at in trust index: {error}"
                        )),
                    })?;
            heads.push(TrustHead {
                by_actor,
                trusted_actor: trusted_actor.clone(),
                statement_id,
                created_at,
                decision: head_entry.decision.clone(),
            });
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
///   - If multiple leaves (a fork — same actor signed two trust
///     statements both pointing at the same predecessor, or two
///     genesis statements), tiebreak on `(created_at, statement_id)`
///     descending.
fn chain_head(entries: &[TrustEntry]) -> Option<&TrustEntry> {
    if entries.is_empty() {
        return None;
    }
    let superseded: HashSet<&str> = entries
        .iter()
        .filter_map(|e| e.supersedes.as_deref())
        .collect();
    let mut best: Option<&TrustEntry> = None;
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

fn entry_greater_than(candidate: &TrustEntry, current: &TrustEntry) -> bool {
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
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn truster_a() -> ActorId {
        ActorId::from_sha256_digest([1; 32])
    }

    fn truster_b() -> ActorId {
        ActorId::from_sha256_digest([2; 32])
    }

    fn trusted_target() -> ActorId {
        ActorId::from_sha256_digest([9; 32])
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
        let index = TrustIndexFile::default();
        assert!(index.lookup_head(&truster_a()).is_none());
    }

    #[test]
    fn single_entry_is_head() {
        let mut index = TrustIndexFile::default();
        let added = index.upsert(
            &truster_a(),
            &statement_id_one(),
            timestamp(100),
            Some("trusted"),
            None,
        );
        assert!(added);
        assert_eq!(
            index
                .lookup_head(&truster_a())
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
    }

    #[test]
    fn duplicate_put_is_noop() {
        let mut index = TrustIndexFile::default();
        index.upsert(
            &truster_a(),
            &statement_id_one(),
            timestamp(100),
            Some("trusted"),
            None,
        );
        let added = index.upsert(
            &truster_a(),
            &statement_id_one(),
            timestamp(100),
            Some("trusted"),
            None,
        );
        assert!(!added);
    }

    #[test]
    fn supersedes_chain_picks_leaf_regardless_of_timestamp_tiebreak() {
        // Grant (statement_id_two, lex-greater) signed first; withdraw
        // (statement_id_one, lex-smaller) signed second and explicitly
        // supersedes the grant. Chain-precedence picks the withdrawal.
        let mut index = TrustIndexFile::default();
        index.upsert(
            &truster_a(),
            &statement_id_two(),
            timestamp(100),
            Some("trusted"),
            None,
        );
        index.upsert(
            &truster_a(),
            &statement_id_one(),
            timestamp(100),
            None,
            Some(&statement_id_two()),
        );
        let head = index.lookup_head(&truster_a()).expect("head present");
        assert_eq!(head.statement_id.as_str(), statement_id_one().as_str());
        assert_eq!(head.decision, None);
    }

    #[test]
    fn fork_tiebreaks_on_created_at_then_statement_id() {
        let mut index = TrustIndexFile::default();
        index.upsert(
            &truster_a(),
            &statement_id_one(),
            timestamp(100),
            Some("trusted"),
            None,
        );
        index.upsert(
            &truster_a(),
            &statement_id_two(),
            timestamp(200),
            Some("untrusted"),
            Some(&statement_id_one()),
        );
        index.upsert(
            &truster_a(),
            &statement_id_three(),
            timestamp(200),
            None,
            Some(&statement_id_one()),
        );
        // Both _two and _three supersede _one; both are leaves at the
        // same created_at. Tiebreak prefers the lex-greater statement
        // id (statement_id_three > statement_id_two).
        assert_eq!(
            index
                .lookup_head(&truster_a())
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_three().as_str())
        );
    }

    #[test]
    fn fork_later_timestamp_wins_over_earlier() {
        let mut index = TrustIndexFile::default();
        index.upsert(
            &truster_a(),
            &statement_id_one(),
            timestamp(100),
            Some("trusted"),
            None,
        );
        index.upsert(
            &truster_a(),
            &statement_id_two(),
            timestamp(300),
            Some("untrusted"),
            Some(&statement_id_one()),
        );
        index.upsert(
            &truster_a(),
            &statement_id_three(),
            timestamp(200),
            None,
            Some(&statement_id_one()),
        );
        assert_eq!(
            index
                .lookup_head(&truster_a())
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_two().as_str())
        );
    }

    #[test]
    fn separate_trusters_are_independent() {
        let mut index = TrustIndexFile::default();
        index.upsert(
            &truster_a(),
            &statement_id_one(),
            timestamp(100),
            Some("trusted"),
            None,
        );
        index.upsert(
            &truster_b(),
            &statement_id_two(),
            timestamp(50),
            Some("untrusted"),
            None,
        );
        assert_eq!(
            index
                .lookup_head(&truster_a())
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
        assert_eq!(
            index
                .lookup_head(&truster_b())
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_two().as_str())
        );
    }

    #[test]
    fn into_heads_lists_each_truster_head() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = TrustIndexFile::default();
        index.upsert(
            &truster_a(),
            &statement_id_one(),
            timestamp(100),
            Some("trusted"),
            None,
        );
        index.upsert(
            &truster_a(),
            &statement_id_two(),
            timestamp(200),
            None,
            Some(&statement_id_one()),
        );
        index.upsert(
            &truster_b(),
            &statement_id_three(),
            timestamp(50),
            Some("untrusted"),
            None,
        );

        let heads = index.into_heads(&trusted_target())?;
        assert_eq!(heads.len(), 2);
        let a_head = heads.iter().find(|h| h.by_actor == truster_a());
        assert!(matches!(a_head, Some(h) if h.statement_id == statement_id_two() && h.decision.is_none()));
        let b_head = heads.iter().find(|h| h.by_actor == truster_b());
        assert!(matches!(b_head, Some(h) if h.statement_id == statement_id_three() && h.decision.as_deref() == Some("untrusted")));
        Ok(())
    }
}
