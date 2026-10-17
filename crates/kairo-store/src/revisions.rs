//! Per-object index of `ObjectRevision` statements keyed by the
//! revision they name, and the parent walk built on it
//! (`specs/DECISIONS.md` §20).
//!
//! Each file at `<root>/object_revisions/<XX>/<YY>/<object-id>.json`
//! maps every revision id (`git:sha1:<oid>`, `git:sha256:<oid>`, or
//! another scheme's id) to the `ObjectRevision` statements that name
//! it for that object. Several statements can name one revision,
//! because different actors may each sign the same commit.
//!
//! The index exists because a revision names its parents by revision
//! id, not by statement id. Walking history from a tip — a branch, a
//! tag, or the genesis's initial revision — means asking, at every
//! step, "which statements name this parent?". With it, a log page
//! or a reachable closure costs the history walked, never the size of
//! the store.
//!
//! ```json
//! { "git:sha1:<oid>": ["<statement-id>", "..."], "...": ["..."] }
//! ```
//!
//! A strict materialization of `statements/`: `rebuild_indexes()`
//! reproduces it. Multi-process safety comes from
//! `lock::with_index_lock` on the per-object sidecar.
//!
//! ## Walk order
//!
//! Like `git log`, newest first. The local log starts from every
//! revision the store holds (`git log --all`, plus revisions no ref
//! reaches yet); what federates starts from the refs only. A max-heap keyed by `(created_at,
//! statement_id)` starts from the tips, emits the newest pending
//! revision, and queues its parents' statements. Each statement is
//! emitted once. The order is deterministic for a given store, so an
//! offset is a stable page position until the history changes.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::path::PathBuf;

use kairo_core::{ObjectId, StatementId, Timestamp};
use kairo_statement::{ObjectRevisionBody, RevisionId, SignedStatement};
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};
use crate::lock::with_index_lock;
use crate::{
    atomic_write, json_to_corrupt, read_or_missing, FilesystemStore, ObjectStore, StatementStore,
    JSON_SUFFIX,
};

pub(crate) const OBJECT_REVISIONS_DIR: &str = "object_revisions";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
struct RevisionIndexFile(BTreeMap<String, BTreeSet<String>>);

/// One page of an object's revision log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionLog {
    /// Newest first.
    pub revisions: Vec<SignedStatement<ObjectRevisionBody>>,
    /// Whether the walk continues past this page.
    pub more: bool,
}

fn corrupt(object: &ObjectId, message: String) -> StoreError {
    StoreError::Corrupt {
        id: object.to_string(),
        reason: CorruptReason::Parse(message),
    }
}

impl FilesystemStore {
    fn revision_index_path(&self, object: &ObjectId) -> Result<PathBuf, StoreError> {
        self.shard_path(OBJECT_REVISIONS_DIR, object.as_str(), JSON_SUFFIX)
    }

