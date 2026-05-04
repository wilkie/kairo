//! Per-grantor materialized index of `ActorCapabilityGrant` chains
//! and `ActorCapabilityRevocation` records.
//!
//! Each file at `<root>/actor_capability/<XX>/<YY>/<grantor-id>.json`
//! records every known grant *signed by* that grantor, plus every
//! revocation that grantor has issued. Grants are keyed by
//! `(grantee, scope)` because the dominant query is "for this
//! `(grantor, grantee, scope)` triple, what is in effect?" Resolution
//! computes the current head per `(grantee, scope)` from these entries
//! on read; the index does not pre-pick a winner because chain-
//! precedence (a statement that names another via `supersedes`)
//! overrides timestamp-only ordering, so the head can change as new
//! entries arrive. See `specs/CAPABILITIES.md` §5.1.1.
//!
//! Sharding is on `grantor` (the signer) rather than on the grantee
//! or the scope, because the grantor is the one *responsible* for
//! maintaining and revoking the grants they issue — the audit query
//! "what has actor `A` delegated lately?" and the key-compromise
//! cleanup runbook (`specs/CAPABILITIES.md` §7.1) drive locality.
//! Decision A in §9 of that spec records the trade-off.
//!
//! Revocations live alongside grants in the same per-grantor file
//! because in the v1 model only the original grantor may revoke
//! (`specs/CAPABILITIES.md` §5.2). The `(grantor, revoked_grant)`
//! pair is the natural lookup key. Multiple revocations targeting the
//! same grant are tolerated (replay tolerance, §6.3); the most-
//! restrictive wins on read.
//!
//! The index is a strict materialization of the underlying statements:
//! if it is lost or corrupt, it can be rebuilt by scanning all
//! capability statements signed by the grantor. The MVP does not
//! implement rebuild; it relies on always going through
//! `put_actor_capability_grant` and `put_actor_capability_revocation`.
//!
//! Note on path layout: `specs/CAPABILITIES.md` §5.3 originally
//! sketched a nested per-grantee directory layout. That sketch
//! pre-dates the resolved store conventions in `kairo-store/AGENTS.md`
//! ("Don't invent a different scheme for new record types — uniformity
//! is the point"). The implementation follows the trust precedent: the
//! signed statement lives in `STATEMENTS_DIR` and this file is the
//! materialized index. The spec is updated to match.
//!
//! Format (JSON, one file per grantor):
//!
//! ```json
//! {
//!   "grants": {
//!     "<grantee-id>": {
//!       "<scope-key>": [
//!         {
//!           "statement_id": "<statement-id>",
//!           "created_at": "<RFC 3339>",
//!           "supersedes": "<statement-id>" | null
//!         },
//!         ...
//!       ]
//!     }
//!   },
//!   "revocations": {
//!     "<revoked-grant-id>": [
//!       {
//!         "statement_id": "<statement-id>",
//!         "created_at": "<RFC 3339>",
//!         "retroactive": true | false
//!       },
//!       ...
//!     ]
//!   }
//! }
//! ```
//!
//! `<scope-key>` is `"object:<id>"` for `CapabilityScope::Object` or
//! `"actor:<id>"` for `CapabilityScope::Actor`. The same wire form is
//! used by canonical envelopes (`subject = "actor:<grantee-id>"` etc.).
//!
//! Multi-process safety is not yet enforced; concurrent writers can
//! race on read-modify-write. File locks land alongside the rest of
//! the multi-process MVP work.

use std::collections::{BTreeMap, HashSet};

use kairo_core::{ActorId, ObjectId, StatementId, Timestamp};
use kairo_statement::CapabilityScope;
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};

/// On-disk representation of one grant chain entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CapabilityGrantEntry {
    pub statement_id: String,
    pub created_at: String,
    pub supersedes: Option<String>,
}

/// On-disk representation of one revocation entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CapabilityRevocationEntry {
    pub statement_id: String,
    pub created_at: String,
    pub retroactive: bool,
}

