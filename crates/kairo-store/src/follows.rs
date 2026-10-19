//! Follows (`specs/DECISIONS.md` §28): what this node keeps current by
//! background sync, and each follow's last result.
//!
//! One unsharded file, `<root>/federation/follows.json`:
//!
//! ```json
//! [
//!   {
//!     "follow": { "object": "<object-id>" },
//!     "added_at": "<RFC 3339>",
//!     "last_attempt": {
//!       "at": "<RFC 3339>",
//!       "peers": [
//!         { "peer_url": "https://a.example", "outcome": "fetched",
//!           "fetched": 3, "already_held": 9, "incomplete_chains": 0,
//!           "flagged": 0 },
//!         { "peer_url": "https://b.example", "outcome": "not_published" },
//!         { "peer_url": "https://c.example", "outcome": "failed",
//!           "reason": "..." }
//!       ]
//!     },
//!     "last_success": "<RFC 3339>"
//!   },
//!   { "follow": { "trust_by": "<actor-id>", "scope": "federation:index" }, ... }
//! ]
//! ```
//!
//! Primary state, like the publish list: `rebuild_indexes()` leaves it
//! alone.

use std::path::PathBuf;

use kairo_core::{ActorId, ObjectId, Timestamp};
use kairo_statement::TrustScope;
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};
use crate::lock::with_index_lock;
use crate::{atomic_write, json_to_corrupt, read_or_missing, FilesystemStore};

const FOLLOWS_RECORD: &str = "federation/follows.json";

/// Something this node keeps current.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Follow {
    Object(ObjectId),
    /// One (truster, scope) trust chain (§27).
    Trust {
        by: ActorId,
        scope: TrustScope,
    },
}

impl std::fmt::Display for Follow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Object(object) => write!(f, "object:{object}"),
            Self::Trust { by, scope } => write!(f, "trust:{by}:{scope}"),
        }
    }
}

/// What one peer did for one sync of one follow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum PeerOutcome {
    Fetched {
        peer_url: String,
        fetched: u64,
        already_held: u64,
        incomplete_chains: u64,
        flagged: u64,
    },
    /// The peer answered `404 fed_not_found`: it doesn't publish this.
    NotPublished {
        peer_url: String,
    },
    Failed {
        peer_url: String,
        reason: String,
    },
}

impl PeerOutcome {
    pub fn peer_url(&self) -> &str {
        match self {
            Self::Fetched { peer_url, .. }
            | Self::NotPublished { peer_url }
            | Self::Failed { peer_url, .. } => peer_url,
        }
    }

    pub fn succeeded(&self) -> bool {
        matches!(self, Self::Fetched { .. })
    }
}

/// One sync of one follow, over every peer tried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncAttempt {
    pub at: Timestamp,
    pub peers: Vec<PeerOutcome>,
}

