//! The node's federation publish list
//! (`specs/DECISIONS.md` §16 sub-decision 3).
//!
//! A federation node serves an object only after its operator
//! publishes it. The list lives at
//! `<root>/federation/published.json` and is **local operator
//! state**, not a statement: it is policy about this node, not a
//! claim about the object, so it is never signed, bundled, or
//! federated.
//!
//! ## Not a materialized index
//!
//! Nothing in `statements/` or `objects/` can reproduce this file,
//! so it is primary state: `rebuild_indexes()` must never touch the
//! `federation/` directory. Losing it unpublishes everything, which
//! fails safe.
//!
//! ## On-disk shape
//!
//! One unsharded file (a node publishes a handful of objects, not
//! millions), a JSON array ordered by `object_id`:
//!
//! ```json
//! [
//!   { "object_id": "<object-id>", "published_at": "<RFC 3339>" },
//!   ...
//! ]
//! ```
//!
//! Writers take `lock::with_index_lock` on the file's sidecar, so
//! concurrent `kairo federation publish` runs don't lose entries.
//! The serving process only reads.

use std::path::PathBuf;

use kairo_core::{ObjectId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};
use crate::lock::with_index_lock;
use crate::{atomic_write, json_to_corrupt, read_or_missing, FilesystemStore, ObjectStore};

const FEDERATION_DIR: &str = "federation";
const PUBLISHED_FILE: &str = "published.json";

/// Id used in `Corrupt` errors for this file.
const PUBLISHED_RECORD: &str = "federation/published.json";

/// One published object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedObject {
    pub object_id: ObjectId,
    /// When this node's operator published it. Local bookkeeping
    /// only; peers never see it.
    pub published_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishedEntry {
    object_id: String,
    published_at: String,
}

/// Read and write the node's federation publish list.
pub trait PublishedObjectStore {
    /// Add `object` to the publish list. The object's genesis must
    /// be in the store (`StoreError::Missing` otherwise). Returns
    /// `false` if it was already published, in which case the
    /// original `published_at` is kept.
    fn publish_object(&self, object: &ObjectId, at: Timestamp) -> Result<bool, StoreError>;

    /// Remove `object` from the publish list. Returns `false` if it
    /// wasn't published. This stops serving it; it cannot recall
    /// copies peers already fetched.
    fn unpublish_object(&self, object: &ObjectId) -> Result<bool, StoreError>;

    /// Every published object, ordered by id.
    fn list_published_objects(&self) -> Result<Vec<PublishedObject>, StoreError>;

    fn is_object_published(&self, object: &ObjectId) -> Result<bool, StoreError>;
}

impl FilesystemStore {
    fn published_path(&self) -> PathBuf {
        self.root.join(FEDERATION_DIR).join(PUBLISHED_FILE)
    }

    fn read_published(&self) -> Result<Vec<PublishedObject>, StoreError> {
        let bytes = match read_or_missing(&self.published_path()) {
            Ok(bytes) => bytes,
            Err(StoreError::Missing) => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let entries: Vec<PublishedEntry> =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(&PUBLISHED_RECORD))?;
        entries
            .into_iter()
            .map(|entry| {
                Ok(PublishedObject {
                    object_id: ObjectId::new(&entry.object_id).map_err(|error| corrupt(&error))?,
                    published_at: entry
                        .published_at
                        .parse()
                        .map_err(|error| corrupt(&error))?,
                })
            })
            .collect()
    }

    fn write_published(&self, objects: &[PublishedObject]) -> Result<(), StoreError> {
        let entries: Vec<PublishedEntry> = objects
            .iter()
            .map(|object| PublishedEntry {
                object_id: object.object_id.to_string(),
                published_at: object.published_at.to_string(),
            })
            .collect();
        let mut bytes =
            serde_json::to_vec_pretty(&entries).map_err(json_to_corrupt(&PUBLISHED_RECORD))?;
        bytes.push(b'\n');
        atomic_write(&self.published_path(), &bytes)
    }
}

fn corrupt(error: &dyn std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        id: PUBLISHED_RECORD.to_owned(),
        reason: CorruptReason::Parse(error.to_string()),
    }
}

impl PublishedObjectStore for FilesystemStore {
    fn publish_object(&self, object: &ObjectId, at: Timestamp) -> Result<bool, StoreError> {
        self.get_object_genesis(object)?;
        with_index_lock(&self.published_path(), || {
            let mut objects = self.read_published()?;
            let Err(position) = objects.binary_search_by(|entry| entry.object_id.cmp(object))
            else {
                return Ok(false);
            };
            objects.insert(
                position,
                PublishedObject {
                    object_id: object.clone(),
                    published_at: at,
                },
            );
            self.write_published(&objects)?;
            Ok(true)
        })
    }

    fn unpublish_object(&self, object: &ObjectId) -> Result<bool, StoreError> {
        with_index_lock(&self.published_path(), || {
            let mut objects = self.read_published()?;
            let Ok(position) = objects.binary_search_by(|entry| entry.object_id.cmp(object)) else {
                return Ok(false);
            };
            objects.remove(position);
            self.write_published(&objects)?;
            Ok(true)
        })
    }

    fn list_published_objects(&self) -> Result<Vec<PublishedObject>, StoreError> {
        let mut objects = self.read_published()?;
        // The writer keeps the file sorted; re-sort anyway so a
        // hand-edited file can't break callers' binary searches.
        objects.sort_by(|a, b| a.object_id.cmp(&b.object_id));
        Ok(objects)
    }

    fn is_object_published(&self, object: &ObjectId) -> Result<bool, StoreError> {
        Ok(self
            .read_published()?
            .iter()
            .any(|entry| &entry.object_id == object))
    }
}
