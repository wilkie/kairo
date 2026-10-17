//! The node's federation pull log: the last successful pull from each
//! peer (`specs/PHASE_2_FEDERATION.md` slice 5), for `kairo
//! federation status`.
//!
//! One unsharded file, `<root>/federation/pulls.json`, mapping each
//! peer URL to its most recent successful pull. It is local
//! bookkeeping: primary state that `rebuild_indexes()` leaves alone,
//! and losing it only forgets history. Per-record provenance
//! (`provenance.rs`) is the durable answer to "where did this come
//! from"; this file answers "when did we last talk to that peer".
//!
//! ```json
//! {
//!   "https://archive.example.org": {
//!     "object": "<object-id>",
//!     "peer_actor": "<actor-id>",
//!     "at": "<RFC 3339>",
//!     "fetched": 12,
//!     "already_held": 3
//!   }
//! }
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use kairo_core::{ActorId, ObjectId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};
use crate::lock::with_index_lock;
use crate::{atomic_write, json_to_corrupt, read_or_missing, FilesystemStore};

const PULLS_FILE: &str = "pulls.json";
const PULLS_RECORD: &str = "federation/pulls.json";

/// One successful pull.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRecord {
    pub peer_url: String,
    pub object: ObjectId,
    /// The node actor the peer claimed.
    pub peer_actor: ActorId,
    pub at: Timestamp,
    pub fetched: u64,
    pub already_held: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PullEntry {
    object: String,
    peer_actor: String,
    at: String,
    fetched: u64,
    already_held: u64,
}

pub trait PullLogStore {
    /// Record `pull` as the latest pull from its peer, replacing any
    /// earlier one.
    fn record_pull(&self, pull: &PullRecord) -> Result<(), StoreError>;

    /// The latest pull from every peer, ordered by peer URL.
    fn list_pulls(&self) -> Result<Vec<PullRecord>, StoreError>;
}

impl FilesystemStore {
    fn pulls_path(&self) -> PathBuf {
        self.root.join("federation").join(PULLS_FILE)
    }

    fn read_pulls(&self) -> Result<BTreeMap<String, PullEntry>, StoreError> {
        match read_or_missing(&self.pulls_path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(json_to_corrupt(&PULLS_RECORD)),
            Err(StoreError::Missing) => Ok(BTreeMap::new()),
            Err(error) => Err(error),
        }
    }
}

fn corrupt(error: &dyn std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        id: PULLS_RECORD.to_owned(),
        reason: CorruptReason::Parse(error.to_string()),
    }
}

impl PullLogStore for FilesystemStore {
    fn record_pull(&self, pull: &PullRecord) -> Result<(), StoreError> {
        let path = self.pulls_path();
        with_index_lock(&path, || {
            let mut pulls = self.read_pulls()?;
            pulls.insert(
                pull.peer_url.clone(),
                PullEntry {
                    object: pull.object.to_string(),
                    peer_actor: pull.peer_actor.to_string(),
                    at: pull.at.to_string(),
                    fetched: pull.fetched,
                    already_held: pull.already_held,
                },
            );
            let mut bytes =
                serde_json::to_vec_pretty(&pulls).map_err(json_to_corrupt(&PULLS_RECORD))?;
            bytes.push(b'\n');
            atomic_write(&path, &bytes)
        })
    }

    fn list_pulls(&self) -> Result<Vec<PullRecord>, StoreError> {
        self.read_pulls()?
            .into_iter()
            .map(|(peer_url, entry)| {
                Ok(PullRecord {
                    object: ObjectId::new(&entry.object).map_err(|e| corrupt(&e))?,
                    peer_actor: ActorId::new(&entry.peer_actor).map_err(|e| corrupt(&e))?,
                    at: entry.at.parse().map_err(|e| corrupt(&e))?,
                    fetched: entry.fetched,
                    already_held: entry.already_held,
                    peer_url,
                })
            })
            .collect()
    }
}