impl SyncAttempt {
    /// Some peer served it.
    pub fn succeeded(&self) -> bool {
        self.peers.iter().any(PeerOutcome::succeeded)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowRecord {
    pub follow: Follow,
    pub added_at: Timestamp,
    pub last_attempt: Option<SyncAttempt>,
    pub last_success: Option<Timestamp>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum FollowJson {
    Object { object: String },
    Trust { trust_by: String, scope: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttemptJson {
    at: String,
    peers: Vec<PeerOutcome>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FollowEntry {
    follow: FollowJson,
    added_at: String,
    #[serde(default)]
    last_attempt: Option<AttemptJson>,
    #[serde(default)]
    last_success: Option<String>,
}

pub trait FollowStore {
    /// `false` if already followed.
    fn add_follow(&self, follow: &Follow, at: Timestamp) -> Result<bool, StoreError>;

    /// `false` if it wasn't followed.
    fn remove_follow(&self, follow: &Follow) -> Result<bool, StoreError>;

    /// Remove `by`'s trust follows, in `scope` or all; how many.
    fn remove_trust_follows(
        &self,
        by: &ActorId,
        scope: Option<&TrustScope>,
    ) -> Result<usize, StoreError>;

    /// In the order added.
    fn list_follows(&self) -> Result<Vec<FollowRecord>, StoreError>;

    /// Record a sync of `follow`; a successful one also sets
    /// `last_success`. A follow removed meanwhile is left removed.
    fn record_sync(&self, follow: &Follow, attempt: &SyncAttempt) -> Result<(), StoreError>;
}

fn corrupt(error: &dyn std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        id: FOLLOWS_RECORD.to_owned(),
        reason: CorruptReason::Parse(error.to_string()),
    }
}

fn to_json(follow: &Follow) -> FollowJson {
    match follow {
        Follow::Object(object) => FollowJson::Object {
            object: object.to_string(),
        },
        Follow::Trust { by, scope } => FollowJson::Trust {
            trust_by: by.to_string(),
            scope: scope.to_string(),
        },
    }
}

fn from_json(json: &FollowJson) -> Result<Follow, StoreError> {
    Ok(match json {
        FollowJson::Object { object } => {
            Follow::Object(ObjectId::new(object).map_err(|e| corrupt(&e))?)
        }
        FollowJson::Trust { trust_by, scope } => Follow::Trust {
            by: ActorId::new(trust_by).map_err(|e| corrupt(&e))?,
            scope: TrustScope::parse(scope).map_err(|e| corrupt(&e))?,
        },
    })
}

impl FilesystemStore {
    fn follows_path(&self) -> PathBuf {
        self.root.join("federation").join("follows.json")
    }

    fn read_follows(&self) -> Result<Vec<FollowEntry>, StoreError> {
        match read_or_missing(&self.follows_path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(json_to_corrupt(&FOLLOWS_RECORD)),
            Err(StoreError::Missing) => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    fn update_follows<T>(
        &self,
        change: impl FnOnce(&mut Vec<FollowEntry>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let path = self.follows_path();
        with_index_lock(&path, || {
            let mut entries = self.read_follows()?;
            let result = change(&mut entries)?;
            let mut bytes =
                serde_json::to_vec_pretty(&entries).map_err(json_to_corrupt(&FOLLOWS_RECORD))?;
            bytes.push(b'\n');
            atomic_write(&path, &bytes)?;
            Ok(result)
        })
    }
}

/// Whether `entry` is `follow`, comparing parsed values.
fn is(entry: &FollowEntry, follow: &Follow) -> Result<bool, StoreError> {
    Ok(&from_json(&entry.follow)? == follow)
}

impl FollowStore for FilesystemStore {
    fn add_follow(&self, follow: &Follow, at: Timestamp) -> Result<bool, StoreError> {
        self.update_follows(|entries| {
            for entry in entries.iter() {
                if is(entry, follow)? {
                    return Ok(false);
                }
            }
            entries.push(FollowEntry {
                follow: to_json(follow),
                added_at: at.to_string(),
                last_attempt: None,
                last_success: None,
            });
            Ok(true)
        })
    }

    fn remove_follow(&self, follow: &Follow) -> Result<bool, StoreError> {
        self.update_follows(|entries| {
            let before = entries.len();
            let mut kept = Vec::with_capacity(before);
            for entry in entries.drain(..) {
                if !is(&entry, follow)? {
                    kept.push(entry);
                }
            }
            *entries = kept;
            Ok(entries.len() < before)
        })
    }

    fn remove_trust_follows(
        &self,
        by: &ActorId,
        scope: Option<&TrustScope>,
    ) -> Result<usize, StoreError> {
        self.update_follows(|entries| {
            let before = entries.len();
            let mut kept = Vec::with_capacity(before);
            for entry in entries.drain(..) {
                let drop = match from_json(&entry.follow)? {
                    Follow::Trust {
                        by: entry_by,
                        scope: entry_scope,
                    } => &entry_by == by && scope.is_none_or(|scope| scope == &entry_scope),
                    Follow::Object(_) => false,
                };
                if !drop {
                    kept.push(entry);
                }
            }
            *entries = kept;
            Ok(before - entries.len())
        })
    }

    fn list_follows(&self) -> Result<Vec<FollowRecord>, StoreError> {
        self.read_follows()?
            .into_iter()
            .map(|entry| {
                Ok(FollowRecord {
                    follow: from_json(&entry.follow)?,
                    added_at: entry.added_at.parse().map_err(|e| corrupt(&e))?,
                    last_attempt: entry
                        .last_attempt
                        .map(|attempt| {
                            Ok::<_, StoreError>(SyncAttempt {
                                at: attempt.at.parse().map_err(|e| corrupt(&e))?,
                                peers: attempt.peers,
                            })
                        })
                        .transpose()?,
                    last_success: entry
                        .last_success
                        .map(|at| at.parse().map_err(|e| corrupt(&e)))
                        .transpose()?,
                })
            })
            .collect()
    }

    fn record_sync(&self, follow: &Follow, attempt: &SyncAttempt) -> Result<(), StoreError> {
        self.update_follows(|entries| {
            for entry in entries.iter_mut() {
                if is(entry, follow)? {
                    entry.last_attempt = Some(AttemptJson {
                        at: attempt.at.to_string(),
                        peers: attempt.peers.clone(),
                    });
                    if attempt.succeeded() {
                        entry.last_success = Some(attempt.at.to_string());
                    }
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, FilesystemStore) {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let store = FilesystemStore::open(dir.path()).expect("store");
        (dir, store)
    }

    #[test]
    fn follows_are_added_once_listed_in_order_and_removed() {
        let (_dir, store) = store();
        let object = Follow::Object(ObjectId::from_sha256_digest([1; 32]));
        let by = ActorId::from_sha256_digest([2; 32]);
        let index = Follow::Trust {
            by: by.clone(),
            scope: TrustScope::federation_index(),
        };
        let push = Follow::Trust {
            by: by.clone(),
            scope: TrustScope::federation_push(),
        };
        let at = Timestamp::from_seconds(1_000);
        assert!(store.add_follow(&object, at).expect("add"));
        assert!(!store.add_follow(&object, at).expect("again"));
        assert!(store.add_follow(&index, at).expect("add"));
        assert!(store.add_follow(&push, at).expect("add"));
        let listed: Vec<Follow> = store
            .list_follows()
            .expect("list")
            .into_iter()
            .map(|record| record.follow)
            .collect();
        assert_eq!(listed, vec![object.clone(), index.clone(), push.clone()]);

        assert_eq!(
            store
                .remove_trust_follows(&by, Some(&TrustScope::federation_index()))
                .expect("remove"),
            1
        );
        assert_eq!(store.remove_trust_follows(&by, None).expect("remove"), 1);
        assert!(store.remove_follow(&object).expect("remove"));
        assert!(!store.remove_follow(&object).expect("gone"));
        assert!(store.list_follows().expect("list").is_empty());
    }

    #[test]
    fn a_sync_result_is_kept_and_success_only_moves_forward() {
        let (_dir, store) = store();
        let object = Follow::Object(ObjectId::from_sha256_digest([1; 32]));
        store
            .add_follow(&object, Timestamp::from_seconds(1_000))
            .expect("add");
        let good = SyncAttempt {
            at: Timestamp::from_seconds(2_000),
            peers: vec![
                PeerOutcome::Fetched {
                    peer_url: "https://a.example".to_owned(),
                    fetched: 3,
                    already_held: 1,
                    incomplete_chains: 0,
                    flagged: 0,
                },
                PeerOutcome::NotPublished {
                    peer_url: "https://b.example".to_owned(),
                },
            ],
        };
        store.record_sync(&object, &good).expect("record");
        let bad = SyncAttempt {
            at: Timestamp::from_seconds(3_000),
            peers: vec![PeerOutcome::Failed {
                peer_url: "https://a.example".to_owned(),
                reason: "down".to_owned(),
            }],
        };
        store.record_sync(&object, &bad).expect("record");
        let record = store.list_follows().expect("list").remove(0);
        assert_eq!(record.last_attempt, Some(bad));
        assert_eq!(record.last_success, Some(Timestamp::from_seconds(2_000)));
    }
}
