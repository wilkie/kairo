//! Per-object materialized index of `BuildResult` statements.
//!
//! Each file at `<root>/build_results/<XX>/<YY>/<object-id>.json`
//! lists every known `BuildResult` about that object, one entry per
//! statement, grouped by signer. Sharding on the object makes the
//! resolver's question — "is there a successful result for
//! `(object, snapshot, target)`?" — one read; who signed it is the
//! caller's policy (`DECISIONS.md` §34 sub-decision 7). Results have no
//! supersedes chain: each is one observation, and several for the
//! same plan are reproduction evidence, not a conflict.
//!
//! Like the other indices it is a strict materialization of the
//! statements under `statements/` and is rebuilt by
//! `rebuild_indexes()`.
//!
//! Format (JSON, one file per object):
//!
//! ```json
//! {
//!   "<actor-id>": [
//!     {
//!       "statement_id": "<statement-id>",
//!       "snapshot": "<snapshot-id>",
//!       "target": "release",
//!       "resolved_manifest": "<blob-id>",
//!       "result": "success",
//!       "artifact": "<blob-id>" | null,
//!       "host_dependent": true,
//!       "created_at": "<RFC 3339>"
//!     }
//!   ]
//! }
//! ```

use std::collections::BTreeMap;

use kairo_core::{ActorId, BlobId, ObjectId, SnapshotId, StatementId, Timestamp};
use kairo_statement::{BuildOutcome, BuildResultBody};
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BuildResultEntry {
    pub statement_id: String,
    pub snapshot: String,
    pub target: String,
    pub resolved_manifest: String,
    pub result: String,
    pub artifact: Option<String>,
    pub host_dependent: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct BuildResultIndexFile {
    pub by_actor: BTreeMap<String, Vec<BuildResultEntry>>,
}

impl BuildResultIndexFile {
    /// Append an entry; `true` if it was new.
    pub(crate) fn upsert(
        &mut self,
        actor: &ActorId,
        statement_id: &StatementId,
        body: &BuildResultBody,
        created_at: Timestamp,
    ) -> bool {
        let entries = self.by_actor.entry(actor.to_string()).or_default();
        if entries
            .iter()
            .any(|entry| entry.statement_id == statement_id.as_str())
        {
            return false;
        }
        entries.push(BuildResultEntry {
            statement_id: statement_id.to_string(),
            snapshot: body.snapshot().to_string(),
            target: body.target().to_owned(),
            resolved_manifest: body.resolved_manifest().to_string(),
            result: body.result().as_str().to_owned(),
            artifact: body.artifact().map(ToString::to_string),
            host_dependent: body.host_dependent(),
            created_at: created_at.to_string(),
        });
        true
    }

    pub(crate) fn heads(&self, object: &ObjectId) -> Result<Vec<BuildResultHead>, StoreError> {
        let corrupt = |what: &str, error: String| StoreError::Corrupt {
            id: object.to_string(),
            reason: CorruptReason::Parse(format!("build result index {what}: {error}")),
        };
        let mut heads = Vec::new();
        for (actor, entries) in &self.by_actor {
            let actor = ActorId::new(actor.clone()).map_err(|e| corrupt("actor", e.to_string()))?;
            for entry in entries {
                heads.push(BuildResultHead {
                    actor: actor.clone(),
                    object: object.clone(),
                    statement_id: StatementId::new(entry.statement_id.clone())
                        .map_err(|e| corrupt("statement_id", e.to_string()))?,
                    snapshot: SnapshotId::new(entry.snapshot.clone())
                        .map_err(|e| corrupt("snapshot", e.to_string()))?,
                    target: entry.target.clone(),
                    resolved_manifest: BlobId::new(entry.resolved_manifest.clone())
                        .map_err(|e| corrupt("resolved_manifest", e.to_string()))?,
                    result: BuildOutcome::parse(&entry.result)
                        .map_err(|e| corrupt("result", e.to_string()))?,
                    artifact: entry
                        .artifact
                        .clone()
                        .map(BlobId::new)
                        .transpose()
                        .map_err(|e| corrupt("artifact", e.to_string()))?,
                    host_dependent: entry.host_dependent,
                    created_at: entry.created_at.parse().map_err(
                        |e: kairo_core::TimestampError| corrupt("created_at", e.to_string()),
                    )?,
                });
            }
        }
        heads.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.statement_id.cmp(&b.statement_id))
        });
        Ok(heads)
    }
}

/// One known `BuildResult` about an object, newest first when listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildResultHead {
    pub actor: ActorId,
    pub object: ObjectId,
    pub statement_id: StatementId,
    pub snapshot: SnapshotId,
    pub target: String,
    pub resolved_manifest: BlobId,
    pub result: BuildOutcome,
    pub artifact: Option<BlobId>,
    pub host_dependent: bool,
    pub created_at: Timestamp,
}
