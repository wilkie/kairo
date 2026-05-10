//! Per-object cross-cutting reverse index of object-scoped
//! `ActorCapabilityGrant` chains.
//!
//! Each file at
//! `<root>/actor_capability_by_object/<XX>/<YY>/<object-id>.json`
//! records every known *object-scoped* grant whose
//! `Capability::scope` names that object — across all grantors and
//! grantees. The dominant query — "for grantee `B` on object `O`, is
//! there any covering grant?" — is the §6.1 capability evaluator's
//! hot path: the chain leaf may be issued by any of several grantors
//! (an object's root authority plus any actors holding `delegable`
//! grants), and the resolver must consider all of them. The
//! per-grantor index in `capabilities.rs` does not answer this
//! question without iterating every grantor's file.
//!
//! Layout: nested `grantee → grantor → chain entries[]`. Grantee is
//! the outer key because the resolver navigates "for this grantee,
//! enumerate grantors who have issued a covering grant." Within each
//! `(grantor, grantee, object)` triple the chain head is computed on
//! read — the same chain-precedence rule used everywhere else
//! (`supersedes`-leaf wins; fork tiebreak on greatest `(created_at,
//! statement_id)`).
//!
//! Actor-scoped grants are intentionally **not** mirrored here. In
//! v1 no statement kind is valid for `CapabilityScope::Actor` (see
//! `specs/CAPABILITIES.md` §4.3 and the per-`(scope, kind)` table),
//! so an actor-scoped grant would carry no usable kinds and the
//! object-keyed reverse index is the wrong shape for it. When
//! actor-surface kinds land, a parallel `capabilities_by_actor.rs`
//! reverse index will join.
//!
//! Revocations are not duplicated here. When the resolver finds a
//! candidate grant via this index it consults the per-grantor index
//! (`capabilities.rs`) for the corresponding revocation.
//!
//! The index is a strict materialization of the underlying signed
//! statements: if it is lost or corrupt, it can be rebuilt by
//! scanning all object-scoped `ActorCapabilityGrant` statements.
//! The store's `rebuild_indexes()` (surfaced as
//! `kairo store rebuild-indexes`) does exactly this for every
//! materialized index in one pass, including this one.
//!
//! Format (JSON, one file per object):
//!
//! ```json
//! {
//!   "<grantee-id>": {
//!     "<grantor-id>": [
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

use std::collections::BTreeMap;

use kairo_core::{ActorId, ObjectId, StatementId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::chain::{chain_head, ChainEntry};
use crate::error::{CorruptReason, StoreError};

/// On-disk representation of one grant chain entry in the reverse
/// index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CapabilityByObjectEntry {
    pub statement_id: String,
    pub created_at: String,
    pub supersedes: Option<String>,
}

impl ChainEntry for CapabilityByObjectEntry {
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

/// On-disk representation of all object-scoped grants targeting a
/// single object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct CapabilityByObjectIndexFile {
    /// `grantee-id -> (grantor-id -> entries[])`. Entries within a
    /// `(grantee, grantor)` bucket are stored in insertion order;
    /// resolution computes the chain head from those entries on read.
    pub by_grantee: BTreeMap<String, BTreeMap<String, Vec<CapabilityByObjectEntry>>>,
}

/// Public summary of one chain head in the reverse index — what
/// callers see when listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityByObjectHead {
    pub grantor: ActorId,
    pub grantee: ActorId,
    pub object: ObjectId,
    pub statement_id: StatementId,
    pub created_at: Timestamp,
}

impl CapabilityByObjectIndexFile {
    /// Append an entry for `(grantee, grantor)`. Returns `true` if
    /// the entry was actually added (i.e. its `statement_id` was not
    /// already present); a duplicate `put` is a no-op.
    pub(crate) fn upsert(
        &mut self,
        grantee: &ActorId,
        grantor: &ActorId,
        statement_id: &StatementId,
        created_at: Timestamp,
        supersedes: Option<&StatementId>,
    ) -> bool {
        let by_grantor = self.by_grantee.entry(grantee.to_string()).or_default();
        let entries = by_grantor.entry(grantor.to_string()).or_default();
        let new_id = statement_id.to_string();
        if entries.iter().any(|e| e.statement_id == new_id) {
            return false;
        }
        entries.push(CapabilityByObjectEntry {
            statement_id: new_id,
            created_at: created_at.to_string(),
            supersedes: supersedes.map(|s| s.to_string()),
        });
        true
    }

    /// Resolve the chain head for `(grantee, grantor)` on this
    /// object. The head is the supersedes-chain leaf among that
    /// triple's entries.
    ///
    /// Currently only consumed by tests; the Step 5 capability
    /// evaluator will use this primitive when it lands. The
    /// `list_capabilities_for_object` resolver path uses
    /// `into_heads` instead.
    #[allow(dead_code)]
    pub(crate) fn lookup_head(
        &self,
        grantee: &ActorId,
        grantor: &ActorId,
    ) -> Option<&CapabilityByObjectEntry> {
        let entries = self
            .by_grantee
            .get(grantee.as_str())
            .and_then(|by_grantor| by_grantor.get(grantor.as_str()))?;
        chain_head(entries)
    }