/// On-disk representation of all capability statements signed by a
/// single grantor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CapabilityIndexFile {
    /// `grantee-id -> (scope-key -> entries[])`. Entries within a key
    /// are stored in insertion order; resolution computes the head
    /// from the chain on read.
    #[serde(default)]
    pub grants: BTreeMap<String, BTreeMap<String, Vec<CapabilityGrantEntry>>>,
    /// `revoked-grant-statement-id -> entries[]`. Multiple revocations
    /// targeting the same grant are tolerated; the most-restrictive
    /// wins on read (any `retroactive = true` overrides; otherwise the
    /// earliest non-retroactive sets the cutoff).
    #[serde(default)]
    pub revocations: BTreeMap<String, Vec<CapabilityRevocationEntry>>,
}

/// Public summary of one capability head — what callers see when
/// listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityHead {
    pub grantor: ActorId,
    pub grantee: ActorId,
    pub scope: CapabilityScope,
    pub statement_id: StatementId,
    pub created_at: Timestamp,
}

/// Public summary of the effective revocation for a particular grant —
/// what callers see when listing or querying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRevocationRecord {
    pub grantor: ActorId,
    pub revoked_grant: StatementId,
    pub statement_id: StatementId,
    pub created_at: Timestamp,
    pub retroactive: bool,
}

impl CapabilityIndexFile {
    /// Append a grant entry. Returns `true` if the entry was actually
    /// added (i.e. its `statement_id` was not already present); a
    /// duplicate `put` is a no-op.
    pub(crate) fn upsert_grant(
        &mut self,
        grantee: &ActorId,
        scope: &CapabilityScope,
        statement_id: &StatementId,
        created_at: Timestamp,
        supersedes: Option<&StatementId>,
    ) -> bool {
        let by_scope = self.grants.entry(grantee.to_string()).or_default();
        let entries = by_scope.entry(scope_key(scope)).or_default();
        let new_id = statement_id.to_string();
        if entries.iter().any(|e| e.statement_id == new_id) {
            return false;
        }
        entries.push(CapabilityGrantEntry {
            statement_id: new_id,
            created_at: created_at.to_string(),
            supersedes: supersedes.map(|s| s.to_string()),
        });
        true
    }

    /// Append a revocation entry. Returns `true` if added.
    pub(crate) fn upsert_revocation(
        &mut self,
        revoked_grant: &StatementId,
        statement_id: &StatementId,
        created_at: Timestamp,
        retroactive: bool,
    ) -> bool {
        let entries = self
            .revocations
            .entry(revoked_grant.to_string())
            .or_default();
        let new_id = statement_id.to_string();
        if entries.iter().any(|e| e.statement_id == new_id) {
            return false;
        }
        entries.push(CapabilityRevocationEntry {
            statement_id: new_id,
            created_at: created_at.to_string(),
            retroactive,
        });
        true
    }

    /// Resolve the chain head for `(grantee, scope)`. The head is the
    /// supersedes-chain leaf among that triple's entries. If multiple
    /// leaves exist (a fork), pick the one with the greatest
    /// `(created_at, statement_id)`.
    ///
    /// Per-grantor keying inside the file (combined with per-`(grantee,
    /// scope)` nesting) structurally enforces that `supersedes` only
    /// ever resolves against entries belonging to the same `(grantor,
    /// grantee, scope)` triple. Cross-triple supersedes is invalid at
    /// the canonical schema layer.
    pub(crate) fn lookup_grant_head(
        &self,
        grantee: &ActorId,
        scope: &CapabilityScope,
    ) -> Option<&CapabilityGrantEntry> {
        let entries = self
            .grants
            .get(grantee.as_str())
            .and_then(|by_scope| by_scope.get(&scope_key(scope)))?;
        chain_head(entries)
    }

