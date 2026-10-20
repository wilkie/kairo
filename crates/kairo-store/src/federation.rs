//! Read-side helpers a federation node serves from
//! (`specs/DECISIONS.md` §16, `specs/PHASE_2_FEDERATION.md` slice 2).
//!
//! [`FilesystemStore::object_closure`] computes the id set a peer
//! needs to verify one object (§16 sub-decision 4). It lives here,
//! not in the federation crate, because it is built from the
//! store's private per-object indexes:
//!
//! - **statements**: every `ObjectBranch` and `ObjectVersionTag` entry
//!   in the per-object branch and tag indexes (whole chains, not just
//!   heads: a peer resolves heads itself), plus every `ObjectRevision`
//!   *reachable* from those refs and the genesis's initial revision
//!   along parent links ([`FilesystemStore::reachable_revisions`]).
//!   As in Git, a revision no ref reaches stays local
//!   (`specs/DECISIONS.md` §20).
//! - **authority**: every entry in the per-object capability reverse
//!   index (`actor_capability_by_object`), plus every revocation of
//!   those grants from each grantor's index.
//! - **actors**: the genesis creator and every signer of the above.
//!   Only ids: an actor's key events are fetched per actor through
//!   [`FilesystemStore::actor_key_event_ids`], because key knowledge
//!   belongs to the actor, not the object.
//! - **blobs**: every reachable revision's manifest blob.
//! - **commits**: the Git commit each reachable `git:` revision names:
//!   what the node's Git endpoint may serve (`specs/DECISIONS.md` §22).
//!
//! `ActorTrust` never appears: nothing here reads the trust index.

use crate::BuildResultResolver;
use std::collections::BTreeSet;

use kairo_core::{ActorId, BlobId, ObjectId, StatementId};
use kairo_statement::GitRevision;

use crate::error::{CorruptReason, StoreError};
use crate::{read_or_missing, FilesystemStore, ObjectStore, JSON_SUFFIX, OBJECTS_DIR};

/// The ids in one object's published closure, each set ascending.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectClosure {
    /// Object-bound envelope statements. `ObjectGenesis` is not a
    /// statement id; it is fetched by object id.
    pub statements: BTreeSet<StatementId>,
    /// Object-scoped capability grants and their revocations.
    pub authority: BTreeSet<StatementId>,
    pub actors: BTreeSet<ActorId>,
    pub blobs: BTreeSet<BlobId>,
    /// Git commits named by `statements`' revisions. Not record ids:
    /// the Git endpoint serves these and nothing else.
    pub commits: BTreeSet<GitRevision>,
}

fn parse_index_id<T>(
    raw: &str,
    owner: &impl ToString,
    parse: impl FnOnce(String) -> Result<T, kairo_core::IdError>,
) -> Result<T, StoreError> {
    parse(raw.to_owned()).map_err(|error| StoreError::Corrupt {
        id: owner.to_string(),
        reason: CorruptReason::Parse(format!("invalid id {raw:?} in index: {error}")),
    })
}

impl FilesystemStore {
    /// The object's `ObjectGenesis` exactly as stored, for serving
    /// byte-for-byte. Like [`FilesystemStore::get_statement_bytes`],
    /// this does not fixity-check; the receiver re-derives the
    /// object id from the body.
    pub fn get_object_genesis_bytes(&self, object: &ObjectId) -> Result<Vec<u8>, StoreError> {
        let path = self.shard_path(OBJECTS_DIR, object.as_str(), JSON_SUFFIX)?;
        read_or_missing(&path)
    }

    /// A source's genesis exactly as stored (`DECISIONS.md` §37), for
    /// serving byte-for-byte; the receiver re-derives the id from the
    /// URL.
    pub fn get_source_bytes(&self, source: &ObjectId) -> Result<Vec<u8>, StoreError> {
        let path = self.shard_path(crate::SOURCES_DIR, source.as_str(), JSON_SUFFIX)?;
        read_or_missing(&path)
    }

    /// A source's closure (`DECISIONS.md` §37): every observation of
    /// it, every observer, and every object an observation names (ref
    /// targets and pinned commits) as servable history.
    pub fn source_closure(&self, source: &ObjectId) -> Result<ObjectClosure, StoreError> {
        crate::SourceStore::get_source(self, source)?;
        let mut closure = ObjectClosure::default();
        for head in crate::SourceStore::list_source_observations(self, source)? {
            let observation =
                crate::StatementStore::get_source_observation(self, &head.statement_id)?;
            closure.actors.insert(head.actor);
            closure.statements.insert(head.statement_id);
            let body = observation.unsigned().body();
            closure
                .commits
                .extend(body.refs().iter().map(|observed| observed.target().clone()));
            closure.commits.extend(body.pinned().iter().cloned());
        }
        Ok(closure)
    }

    /// The genesis of `id` exactly as stored, whichever kind it has: a
    /// native object's `ObjectGenesis`, or a Git or file source's.
    pub fn get_any_genesis_bytes(&self, id: &ObjectId) -> Result<Vec<u8>, StoreError> {
        match self.get_object_genesis_bytes(id) {
            Err(StoreError::Missing) => match self.get_source_bytes(id) {
                Err(StoreError::Missing) => self.get_file_source_genesis_bytes(id),
                other => other,
            },
            other => other,
        }
    }

    /// A file source's genesis exactly as stored (`DECISIONS.md` §38),
    /// for serving byte-for-byte; the receiver re-derives the id from
    /// the hash.
    pub fn get_file_source_genesis_bytes(&self, source: &ObjectId) -> Result<Vec<u8>, StoreError> {
        let path = self.shard_path(crate::FILE_SOURCES_DIR, source.as_str(), JSON_SUFFIX)?;
        read_or_missing(&path)
    }

