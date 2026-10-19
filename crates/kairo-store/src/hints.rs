//! Discovery hints (`specs/DECISIONS.md` §29 sub-decision 5): the
//! newest verified inventory per node, with who it was learned from.
//! Hints are state, never permissions.
//!
//! One unsharded file, `<root>/federation/hints.json`, keyed by node
//! actor:
//!
//! ```json
//! {
//!   "<node-actor-id>": {
//!     "base_url": "https://archive.example.org",
//!     "objects": ["<object-id>"],
//!     "signed_at": "<RFC 3339>",
//!     "learned_from": "https://peer.example.org",
//!     "learned_via": null,
//!     "learned_at": "<RFC 3339>",
//!     "inventory": "<the signed inventory JSON, verbatim>"
//!   }
//! }
//! ```
//!
//! `learned_via` is the relaying node, or `null` when the node's own
//! inventory was fetched from it. The verbatim inventory is kept so it
//! can be relayed with its signature intact. Primary state:
//! `rebuild_indexes()` leaves it alone.

use std::collections::BTreeMap;
use std::path::PathBuf;

use kairo_core::{ActorId, ObjectId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};
use crate::lock::with_index_lock;
use crate::{atomic_write, json_to_corrupt, read_or_missing, FilesystemStore};

const HINTS_RECORD: &str = "federation/hints.json";

/// One node's newest verified inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub node_actor: ActorId,
    pub base_url: String,
    pub objects: Vec<ObjectId>,
    pub signed_at: Timestamp,
    /// The URL it was fetched from: the node itself, or a relay.
    pub learned_from: String,
    /// The relaying node, if it came by relay.
    pub learned_via: Option<ActorId>,
    pub learned_at: Timestamp,
    /// The signed inventory JSON, verbatim.
    pub inventory: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HintEntry {
    base_url: String,
    objects: Vec<String>,
    signed_at: String,
    learned_from: String,
    learned_via: Option<String>,
    learned_at: String,
    inventory: String,
}

/// What [`HintStore::put_hint`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintPut {
    /// Newer than what was held, or the first for this node.
    Stored,
    /// Not newer than the held hint; kept the held one.
    Older,
}

pub trait HintStore {
    /// Keep `hint` if it's newer (by `signed_at`) than the one held for
    /// its node. Hints with `signed_at` before `expired_before` are
    /// dropped on the way, the new one included.
    fn put_hint(&self, hint: &Hint, expired_before: Timestamp) -> Result<HintPut, StoreError>;

    /// Hints signed at or after `expired_before`, by node actor.
    fn list_hints(&self, expired_before: Timestamp) -> Result<Vec<Hint>, StoreError>;
}

fn corrupt(error: &dyn std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        id: HINTS_RECORD.to_owned(),
        reason: CorruptReason::Parse(error.to_string()),
    }
}

fn to_hint(node: &str, entry: &HintEntry) -> Result<Hint, StoreError> {
    Ok(Hint {
        node_actor: ActorId::new(node).map_err(|e| corrupt(&e))?,
        base_url: entry.base_url.clone(),
        objects: entry
            .objects
            .iter()
            .map(ObjectId::new)
            .collect::<Result<_, _>>()
            .map_err(|e| corrupt(&e))?,
        signed_at: entry.signed_at.parse().map_err(|e| corrupt(&e))?,
        learned_from: entry.learned_from.clone(),
        learned_via: entry
            .learned_via
            .as_deref()
            .map(ActorId::new)
            .transpose()
            .map_err(|e| corrupt(&e))?,
        learned_at: entry.learned_at.parse().map_err(|e| corrupt(&e))?,
        inventory: entry.inventory.clone(),
    })
}

impl FilesystemStore {
    fn hints_path(&self) -> PathBuf {
        self.root.join("federation").join("hints.json")
    }

    fn read_hints(&self) -> Result<BTreeMap<String, HintEntry>, StoreError> {
        match read_or_missing(&self.hints_path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(json_to_corrupt(&HINTS_RECORD)),
            Err(StoreError::Missing) => Ok(BTreeMap::new()),
            Err(error) => Err(error),
        }
    }
}

