//! Per-actor materialized index of `ActorKeyRotation` and
//! `ActorKeyRevocation` statements.
//!
//! Each file at `<root>/actor_keys/<XX>/<YY>/<actor-id>.json` records
//! every known key event for that actor — rotations in a chained list,
//! revocations in a standalone set — materialized from the underlying
//! signed statements stored in `statements/`. The two queries served
//! by this index are:
//!
//! - **Active key at causal position `T`**: walk the rotation chain
//!   leaf with `created_at <= T`, falling back to
//!   `ActorGenesis.initial_key` if no rotation precedes `T`.
//! - **Is `(actor, key_id)` revoked at `T`?**: any revocation matches
//!   if `retroactive = true` or `created_at <= T`. Most-restrictive
//!   wins.
//!
//! Sharding is on `actor_id` because the queries above always start
//! from a known actor; per-actor locality also matches first-person
//! authority (only the actor whose key it is may modify their own key
//! chain — see `STATEMENTS.md` §4.2f / §4.2g).
//!
//! The file holds rotations and revocations side by side because both
//! are produced by the same actor, both feed the same verifier rule
//! (§6.1), and keeping them in one file gives the resolver atomic
//! visibility — a rotation and its accompanying revocation either
//! both persist or neither does.
//!
//! Format (JSON, one file per actor):
//!
//! ```json
//! {
//!   "rotations": [
//!     {
//!       "statement_id": "<statement-id>",
//!       "next_key": { "algorithm": "ed25519", "bytes": "<base64>" },
//!       "created_at": "<RFC 3339>",
//!       "supersedes": "<statement-id>" | null
//!     },
//!     ...
//!   ],
//!   "revocations": [
//!     {
//!       "statement_id": "<statement-id>",
//!       "revoked_key": "<key-id>",
//!       "retroactive": true | false,
//!       "created_at": "<RFC 3339>"
//!     },
//!     ...
//!   ]
//! }
//! ```
//!
//! Multi-process safety is not yet enforced; concurrent writers can
//! race on read-modify-write. File locks land alongside the rest of
//! the multi-process MVP work.

use kairo_core::{StatementId, Timestamp};
use kairo_identity::json::PublicKeyJson;
use kairo_identity::{KeyId, KeyRevocationEntry, KeyRotationEntry, PublicKey};
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};

/// On-disk representation of one rotation entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RotationEntry {
    pub statement_id: String,
    pub next_key: PublicKeyJson,
    pub created_at: String,
    pub supersedes: Option<String>,
}

/// On-disk representation of one revocation entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RevocationEntry {
    pub statement_id: String,
    pub revoked_key: String,
    pub retroactive: bool,
    pub created_at: String,
}

/// On-disk representation of all key events for a single actor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KeyEventIndexFile {
    #[serde(default)]
    pub rotations: Vec<RotationEntry>,
    #[serde(default)]
    pub revocations: Vec<RevocationEntry>,
}

impl KeyEventIndexFile {
    /// Append a rotation entry. Returns `true` if the entry was
    /// actually added; a duplicate `put` is a no-op.
    pub(crate) fn upsert_rotation(
        &mut self,
        statement_id: &StatementId,
        next_key: &PublicKey,
        created_at: Timestamp,
        supersedes: Option<&StatementId>,
    ) -> bool {
        let new_id = statement_id.to_string();
        if self.rotations.iter().any(|e| e.statement_id == new_id) {
            return false;
        }
        self.rotations.push(RotationEntry {
            statement_id: new_id,
            next_key: PublicKeyJson::from_public_key(next_key),
            created_at: created_at.to_string(),
            supersedes: supersedes.map(|s| s.to_string()),
        });
        true
    }