    /// A file source's closure (`DECISIONS.md` §38): every observation
    /// of it, every observer, and the file's bytes as the blob its hash
    /// names. Only a held file source has one: its bytes must be here.
    pub fn file_source_closure(&self, source: &ObjectId) -> Result<ObjectClosure, StoreError> {
        let genesis = crate::SourceStore::get_file_source(self, source)?;
        if !crate::SourceStore::holds_file_source(self, source)? {
            return Err(StoreError::Missing);
        }
        let mut closure = ObjectClosure::default();
        closure.blobs.insert(genesis.sha256().blob_id());
        for head in crate::SourceStore::list_file_observations(self, source)? {
            closure.actors.insert(head.actor);
            closure.statements.insert(head.statement_id);
        }
        Ok(closure)
    }

    /// Compute `object`'s closure (module docs), or a Git or file
    /// source's when `object` is one. `StoreError::Missing` if the store
    /// holds no such genesis.
    pub fn object_closure(&self, object: &ObjectId) -> Result<ObjectClosure, StoreError> {
        let genesis = match self.get_object_genesis(object) {
            Ok(genesis) => genesis,
            Err(StoreError::Missing) => {
                return match self.source_closure(object) {
                    Err(StoreError::Missing) => self.file_source_closure(object),
                    other => other,
                }
            }
            Err(error) => return Err(error),
        };
        let mut closure = ObjectClosure::default();
        closure.actors.insert(genesis.body().created_by().clone());

        for revision in self.reachable_revisions(object)? {
            closure.statements.insert(revision.statement_id());
            closure.actors.insert(revision.unsigned().actor().clone());
            closure
                .blobs
                .insert(revision.unsigned().body().manifest_hash().clone());
            // A malformed `git:` revision names no commit; `verify
            // object` reports it.
            if let Ok(Some(commit)) = revision.unsigned().body().revision().git() {
                closure.commits.insert(commit);
            }
        }

        if let Some(index) = self.read_branch_index(object)? {
            for (actor, names) in &index.by_actor {
                closure
                    .actors
                    .insert(parse_index_id(actor, object, ActorId::new)?);
                for entry in names.values().flatten() {
                    closure.statements.insert(parse_index_id(
                        &entry.statement_id,
                        object,
                        StatementId::new,
                    )?);
                }
            }
        }

        if let Some(index) = self.read_version_tag_index(object)? {
            for (actor, versions) in &index.by_actor {
                closure
                    .actors
                    .insert(parse_index_id(actor, object, ActorId::new)?);
                for entry in versions.values().flatten() {
                    closure.statements.insert(parse_index_id(
                        &entry.statement_id,
                        object,
                        StatementId::new,
                    )?);
                }
            }
        }

        // Build results are evidence about the object's snapshots and
        // ride with it; their artifacts are a cache and don't
        // (`DECISIONS.md` §34 sub-decision 8).
        for head in self.list_build_results(object)? {
            closure.actors.insert(head.actor);
            closure.statements.insert(head.statement_id);
        }

        if let Some(index) = self.read_capability_by_object_index(object)? {
            // Grantors sign grants and their revocations; grantees
            // appear only if they signed something else above.
            let mut grantors: BTreeSet<ActorId> = BTreeSet::new();
            let mut grants: BTreeSet<StatementId> = BTreeSet::new();
            for by_grantor in index.by_grantee.values() {
                for (grantor, entries) in by_grantor {
                    grantors.insert(parse_index_id(grantor, object, ActorId::new)?);
                    for entry in entries {
                        grants.insert(parse_index_id(
                            &entry.statement_id,
                            object,
                            StatementId::new,
                        )?);
                    }
                }
            }
            for grantor in &grantors {
                let Some(grantor_index) = self.read_capability_index(grantor)? else {
                    continue;
                };
                for (revoked, revocations) in &grantor_index.revocations {
                    let revoked = parse_index_id(revoked, grantor, StatementId::new)?;
                    if !grants.contains(&revoked) {
                        continue;
                    }
                    for revocation in revocations {
                        closure.authority.insert(parse_index_id(
                            &revocation.statement_id,
                            grantor,
                            StatementId::new,
                        )?);
                    }
                }
            }
            closure.actors.extend(grantors);
            closure.authority.extend(grants);
        }

        Ok(closure)
    }

    /// Every key-event statement `actor` has signed — rotations,
    /// revocations, emergency, and attestation kinds — ascending.
    /// Empty for an actor with no key events (or not in the store).
    pub fn actor_key_event_ids(&self, actor: &ActorId) -> Result<Vec<StatementId>, StoreError> {
        let index = self.read_key_index_or_default(actor)?;
        let raw = index
            .rotations
            .iter()
            .map(|entry| &entry.statement_id)
            .chain(index.revocations.iter().map(|entry| &entry.statement_id))
            .chain(
                index
                    .attestation_adds
                    .iter()
                    .map(|entry| &entry.statement_id),
            )
            .chain(
                index
                    .attestation_revocations
                    .iter()
                    .map(|entry| &entry.statement_id),
            )
            .chain(
                index
                    .attestation_threshold_changes
                    .iter()
                    .map(|entry| &entry.statement_id),
            );
        let mut ids = BTreeSet::new();
        for id in raw {
            ids.insert(parse_index_id(id, actor, StatementId::new)?);
        }
        Ok(ids.into_iter().collect())
    }
}