    fn read_revision_index(&self, object: &ObjectId) -> Result<RevisionIndexFile, StoreError> {
        match read_or_missing(&self.revision_index_path(object)?) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| corrupt(object, format!("invalid revision index: {error}"))),
            Err(StoreError::Missing) => Ok(RevisionIndexFile::default()),
            Err(error) => Err(error),
        }
    }

    /// Record that `statement_id` names `revision` for `object`.
    pub(crate) fn upsert_revision_index(
        &self,
        object: &ObjectId,
        revision: &RevisionId,
        statement_id: &StatementId,
    ) -> Result<(), StoreError> {
        let path = self.revision_index_path(object)?;
        with_index_lock(&path, || {
            let mut index = self.read_revision_index(object)?;
            let inserted = index
                .0
                .entry(revision.as_str().to_owned())
                .or_default()
                .insert(statement_id.to_string());
            if inserted {
                let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(object))?;
                atomic_write(&path, &bytes)?;
            }
            Ok(())
        })
    }

    /// The `ObjectRevision` statements that name `revision` for
    /// `object`, ascending by id. Empty if none do.
    pub fn revision_statements(
        &self,
        object: &ObjectId,
        revision: &RevisionId,
    ) -> Result<Vec<StatementId>, StoreError> {
        let index = self.read_revision_index(object)?;
        let Some(ids) = index.0.get(revision.as_str()) else {
            return Ok(Vec::new());
        };
        ids.iter()
            .map(|id| {
                StatementId::new(id.as_str())
                    .map_err(|error| corrupt(object, format!("invalid id {id:?}: {error}")))
            })
            .collect()
    }

    /// Every revision id some statement names for `object`, ascending.
    /// Cheap (one index file); used to learn an object's Git object
    /// format without walking history.
    pub fn revision_ids(&self, object: &ObjectId) -> Result<Vec<RevisionId>, StoreError> {
        Ok(self
            .read_revision_index(object)?
            .0
            .into_keys()
            .map(RevisionId::new)
            .collect())
    }

    /// The object's refs, as revision statements to walk from: the
    /// targets of every `ObjectBranch` and `ObjectVersionTag` entry
    /// (whole chains, not just heads, so superseded refs keep their
    /// history reachable) and the statements naming the genesis's
    /// initial revision. Only statements that are revisions of
    /// `object` count; a ref pointing elsewhere is ignored. Ascending,
    /// deduplicated.
    pub fn revision_ref_tips(&self, object: &ObjectId) -> Result<Vec<StatementId>, StoreError> {
        let index = self.read_revision_index(object)?;
        let known: BTreeSet<&str> = index.0.values().flatten().map(String::as_str).collect();
        let mut tips: BTreeSet<StatementId> = BTreeSet::new();
        let mut consider = |id: &StatementId| {
            if known.contains(id.as_str()) {
                tips.insert(id.clone());
            }
        };

        let genesis = self.get_object_genesis(object)?;
        if let Some(initial) = genesis.body().initial_revision() {
            for id in index.0.get(initial.as_str()).into_iter().flatten() {
                if let Ok(id) = StatementId::new(id.as_str()) {
                    consider(&id);
                }
            }
        }
        let parse = |raw: &str| {
            StatementId::new(raw)
                .map_err(|error| corrupt(object, format!("invalid id {raw:?}: {error}")))
        };
        if let Some(branches) = self.read_branch_index(object)? {
            for entry in branches
                .by_actor
                .values()
                .flat_map(BTreeMap::values)
                .flatten()
            {
                let statement = self.get_object_branch(&parse(&entry.statement_id)?)?;
                consider(statement.unsigned().body().revision());
            }
        }
        if let Some(tags) = self.read_version_tag_index(object)? {
            for entry in tags.by_actor.values().flat_map(BTreeMap::values).flatten() {
                let statement = self.get_object_version_tag(&parse(&entry.statement_id)?)?;
                if let Some(target) = statement.unsigned().body().target() {
                    consider(target);
                }
            }
        }
        Ok(tips.into_iter().collect())
    }

    /// Every `ObjectRevision` statement this store holds for `object`,
    /// ascending by id: the default starting points of the local log,
    /// so revisions no ref reaches yet (one just signed, before a
    /// branch moves to it) still show. Federation uses
    /// [`Self::revision_ref_tips`] instead.
    pub fn revision_statement_ids(
        &self,
        object: &ObjectId,
    ) -> Result<Vec<StatementId>, StoreError> {
        let index = self.read_revision_index(object)?;
        let ids: BTreeSet<&String> = index.0.values().flatten().collect();
        ids.into_iter()
            .map(|id| {
                StatementId::new(id.as_str())
                    .map_err(|error| corrupt(object, format!("invalid id {id:?}: {error}")))
            })
            .collect()
    }

    /// Walk `object`'s history from `tips` along parent links, newest
    /// first, skipping the first `skip` revisions and returning up to
    /// `limit`. Tips that aren't revisions of `object` are ignored.
    pub fn revision_log(
        &self,
        object: &ObjectId,
        tips: &[StatementId],
        skip: usize,
        limit: usize,
    ) -> Result<RevisionLog, StoreError> {
        let mut revisions = Vec::new();
        let mut seen = 0usize;
        let mut more = false;
        self.walk_revisions(object, tips, |revision| {
            if seen < skip {
                seen += 1;
                return true;
            }
            if revisions.len() == limit {
                more = true;
                return false;
            }
            revisions.push(revision);
            true
        })?;
        Ok(RevisionLog { revisions, more })
    }

    /// Every revision reachable from `object`'s refs
    /// ([`Self::revision_ref_tips`]), newest first: what a peer or a
    /// bundle receives (`specs/DECISIONS.md` §20). A revision no ref
    /// reaches stays local, like a dangling Git commit.
    pub fn reachable_revisions(
        &self,
        object: &ObjectId,
    ) -> Result<Vec<SignedStatement<ObjectRevisionBody>>, StoreError> {
        let tips = self.revision_ref_tips(object)?;
        let mut revisions = Vec::new();
        self.walk_revisions(object, &tips, |revision| {
            revisions.push(revision);
            true
        })?;
        Ok(revisions)
    }

    /// The walk behind [`Self::revision_log`] and
    /// [`Self::reachable_revisions`]. `visit` returns `false` to stop.
    fn walk_revisions(
        &self,
        object: &ObjectId,
        tips: &[StatementId],
        mut visit: impl FnMut(SignedStatement<ObjectRevisionBody>) -> bool,
    ) -> Result<(), StoreError> {
        let index = self.read_revision_index(object)?;
        let known: BTreeSet<&str> = index.0.values().flatten().map(String::as_str).collect();
        let mut queue = Queue {
            queued: BTreeSet::new(),
            heap: BinaryHeap::new(),
            pending: BTreeMap::new(),
        };
        let enqueue = |id: StatementId, queue: &mut Queue| -> Result<(), StoreError> {
            if !known.contains(id.as_str()) || !queue.queued.insert(id.clone()) {
                return Ok(());
            }
            let statement = self.get_object_revision(&id)?;
            if statement.unsigned().body().object() != object {
                return Ok(());
            }
            queue
                .heap
                .push((statement.unsigned().created_at(), id.clone()));
            queue.pending.insert(id, statement);
            Ok(())
        };

        for tip in tips {
            enqueue(tip.clone(), &mut queue)?;
        }
        while let Some((_, id)) = queue.heap.pop() {
            let pending = &mut queue.pending;
            let Some(statement) = pending.remove(&id) else {
                continue;
            };
            let parents: Vec<RevisionId> = statement.unsigned().body().parents().to_vec();
            if !visit(statement) {
                return Ok(());
            }
            for parent in &parents {
                for parent_id in index.0.get(parent.as_str()).into_iter().flatten() {
                    let parent_id = StatementId::new(parent_id.as_str()).map_err(|error| {
                        corrupt(object, format!("invalid id {parent_id:?}: {error}"))
                    })?;
                    enqueue(parent_id, &mut queue)?;
                }
            }
        }
        Ok(())
    }
}

/// Walk state: what's been queued, the newest-first frontier, and the
/// loaded statements waiting in it.
struct Queue {
    queued: BTreeSet<StatementId>,
    heap: BinaryHeap<(Timestamp, StatementId)>,
    pending: BTreeMap<StatementId, SignedStatement<ObjectRevisionBody>>,
}