    /// Resolve the effective revocation for `revoked_grant`, if any.
    /// Most-restrictive wins: any `retroactive = true` revocation
    /// overrides; otherwise the earliest non-retroactive entry sets
    /// the cutoff.
    pub(crate) fn lookup_revocation(
        &self,
        revoked_grant: &StatementId,
    ) -> Option<&CapabilityRevocationEntry> {
        let entries = self.revocations.get(revoked_grant.as_str())?;
        most_restrictive_revocation(entries)
    }

    /// Resolve all `(grantee, scope, head)` triples known about
    /// `grantor`. The `grantor` is the file context — it's passed in
    /// so the returned heads can name it.
    pub(crate) fn into_heads(
        self,
        grantor: &ActorId,
    ) -> Result<Vec<CapabilityHead>, StoreError> {
        let mut heads = Vec::new();
        for (grantee_str, by_scope) in &self.grants {
            let grantee =
                ActorId::new(grantee_str.clone()).map_err(|error| StoreError::Corrupt {
                    id: grantee_str.clone(),
                    reason: CorruptReason::Parse(format!(
                        "invalid grantee actor id in capability index: {error}"
                    )),
                })?;
            for (scope_key, entries) in by_scope {
                let Some(head_entry) = chain_head(entries) else {
                    continue;
                };
                let scope = parse_scope_key(scope_key).map_err(|reason| StoreError::Corrupt {
                    id: head_entry.statement_id.clone(),
                    reason: CorruptReason::Parse(reason),
                })?;
                let statement_id = StatementId::new(head_entry.statement_id.clone()).map_err(
                    |error| StoreError::Corrupt {
                        id: head_entry.statement_id.clone(),
                        reason: CorruptReason::Parse(format!(
                            "invalid statement id in capability index: {error}"
                        )),
                    },
                )?;
                let created_at: Timestamp = head_entry.created_at.parse().map_err(|error| {
                    StoreError::Corrupt {
                        id: head_entry.statement_id.clone(),
                        reason: CorruptReason::Parse(format!(
                            "invalid created_at in capability index: {error}"
                        )),
                    }
                })?;
                heads.push(CapabilityHead {
                    grantor: grantor.clone(),
                    grantee: grantee.clone(),
                    scope,
                    statement_id,
                    created_at,
                });
            }
        }
        Ok(heads)
    }
}

/// Encode a `CapabilityScope` as the BTreeMap key used inside an index
/// file. Mirrors the `subject = "actor:<grantee-id>"` envelope
/// convention.
pub(crate) fn scope_key(scope: &CapabilityScope) -> String {
    match scope {
        CapabilityScope::Object(id) => format!("object:{id}"),
        CapabilityScope::Actor(id) => format!("actor:{id}"),
    }
}

/// Parse a scope key produced by [`scope_key`]. Returns a corruption
/// message on failure.
pub(crate) fn parse_scope_key(key: &str) -> Result<CapabilityScope, String> {
    if let Some(rest) = key.strip_prefix("object:") {
        let id = ObjectId::new(rest.to_string()).map_err(|error| {
            format!("invalid object id in capability scope key {key:?}: {error}")
        })?;
        Ok(CapabilityScope::Object(id))
    } else if let Some(rest) = key.strip_prefix("actor:") {
        let id = ActorId::new(rest.to_string()).map_err(|error| {
            format!("invalid actor id in capability scope key {key:?}: {error}")
        })?;
        Ok(CapabilityScope::Actor(id))
    } else {
        Err(format!("unknown capability scope key prefix in {key:?}"))
    }
}