impl HintStore for FilesystemStore {
    fn put_hint(&self, hint: &Hint, expired_before: Timestamp) -> Result<HintPut, StoreError> {
        let path = self.hints_path();
        with_index_lock(&path, || {
            let mut hints = self.read_hints()?;
            hints.retain(|_, entry| {
                entry
                    .signed_at
                    .parse::<Timestamp>()
                    .is_ok_and(|at| at >= expired_before)
            });
            let key = hint.node_actor.to_string();
            let newer = match hints.get(&key) {
                Some(held) => held
                    .signed_at
                    .parse::<Timestamp>()
                    .map_or(true, |held_at| hint.signed_at > held_at),
                None => true,
            };
            let fresh = hint.signed_at >= expired_before;
            if newer && fresh {
                hints.insert(
                    key,
                    HintEntry {
                        base_url: hint.base_url.clone(),
                        objects: hint.objects.iter().map(ToString::to_string).collect(),
                        signed_at: hint.signed_at.to_string(),
                        learned_from: hint.learned_from.clone(),
                        learned_via: hint.learned_via.as_ref().map(ToString::to_string),
                        learned_at: hint.learned_at.to_string(),
                        inventory: hint.inventory.clone(),
                    },
                );
            }
            let mut bytes =
                serde_json::to_vec_pretty(&hints).map_err(json_to_corrupt(&HINTS_RECORD))?;
            bytes.push(b'\n');
            atomic_write(&path, &bytes)?;
            Ok(if newer && fresh {
                HintPut::Stored
            } else {
                HintPut::Older
            })
        })
    }

    fn list_hints(&self, expired_before: Timestamp) -> Result<Vec<Hint>, StoreError> {
        self.read_hints()?
            .iter()
            .map(|(node, entry)| to_hint(node, entry))
            .filter(|hint| {
                hint.as_ref()
                    .map_or(true, |hint| hint.signed_at >= expired_before)
            })
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn hint(node: u8, signed: i64, objects: &[u8]) -> Hint {
        Hint {
            node_actor: ActorId::from_sha256_digest([node; 32]),
            base_url: format!("https://node{node}.example.org"),
            objects: objects
                .iter()
                .map(|tag| ObjectId::from_sha256_digest([*tag; 32]))
                .collect(),
            signed_at: Timestamp::from_seconds(signed),
            learned_from: format!("https://node{node}.example.org"),
            learned_via: None,
            learned_at: Timestamp::from_seconds(signed + 1),
            inventory: "{}".to_owned(),
        }
    }

    #[test]
    fn the_newest_hint_per_node_wins_and_old_ones_expire() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let store = FilesystemStore::open(dir.path()).expect("store");
        let long_ago = Timestamp::from_seconds(0);
        assert_eq!(
            store.put_hint(&hint(1, 100, &[1]), long_ago).expect("put"),
            HintPut::Stored
        );
        assert_eq!(
            store.put_hint(&hint(1, 90, &[2]), long_ago).expect("put"),
            HintPut::Older
        );
        assert_eq!(
            store.put_hint(&hint(1, 200, &[3]), long_ago).expect("put"),
            HintPut::Stored
        );
        store.put_hint(&hint(2, 150, &[1]), long_ago).expect("put");
        let hints = store.list_hints(long_ago).expect("list");
        assert_eq!(hints.len(), 2);
        assert_eq!(
            hints
                .iter()
                .find(|h| h.signed_at.seconds() == 200)
                .map(|h| h.objects.len()),
            Some(1)
        );

        // Past the TTL: listed no more, and dropped by the next write.
        let cutoff = Timestamp::from_seconds(160);
        let fresh = store.list_hints(cutoff).expect("list");
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].signed_at.seconds(), 200);
        assert_eq!(
            store.put_hint(&hint(3, 100, &[1]), cutoff).expect("put"),
            HintPut::Older
        );
        assert_eq!(store.list_hints(long_ago).expect("list").len(), 1);
    }
}