    /// Resolve every `(grantor, grantee, head)` triple known about
    /// `object`. The `object` is the file context — it's passed in so
    /// the returned heads can name it.
    pub(crate) fn into_heads(
        self,
        object: &ObjectId,
    ) -> Result<Vec<CapabilityByObjectHead>, StoreError> {
        let mut heads = Vec::new();
        for (grantee_str, by_grantor) in &self.by_grantee {
            let grantee =
                ActorId::new(grantee_str.clone()).map_err(|error| StoreError::Corrupt {
                    id: grantee_str.clone(),
                    reason: CorruptReason::Parse(format!(
                        "invalid grantee actor id in capabilities-by-object index: {error}"
                    )),
                })?;
            for (grantor_str, entries) in by_grantor {
                let grantor =
                    ActorId::new(grantor_str.clone()).map_err(|error| StoreError::Corrupt {
                        id: grantor_str.clone(),
                        reason: CorruptReason::Parse(format!(
                            "invalid grantor actor id in capabilities-by-object index: {error}"
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
                                "invalid statement id in capabilities-by-object index: {error}"
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
                                "invalid created_at in capabilities-by-object index: {error}"
                            )),
                        })?;
                heads.push(CapabilityByObjectHead {
                    grantor,
                    grantee: grantee.clone(),
                    object: object.clone(),
                    statement_id,
                    created_at,
                });
            }
        }
        Ok(heads)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn grantor_a() -> ActorId {
        ActorId::from_sha256_digest([1; 32])
    }

    fn grantor_b() -> ActorId {
        ActorId::from_sha256_digest([2; 32])
    }

    fn grantee_a() -> ActorId {
        ActorId::from_sha256_digest([3; 32])
    }

    fn grantee_b() -> ActorId {
        ActorId::from_sha256_digest([4; 32])
    }

    fn object() -> ObjectId {
        ObjectId::from_sha256_digest([9; 32])
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
        let index = CapabilityByObjectIndexFile::default();
        assert!(index.lookup_head(&grantee_a(), &grantor_a()).is_none());
    }

    #[test]
    fn single_entry_is_head() {
        let mut index = CapabilityByObjectIndexFile::default();
        let added = index.upsert(
            &grantee_a(),
            &grantor_a(),
            &statement_id_one(),
            timestamp(100),
            None,
        );
        assert!(added);
        assert_eq!(
            index
                .lookup_head(&grantee_a(), &grantor_a())
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
    }

    #[test]
    fn duplicate_put_is_noop() {
        let mut index = CapabilityByObjectIndexFile::default();
        index.upsert(
            &grantee_a(),
            &grantor_a(),
            &statement_id_one(),
            timestamp(100),
            None,
        );
        let added = index.upsert(
            &grantee_a(),
            &grantor_a(),
            &statement_id_one(),
            timestamp(100),
            None,
        );
        assert!(!added);
    }

    #[test]
    fn supersedes_chain_picks_leaf() {
        let mut index = CapabilityByObjectIndexFile::default();
        index.upsert(
            &grantee_a(),
            &grantor_a(),
            &statement_id_two(),
            timestamp(100),
            None,
        );
        index.upsert(
            &grantee_a(),
            &grantor_a(),
            &statement_id_one(),
            timestamp(100),
            Some(&statement_id_two()),
        );
        let head = index
            .lookup_head(&grantee_a(), &grantor_a())
            .expect("head present");
        assert_eq!(head.statement_id.as_str(), statement_id_one().as_str());
    }

    #[test]
    fn separate_grantors_for_same_grantee_are_independent() {
        // Two grantors each issue grants on the same object to the
        // same grantee. Each (grantor, grantee, object) triple
        // resolves to its own chain leaf — neither suppresses the
        // other.
        let mut index = CapabilityByObjectIndexFile::default();
        index.upsert(
            &grantee_a(),
            &grantor_a(),
            &statement_id_one(),
            timestamp(100),
            None,
        );
        index.upsert(
            &grantee_a(),
            &grantor_b(),
            &statement_id_two(),
            timestamp(100),
            None,
        );
        assert_eq!(
            index
                .lookup_head(&grantee_a(), &grantor_a())
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
        assert_eq!(
            index
                .lookup_head(&grantee_a(), &grantor_b())
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_two().as_str())
        );
    }

    #[test]
    fn separate_grantees_are_independent() {
        let mut index = CapabilityByObjectIndexFile::default();
        index.upsert(
            &grantee_a(),
            &grantor_a(),
            &statement_id_one(),
            timestamp(100),
            None,
        );
        index.upsert(
            &grantee_b(),
            &grantor_a(),
            &statement_id_two(),
            timestamp(100),
            None,
        );
        assert_eq!(
            index
                .lookup_head(&grantee_a(), &grantor_a())
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_one().as_str())
        );
        assert_eq!(
            index
                .lookup_head(&grantee_b(), &grantor_a())
                .map(|e| e.statement_id.as_str()),
            Some(statement_id_two().as_str())
        );
    }

    #[test]
    fn into_heads_lists_each_triple_head() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = CapabilityByObjectIndexFile::default();
        index.upsert(
            &grantee_a(),
            &grantor_a(),
            &statement_id_one(),
            timestamp(100),
            None,
        );
        index.upsert(
            &grantee_a(),
            &grantor_a(),
            &statement_id_two(),
            timestamp(200),
            Some(&statement_id_one()),
        );
        index.upsert(
            &grantee_b(),
            &grantor_b(),
            &statement_id_three(),
            timestamp(50),
            None,
        );

        let heads = index.into_heads(&object())?;
        assert_eq!(heads.len(), 2);
        let a_head = heads
            .iter()
            .find(|h| h.grantee == grantee_a() && h.grantor == grantor_a())
            .expect("(grantor_a, grantee_a) head present");
        assert_eq!(a_head.statement_id, statement_id_two());
        let b_head = heads
            .iter()
            .find(|h| h.grantee == grantee_b() && h.grantor == grantor_b())
            .expect("(grantor_b, grantee_b) head present");
        assert_eq!(b_head.statement_id, statement_id_three());
        Ok(())
    }
}