/// Pick the chain head from a set of grant entries. Returns `None` if
/// `entries` is empty. Otherwise:
///   - Mark each entry as "superseded" if any sibling's `supersedes`
///     names it.
///   - The leaves are entries that are not superseded.
///   - If exactly one leaf, that's the head.
///   - If multiple leaves (a fork — same triple has two grants both
///     pointing at the same predecessor, or two genesis grants),
///     tiebreak on `(created_at, statement_id)` descending.
fn chain_head(entries: &[CapabilityGrantEntry]) -> Option<&CapabilityGrantEntry> {
    if entries.is_empty() {
        return None;
    }
    let superseded: HashSet<&str> = entries
        .iter()
        .filter_map(|e| e.supersedes.as_deref())
        .collect();
    let mut best: Option<&CapabilityGrantEntry> = None;
    for entry in entries {
        if superseded.contains(entry.statement_id.as_str()) {
            continue;
        }
        match best {
            None => best = Some(entry),
            Some(current) if grant_entry_greater_than(entry, current) => best = Some(entry),
            _ => {}
        }
    }
    best
}

fn grant_entry_greater_than(
    candidate: &CapabilityGrantEntry,
    current: &CapabilityGrantEntry,
) -> bool {
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

/// Pick the most-restrictive revocation. Any `retroactive = true`
/// entry wins outright (oldest such by `(created_at, statement_id)`,
/// since retroactive cuts from inception). Otherwise pick the
/// earliest non-retroactive entry, since its `created_at` sets the
/// strictest cutoff for "statements after the revocation."
fn most_restrictive_revocation(
    entries: &[CapabilityRevocationEntry],
) -> Option<&CapabilityRevocationEntry> {
    if entries.is_empty() {
        return None;
    }
    let mut best: Option<&CapabilityRevocationEntry> = None;
    for entry in entries {
        match best {
            None => best = Some(entry),
            Some(current) => {
                if revocation_more_restrictive(entry, current) {
                    best = Some(entry);
                }
            }
        }
    }
    best
}

fn revocation_more_restrictive(
    candidate: &CapabilityRevocationEntry,
    current: &CapabilityRevocationEntry,
) -> bool {
    match (candidate.retroactive, current.retroactive) {
        (true, false) => true,
        (false, true) => false,
        // Same retroactivity — pick the entry with the *earlier*
        // (created_at, statement_id), since it sets the strictest
        // cutoff. (For retroactive entries, both invalidate from
        // inception, so this just picks a deterministic survivor.)
        _ => revocation_earlier(candidate, current),
    }
}

fn revocation_earlier(
    candidate: &CapabilityRevocationEntry,
    current: &CapabilityRevocationEntry,
) -> bool {
    match (
        candidate.created_at.parse::<Timestamp>(),
        current.created_at.parse::<Timestamp>(),
    ) {
        (Ok(a), Ok(b)) => {
            if a < b {
                return true;
            }
            if a > b {
                return false;
            }
            candidate.statement_id < current.statement_id
        }
        _ => candidate.statement_id < current.statement_id,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn grantor() -> ActorId {
        ActorId::from_sha256_digest([1; 32])
    }

    fn grantee_a() -> ActorId {
        ActorId::from_sha256_digest([2; 32])
    }

    fn grantee_b() -> ActorId {
        ActorId::from_sha256_digest([3; 32])
    }

    fn object() -> ObjectId {
        ObjectId::from_sha256_digest([9; 32])
    }

    fn other_object() -> ObjectId {
        ObjectId::from_sha256_digest([10; 32])
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
    fn empty_index_has_no_head() {
        let index = CapabilityIndexFile::default();
        let scope = CapabilityScope::Object(object());
        assert!(index.lookup_grant_head(&grantee_a(), &scope).is_none());
    }

    #[test]
    fn single_grant_is_head() {
        let mut index = CapabilityIndexFile::default();
        let scope = CapabilityScope::Object(object());
        let added = index.upsert_grant(
            &grantee_a(),
            &scope,
            &statement_id_one(),
            timestamp(100),
            None,
        );
        assert!(added);
        assert_eq!(
            index
                .lookup_grant_head(&grantee_a(), &scope)
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
    }

    #[test]
    fn duplicate_grant_put_is_noop() {
        let mut index = CapabilityIndexFile::default();
        let scope = CapabilityScope::Object(object());
        index.upsert_grant(
            &grantee_a(),
            &scope,
            &statement_id_one(),
            timestamp(100),
            None,
        );
        let added = index.upsert_grant(
            &grantee_a(),
            &scope,
            &statement_id_one(),
            timestamp(100),
            None,
        );
        assert!(!added);
    }

    #[test]
    fn supersedes_chain_picks_leaf_regardless_of_timestamp_tiebreak() {
        // Genesis grant signed first, then a successor that supersedes
        // it at the same created_at. Chain-precedence picks the
        // successor regardless of statement-id ordering.
        let mut index = CapabilityIndexFile::default();
        let scope = CapabilityScope::Object(object());
        index.upsert_grant(
            &grantee_a(),
            &scope,
            &statement_id_two(),
            timestamp(100),
            None,
        );
        index.upsert_grant(
            &grantee_a(),
            &scope,
            &statement_id_one(),
            timestamp(100),
            Some(&statement_id_two()),
        );
        let head = index
            .lookup_grant_head(&grantee_a(), &scope)
            .expect("head present");
        assert_eq!(head.statement_id.as_str(), statement_id_one().as_str());
    }

    #[test]
    fn fork_tiebreaks_on_created_at_then_statement_id() {
        let mut index = CapabilityIndexFile::default();
        let scope = CapabilityScope::Object(object());
        index.upsert_grant(
            &grantee_a(),
            &scope,
            &statement_id_one(),
            timestamp(100),
            None,
        );
        index.upsert_grant(
            &grantee_a(),
            &scope,
            &statement_id_two(),
            timestamp(200),
            Some(&statement_id_one()),
        );
        index.upsert_grant(
            &grantee_a(),
            &scope,
            &statement_id_three(),
            timestamp(200),
            Some(&statement_id_one()),
        );
        // Both _two and _three supersede _one; both are leaves at the
        // same created_at. Tiebreak prefers the lex-greater statement
        // id (_three > _two).
        assert_eq!(
            index
                .lookup_grant_head(&grantee_a(), &scope)
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_three().as_str())
        );
    }

    #[test]
    fn separate_grantees_are_independent() {
        let mut index = CapabilityIndexFile::default();
        let scope = CapabilityScope::Object(object());
        index.upsert_grant(
            &grantee_a(),
            &scope,
            &statement_id_one(),
            timestamp(100),
            None,
        );
        index.upsert_grant(
            &grantee_b(),
            &scope,
            &statement_id_two(),
            timestamp(50),
            None,
        );
        assert_eq!(
            index
                .lookup_grant_head(&grantee_a(), &scope)
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
        assert_eq!(
            index
                .lookup_grant_head(&grantee_b(), &scope)
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_two().as_str())
        );
    }

    #[test]
    fn separate_scopes_are_independent_for_same_grantee() {
        // Same (grantor, grantee), two different objects. Each scope
        // has its own chain.
        let mut index = CapabilityIndexFile::default();
        let scope_one = CapabilityScope::Object(object());
        let scope_two = CapabilityScope::Object(other_object());
        index.upsert_grant(
            &grantee_a(),
            &scope_one,
            &statement_id_one(),
            timestamp(100),
            None,
        );
        index.upsert_grant(
            &grantee_a(),
            &scope_two,
            &statement_id_two(),
            timestamp(100),
            None,
        );
        assert_eq!(
            index
                .lookup_grant_head(&grantee_a(), &scope_one)
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
        assert_eq!(
            index
                .lookup_grant_head(&grantee_a(), &scope_two)
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_two().as_str())
        );
    }

    #[test]
    fn no_revocation_returns_none() {
        let index = CapabilityIndexFile::default();
        assert!(index.lookup_revocation(&statement_id_one()).is_none());
    }

    #[test]
    fn single_revocation_is_returned() {
        let mut index = CapabilityIndexFile::default();
        let added = index.upsert_revocation(
            &statement_id_one(),
            &statement_id_two(),
            timestamp(200),
            false,
        );
        assert!(added);
        let entry = index
            .lookup_revocation(&statement_id_one())
            .expect("revocation present");
        assert_eq!(entry.statement_id.as_str(), statement_id_two().as_str());
        assert!(!entry.retroactive);
    }

    #[test]
    fn retroactive_revocation_overrides_non_retroactive() {
        // Two revocations target the same grant. The retroactive one
        // wins, even if it was issued later than a non-retroactive
        // sibling (replay tolerance + most-restrictive-wins).
        let mut index = CapabilityIndexFile::default();
        index.upsert_revocation(
            &statement_id_one(),
            &statement_id_two(),
            timestamp(200),
            false,
        );
        index.upsert_revocation(
            &statement_id_one(),
            &statement_id_three(),
            timestamp(300),
            true,
        );
        let effective = index
            .lookup_revocation(&statement_id_one())
            .expect("revocation present");
        assert_eq!(
            effective.statement_id.as_str(),
            statement_id_three().as_str()
        );
        assert!(effective.retroactive);
    }

    #[test]
    fn earliest_non_retroactive_revocation_sets_cutoff() {
        // Two non-retroactive revocations target the same grant.
        // The earlier one's created_at is the strictest cutoff.
        let mut index = CapabilityIndexFile::default();
        index.upsert_revocation(
            &statement_id_one(),
            &statement_id_two(),
            timestamp(300),
            false,
        );
        index.upsert_revocation(
            &statement_id_one(),
            &statement_id_three(),
            timestamp(200),
            false,
        );
        let effective = index
            .lookup_revocation(&statement_id_one())
            .expect("revocation present");
        assert_eq!(
            effective.statement_id.as_str(),
            statement_id_three().as_str()
        );
    }

    #[test]
    fn into_heads_lists_each_triple_head() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = CapabilityIndexFile::default();
        let scope_one = CapabilityScope::Object(object());
        let scope_two = CapabilityScope::Object(other_object());
        index.upsert_grant(
            &grantee_a(),
            &scope_one,
            &statement_id_one(),
            timestamp(100),
            None,
        );
        index.upsert_grant(
            &grantee_a(),
            &scope_one,
            &statement_id_two(),
            timestamp(200),
            Some(&statement_id_one()),
        );
        index.upsert_grant(
            &grantee_b(),
            &scope_two,
            &statement_id_three(),
            timestamp(50),
            None,
        );

        let heads = index.into_heads(&grantor())?;
        assert_eq!(heads.len(), 2);
        let a_head = heads
            .iter()
            .find(|h| h.grantee == grantee_a())
            .expect("grantee_a head present");
        assert_eq!(a_head.statement_id, statement_id_two());
        assert_eq!(a_head.scope, scope_one);
        let b_head = heads
            .iter()
            .find(|h| h.grantee == grantee_b())
            .expect("grantee_b head present");
        assert_eq!(b_head.statement_id, statement_id_three());
        assert_eq!(b_head.scope, scope_two);
        Ok(())
    }

    #[test]
    fn scope_key_round_trip_object() -> Result<(), Box<dyn std::error::Error>> {
        let scope = CapabilityScope::Object(object());
        let key = scope_key(&scope);
        assert!(key.starts_with("object:"));
        let parsed = parse_scope_key(&key).expect("scope parses");
        assert_eq!(parsed, scope);
        Ok(())
    }

    #[test]
    fn scope_key_round_trip_actor() -> Result<(), Box<dyn std::error::Error>> {
        let scope = CapabilityScope::Actor(grantee_a());
        let key = scope_key(&scope);
        assert!(key.starts_with("actor:"));
        let parsed = parse_scope_key(&key).expect("scope parses");
        assert_eq!(parsed, scope);
        Ok(())
    }

    #[test]
    fn scope_key_unknown_prefix_errors() {
        let result = parse_scope_key("blob:zSomeId");
        assert!(result.is_err());
    }
}
