//! Where fetched records came from (`specs/DECISIONS.md` §16
//! sub-decision 8).
//!
//! For every record a federation pull stores or re-sees, the pulling
//! node notes the peer that served it. A record at
//! `<root>/provenance/<XX>/<YY>/<id>.json` (the standard shard
//! scheme, keyed on the record's bare id) holds the record's typed
//! reference and one entry per distinct peer, oldest first: the
//! first entry is where the record was first seen.
//!
//! ```json
//! {
//!   "record": "statement:<id>",
//!   "entries": [
//!     { "peer_url": "https://a.example", "peer_actor": "<actor-id>",
//!       "fetched_at": "<RFC 3339>" }
//!   ]
//! }
//! ```
//!
//! Only the immediate peer is recorded, not the route a relayed record
//! took. Authorship needs no provenance; the signature proves it.
//! `peer_actor` is the node actor the peer's unsigned node document
//! *claimed*; `peer_url` is the fact.
//!
//! ## Primary state, not an index
//!
//! Nothing in `statements/` can reproduce this, so `rebuild_indexes()`
//! must never touch `provenance/`. A record with no provenance was
//! authored or imported locally.

use kairo_core::{ActorId, KairoRef, Timestamp};
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};
use crate::lock::with_index_lock;
use crate::{atomic_write, json_to_corrupt, read_or_missing, FilesystemStore, JSON_SUFFIX};

pub(crate) const PROVENANCE_DIR: &str = "provenance";

/// One peer that served a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceEntry {
    /// Base URL the record was fetched from.
    pub peer_url: String,
    /// The node actor the peer claimed (unauthenticated in the MVP).
    pub peer_actor: ActorId,
    pub fetched_at: Timestamp,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceFile {
    record: String,
    entries: Vec<ProvenanceEntryJson>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceEntryJson {
    peer_url: String,
    peer_actor: String,
    fetched_at: String,
}

pub trait ProvenanceStore {
    /// Note that `entry.peer_url` served `record`. Appends only when
    /// no entry for that `peer_url` exists yet, so re-pulling from the
    /// same peer keeps the first `fetched_at`. Returns whether an
    /// entry was added.
    fn record_provenance(
        &self,
        record: &KairoRef,
        entry: &ProvenanceEntry,
    ) -> Result<bool, StoreError>;

    /// Every peer that served `record`, first-seen first. Empty for a
    /// record that was never fetched.
    fn get_provenance(&self, record: &KairoRef) -> Result<Vec<ProvenanceEntry>, StoreError>;
}

/// The bare id a record's provenance file is sharded on. Snapshot
/// references are not records and have no provenance.
fn record_id(record: &KairoRef) -> Result<&str, StoreError> {
    match record {
        KairoRef::Object(id) => Ok(id.as_str()),
        KairoRef::Statement(id) => Ok(id.as_str()),
        KairoRef::Blob(id) => Ok(id.as_str()),
        KairoRef::Actor(id) => Ok(id.as_str()),
        KairoRef::ObjectSnapshot { .. } => Err(StoreError::Rejected {
            reason: "snapshot references have no provenance".to_owned(),
        }),
    }
}

impl FilesystemStore {
    fn read_provenance_file(
        &self,
        record: &KairoRef,
    ) -> Result<Option<ProvenanceFile>, StoreError> {
        let path = self.shard_path(PROVENANCE_DIR, record_id(record)?, JSON_SUFFIX)?;
        let bytes = match read_or_missing(&path) {
            Ok(bytes) => bytes,
            Err(StoreError::Missing) => return Ok(None),
            Err(error) => return Err(error),
        };
        let file: ProvenanceFile =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(record))?;
        if file.record != record.to_string() {
            return Err(StoreError::Corrupt {
                id: record.to_string(),
                reason: CorruptReason::Parse(format!(
                    "provenance file names {:?}, expected {record}",
                    file.record
                )),
            });
        }
        Ok(Some(file))
    }
}

fn parse_entry(
    record: &KairoRef,
    entry: ProvenanceEntryJson,
) -> Result<ProvenanceEntry, StoreError> {
    let corrupt = |message: String| StoreError::Corrupt {
        id: record.to_string(),
        reason: CorruptReason::Parse(message),
    };
    Ok(ProvenanceEntry {
        peer_actor: ActorId::new(entry.peer_actor).map_err(|e| corrupt(e.to_string()))?,
        fetched_at: entry
            .fetched_at
            .parse()
            .map_err(|e: kairo_core::TimestampError| corrupt(e.to_string()))?,
        peer_url: entry.peer_url,
    })
}

impl ProvenanceStore for FilesystemStore {
    fn record_provenance(
        &self,
        record: &KairoRef,
        entry: &ProvenanceEntry,
    ) -> Result<bool, StoreError> {
        let path = self.shard_path(PROVENANCE_DIR, record_id(record)?, JSON_SUFFIX)?;
        with_index_lock(&path, || {
            let mut file = self
                .read_provenance_file(record)?
                .unwrap_or(ProvenanceFile {
                    record: record.to_string(),
                    entries: Vec::new(),
                });
            if file
                .entries
                .iter()
                .any(|existing| existing.peer_url == entry.peer_url)
            {
                return Ok(false);
            }
            file.entries.push(ProvenanceEntryJson {
                peer_url: entry.peer_url.clone(),
                peer_actor: entry.peer_actor.to_string(),
                fetched_at: entry.fetched_at.to_string(),
            });
            let mut bytes = serde_json::to_vec_pretty(&file).map_err(json_to_corrupt(record))?;
            bytes.push(b'\n');
            atomic_write(&path, &bytes)?;
            Ok(true)
        })
    }

    fn get_provenance(&self, record: &KairoRef) -> Result<Vec<ProvenanceEntry>, StoreError> {
        let Some(file) = self.read_provenance_file(record)? else {
            return Ok(Vec::new());
        };
        file.entries
            .into_iter()
            .map(|entry| parse_entry(record, entry))
            .collect()
    }
}
