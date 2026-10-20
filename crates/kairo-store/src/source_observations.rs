//! Per-source materialized index of `SourceObservation` statements
//! (`specs/DECISIONS.md` §37 sub-decision 2).
//!
//! Each file at `<root>/source_observations/<XX>/<YY>/<source-id>.json`
//! lists every known observation of that source, grouped by observer.
//! Sharding on the source makes "what has been seen of this
//! repository, by whom?" one read. Observations are never superseded:
//! each records one fetch, and together they are the source's ref
//! history, so nothing here is ever removed (sub-decision 3).
//!
//! Like the other indices it is a strict materialization of the
//! statements under `statements/` and is rebuilt by
//! `rebuild_indexes()`.
//!
//! Format (JSON, one file per source):
//!
//! ```json
//! {
//!   "<actor-id>": [
//!     { "statement_id": "<statement-id>", "created_at": "<RFC 3339>" }
//!   ]
//! }
//! ```

use std::collections::BTreeMap;

use kairo_core::{ActorId, ObjectId, StatementId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ObservationEntry {
    pub statement_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct ObservationIndexFile {
    pub by_actor: BTreeMap<String, Vec<ObservationEntry>>,
}

impl ObservationIndexFile {
    /// Append an entry; `true` if it was new.
    pub(crate) fn upsert(
        &mut self,
        actor: &ActorId,
        statement_id: &StatementId,
        created_at: Timestamp,
    ) -> bool {
        let entries = self.by_actor.entry(actor.to_string()).or_default();
        if entries
            .iter()
            .any(|entry| entry.statement_id == statement_id.as_str())
        {
            return false;
        }
        entries.push(ObservationEntry {
            statement_id: statement_id.to_string(),
            created_at: created_at.to_string(),
        });
        true
    }

    pub(crate) fn heads(&self, source: &ObjectId) -> Result<Vec<ObservationHead>, StoreError> {
        let corrupt = |what: &str, error: String| StoreError::Corrupt {
            id: source.to_string(),
            reason: CorruptReason::Parse(format!("source observation index {what}: {error}")),
        };
        let mut heads = Vec::new();
        for (actor, entries) in &self.by_actor {
            let actor = ActorId::new(actor.clone()).map_err(|e| corrupt("actor", e.to_string()))?;
            for entry in entries {
                heads.push(ObservationHead {
                    actor: actor.clone(),
                    source: source.clone(),
                    statement_id: StatementId::new(entry.statement_id.clone())
                        .map_err(|e| corrupt("statement_id", e.to_string()))?,
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

/// One known observation of a source, newest first when listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationHead {
    pub actor: ActorId,
    pub source: ObjectId,
    pub statement_id: StatementId,
    pub created_at: Timestamp,
}