    /// Append a revocation entry. Returns `true` if the entry was
    /// actually added; a duplicate `put` is a no-op.
    pub(crate) fn upsert_revocation(
        &mut self,
        statement_id: &StatementId,
        revoked_key: &KeyId,
        retroactive: bool,
        created_at: Timestamp,
    ) -> bool {
        let new_id = statement_id.to_string();
        if self.revocations.iter().any(|e| e.statement_id == new_id) {
            return false;
        }
        self.revocations.push(RevocationEntry {
            statement_id: new_id,
            revoked_key: revoked_key.to_string(),
            retroactive,
            created_at: created_at.to_string(),
        });
        true
    }

    /// Decode the rotation set into the resolver-facing summary.
    /// Used by `FilesystemStore` to satisfy `ActorResolver::key_rotations`.
    pub(crate) fn decode_rotations(
        &self,
        actor_str: &str,
    ) -> Result<Vec<KeyRotationEntry>, StoreError> {
        let mut out = Vec::with_capacity(self.rotations.len());
        for entry in &self.rotations {
            let next_key = entry
                .next_key
                .to_public_key()
                .map_err(|error| StoreError::Corrupt {
                    id: entry.statement_id.clone(),
                    reason: CorruptReason::Parse(format!(
                        "invalid rotation public key for actor {actor_str}: {error}"
                    )),
                })?;
            let created_at: Timestamp =
                entry.created_at.parse().map_err(|error| StoreError::Corrupt {
                    id: entry.statement_id.clone(),
                    reason: CorruptReason::Parse(format!(
                        "invalid created_at in rotation index: {error}"
                    )),
                })?;
            out.push(KeyRotationEntry {
                statement_id: entry.statement_id.clone(),
                next_key,
                created_at,
                supersedes: entry.supersedes.clone(),
            });
        }
        Ok(out)
    }

    /// Decode the revocation set into the resolver-facing summary.
    pub(crate) fn decode_revocations(
        &self,
    ) -> Result<Vec<KeyRevocationEntry>, StoreError> {
        let mut out = Vec::with_capacity(self.revocations.len());
        for entry in &self.revocations {
            let created_at: Timestamp =
                entry.created_at.parse().map_err(|error| StoreError::Corrupt {
                    id: entry.statement_id.clone(),
                    reason: CorruptReason::Parse(format!(
                        "invalid created_at in revocation index: {error}"
                    )),
                })?;
            out.push(KeyRevocationEntry {
                statement_id: entry.statement_id.clone(),
                revoked_key: KeyId::new(entry.revoked_key.clone()),
                retroactive: entry.retroactive,
                created_at,
            });
        }
        Ok(out)
    }
}

/// Pick the rotation chain leaf considering only entries with
/// `created_at <= at`. Returns `None` if no eligible entry exists.
///
/// Mirrors the trust/branch chain-head pattern: leaves are entries
/// not named by any sibling's `supersedes`; multiple leaves tiebreak
/// on `(created_at, statement_id)` descending.
#[cfg(test)]
pub(crate) fn rotation_chain_head_at(
    rotations: &[RotationEntry],
    at: Timestamp,
) -> Option<&RotationEntry> {
    let eligible: Vec<&RotationEntry> = rotations
        .iter()
        .filter(|e| {
            e.created_at
                .parse::<Timestamp>()
                .map(|ts| ts <= at)
                .unwrap_or(false)
        })
        .collect();
    if eligible.is_empty() {
        return None;
    }
    let superseded: std::collections::HashSet<&str> = eligible
        .iter()
        .filter_map(|e| e.supersedes.as_deref())
        .collect();
    let mut best: Option<&RotationEntry> = None;
    for entry in &eligible {
        if superseded.contains(entry.statement_id.as_str()) {
            continue;
        }
        match best {
            None => best = Some(entry),
            Some(current) if rotation_greater(entry, current) => best = Some(entry),
            _ => {}
        }
    }
    best
}

#[cfg(test)]
fn rotation_greater(candidate: &RotationEntry, current: &RotationEntry) -> bool {
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
        _ => candidate.statement_id > current.statement_id,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use ed25519_dalek::SigningKey;

    use super::*;

    fn statement_id_one() -> StatementId {
        StatementId::from_sha256_digest([0xAA; 32])
    }

    fn statement_id_two() -> StatementId {
        StatementId::from_sha256_digest([0xBB; 32])
    }

    fn statement_id_three() -> StatementId {
        StatementId::from_sha256_digest([0xCC; 32])
    }

    fn key_a() -> PublicKey {
        PublicKey::ed25519(SigningKey::from_bytes(&[7; 32]).verifying_key().to_bytes())
    }

    fn key_b() -> PublicKey {
        PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes())
    }

    fn timestamp(seconds: i64) -> Timestamp {
        Timestamp::from_seconds(seconds)
    }

    #[test]
    fn upsert_rotation_returns_true_only_for_new_statements() {
        let mut index = KeyEventIndexFile::default();
        assert!(index.upsert_rotation(&statement_id_one(), &key_a(), timestamp(100), None));
        assert!(!index.upsert_rotation(&statement_id_one(), &key_a(), timestamp(100), None));
    }

    #[test]
    fn upsert_revocation_returns_true_only_for_new_statements() {
        let mut index = KeyEventIndexFile::default();
        let key_id = key_a().key_id();
        assert!(index.upsert_revocation(&statement_id_one(), &key_id, false, timestamp(100)));
        assert!(!index.upsert_revocation(&statement_id_one(), &key_id, false, timestamp(100)));
    }

    #[test]
    fn rotation_chain_head_picks_leaf_at_or_before_timestamp() {
        let mut index = KeyEventIndexFile::default();
        index.upsert_rotation(&statement_id_one(), &key_a(), timestamp(100), None);
        index.upsert_rotation(
            &statement_id_two(),
            &key_b(),
            timestamp(200),
            Some(&statement_id_one()),
        );

        let head = rotation_chain_head_at(&index.rotations, timestamp(150))
            .expect("entry at t=150");
        assert_eq!(head.statement_id, statement_id_one().to_string());

        let head = rotation_chain_head_at(&index.rotations, timestamp(250))
            .expect("entry at t=250");
        assert_eq!(head.statement_id, statement_id_two().to_string());
    }

    #[test]
    fn rotation_chain_head_returns_none_before_first_rotation() {
        let mut index = KeyEventIndexFile::default();
        index.upsert_rotation(&statement_id_one(), &key_a(), timestamp(100), None);
        assert!(rotation_chain_head_at(&index.rotations, timestamp(50)).is_none());
    }

    #[test]
    fn rotation_fork_tiebreaks_on_created_at_then_statement_id() {
        let mut index = KeyEventIndexFile::default();
        index.upsert_rotation(&statement_id_one(), &key_a(), timestamp(100), None);
        index.upsert_rotation(
            &statement_id_two(),
            &key_b(),
            timestamp(200),
            Some(&statement_id_one()),
        );
        index.upsert_rotation(
            &statement_id_three(),
            &key_a(),
            timestamp(200),
            Some(&statement_id_one()),
        );
        // Both _two and _three supersede _one; tiebreak on lex-greater
        // statement id (_three > _two).
        let head = rotation_chain_head_at(&index.rotations, timestamp(300))
            .expect("entry at t=300");
        assert_eq!(head.statement_id, statement_id_three().to_string());
    }

    #[test]
    fn decode_round_trips_rotation_entries() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = KeyEventIndexFile::default();
        index.upsert_rotation(&statement_id_one(), &key_a(), timestamp(100), None);
        let decoded = index.decode_rotations("dummy-actor")?;
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].next_key, key_a());
        Ok(())
    }

    #[test]
    fn decode_round_trips_revocation_entries() -> Result<(), Box<dyn std::error::Error>> {
        let mut index = KeyEventIndexFile::default();
        let key_id = key_a().key_id();
        index.upsert_revocation(&statement_id_one(), &key_id, true, timestamp(100));
        let decoded = index.decode_revocations()?;
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].revoked_key, key_id);
        assert!(decoded[0].retroactive);
        Ok(())
    }
}
