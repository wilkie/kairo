//! Trust transport and `forget` (`specs/DECISIONS.md` §27): which
//! (truster, scope) chains this node serves, what a chain is, and
//! deleting a non-local truster's opinions.
//!
//! The publish list is `<root>/federation/published-trust.json`:
//!
//! ```json
//! { "<truster-id>": { "<scope>": "<RFC 3339 published_at>" } }
//! ```
//!
//! Primary state, like the object publish list: `rebuild_indexes()`
//! leaves it alone.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use kairo_core::{ActorId, StatementId, Timestamp};
use kairo_statement::{StatementKind, TrustScope};

use crate::error::{CorruptReason, StoreError};
use crate::lock::with_index_lock;
use crate::provenance::PROVENANCE_DIR;
use crate::{
    atomic_write, json_to_corrupt, read_or_missing, FilesystemStore, StatementByActorResolver,
    StatementStore, JSON_SUFFIX, STATEMENTS_BY_ACTOR_DIR, STATEMENTS_DIR, TRUST_DIR,
};

const PUBLISHED_TRUST_RECORD: &str = "federation/published-trust.json";

/// One published trust chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedTrust {
    pub by: ActorId,
    pub scope: TrustScope,
    pub published_at: Timestamp,
}

type PublishedTrustFile = BTreeMap<String, BTreeMap<String, String>>;

pub trait TrustTransportStore {
    /// Every `ActorTrust` `by` signed in `scope`, about anyone,
    /// withdrawals included, in `(created_at, id)` order.
    fn trust_chain_ids(
        &self,
        by: &ActorId,
        scope: &TrustScope,
    ) -> Result<Vec<StatementId>, StoreError>;

    /// Publish `(by, scope)`. `false` if it already was. Refused when
    /// the chain is empty: there would be nothing to serve.
    fn publish_trust(
        &self,
        by: &ActorId,
        scope: &TrustScope,
        at: Timestamp,
    ) -> Result<bool, StoreError>;

    /// `false` if it wasn't published.
    fn unpublish_trust(&self, by: &ActorId, scope: &TrustScope) -> Result<bool, StoreError>;

    /// Ordered by truster, then scope.
    fn list_published_trust(&self) -> Result<Vec<PublishedTrust>, StoreError>;

    /// Delete every `ActorTrust` `by` signed (in `scope`, if given),
    /// with its index entries and provenance, and unpublish the chains.
    /// The caller refuses local trusters (§27 sub-decision 5). Returns
    /// how many statements were deleted.
    fn forget_trust(&self, by: &ActorId, scope: Option<&TrustScope>) -> Result<usize, StoreError>;
}

impl FilesystemStore {
    fn published_trust_path(&self) -> PathBuf {
        self.root.join("federation").join("published-trust.json")
    }

    fn read_published_trust(&self) -> Result<PublishedTrustFile, StoreError> {
        match read_or_missing(&self.published_trust_path()) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(json_to_corrupt(&PUBLISHED_TRUST_RECORD))
            }
            Err(StoreError::Missing) => Ok(BTreeMap::new()),
            Err(error) => Err(error),
        }
    }

    fn update_published_trust<T>(
        &self,
        change: impl FnOnce(&mut PublishedTrustFile) -> T,
    ) -> Result<T, StoreError> {
        let path = self.published_trust_path();
        with_index_lock(&path, || {
            let mut file = self.read_published_trust()?;
            let result = change(&mut file);
            file.retain(|_, scopes| !scopes.is_empty());
            let mut bytes = serde_json::to_vec_pretty(&file)
                .map_err(json_to_corrupt(&PUBLISHED_TRUST_RECORD))?;
            bytes.push(b'\n');
            atomic_write(&path, &bytes)?;
            Ok(result)
        })
    }

    /// `by`'s `ActorTrust` ids, with each one's scope and subject.
    fn trust_statements_by(
        &self,
        by: &ActorId,
    ) -> Result<Vec<(StatementId, TrustScope, ActorId)>, StoreError> {
        let mut found = Vec::new();
        for entry in self.list_statements_by_actor(by)? {
            if entry.kind != StatementKind::ActorTrust {
                continue;
            }
            let signed = self.get_actor_trust(&entry.statement_id)?;
            let body = signed.unsigned().body();
            found.push((
                entry.statement_id,
                body.scope().clone(),
                body.trusted_actor().clone(),
            ));
        }
        Ok(found)
    }

    /// Drop `id` from `by`'s entries in `trusted`'s trust index.
    fn remove_trust_index_entry(
        &self,
        trusted: &ActorId,
        by: &ActorId,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(TRUST_DIR, trusted.as_str(), JSON_SUFFIX)?;
        with_index_lock(&path, || {
            let Some(mut index) = self.read_trust_index(trusted)? else {
                return Ok(());
            };
            if let Some(entries) = index.by_truster.get_mut(by.as_str()) {
                entries.retain(|entry| entry.statement_id != id.as_str());
                if entries.is_empty() {
                    index.by_truster.remove(by.as_str());
                }
            }
            if index.by_truster.is_empty() {
                return remove_if_present(&path);
            }
            let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(trusted))?;
            atomic_write(&path, &bytes)
        })
    }

    fn remove_statement_by_actor_entry(
        &self,
        actor: &ActorId,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(STATEMENTS_BY_ACTOR_DIR, actor.as_str(), JSON_SUFFIX)?;
        with_index_lock(&path, || {
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(StoreError::Unavailable(error)),
            };
            let mut index: crate::statements_by_actor::StatementByActorIndexFile =
                serde_json::from_slice(&bytes).map_err(|error| StoreError::Corrupt {
                    id: actor.to_string(),
                    reason: CorruptReason::Parse(format!(
                        "invalid statements_by_actor index: {error}"
                    )),
                })?;
            index
                .entries
                .retain(|entry| entry.statement_id != id.as_str());
            let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(actor))?;
            atomic_write(&path, &bytes)
        })
    }
}

fn remove_if_present(path: &std::path::Path) -> Result<(), StoreError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StoreError::Unavailable(error)),
    }
}

fn corrupt(error: &dyn std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        id: PUBLISHED_TRUST_RECORD.to_owned(),
        reason: CorruptReason::Parse(error.to_string()),
    }
}

impl TrustTransportStore for FilesystemStore {
    fn trust_chain_ids(
        &self,
        by: &ActorId,
        scope: &TrustScope,
    ) -> Result<Vec<StatementId>, StoreError> {
        Ok(self
            .trust_statements_by(by)?
            .into_iter()
            .filter(|(_, in_scope, _)| in_scope == scope)
            .map(|(id, _, _)| id)
            .collect())
    }

    fn publish_trust(
        &self,
        by: &ActorId,
        scope: &TrustScope,
        at: Timestamp,
    ) -> Result<bool, StoreError> {
        if self.trust_chain_ids(by, scope)?.is_empty() {
            return Err(StoreError::Rejected {
                reason: format!("{by} has no opinions in scope {scope} to publish"),
            });
        }
        self.update_published_trust(|file| {
            file.entry(by.to_string())
                .or_default()
                .insert(scope.to_string(), at.to_string())
                .is_none()
        })
    }

    fn unpublish_trust(&self, by: &ActorId, scope: &TrustScope) -> Result<bool, StoreError> {
        self.update_published_trust(|file| {
            file.get_mut(by.as_str())
                .is_some_and(|scopes| scopes.remove(scope.as_str()).is_some())
        })
    }

    fn list_published_trust(&self) -> Result<Vec<PublishedTrust>, StoreError> {
        let mut chains = Vec::new();
        for (by, scopes) in self.read_published_trust()? {
            let by = ActorId::new(&by).map_err(|e| corrupt(&e))?;
            for (scope, at) in scopes {
                chains.push(PublishedTrust {
                    by: by.clone(),
                    scope: TrustScope::parse(&scope).map_err(|e| corrupt(&e))?,
                    published_at: at.parse().map_err(|e| corrupt(&e))?,
                });
            }
        }
        Ok(chains)
    }

    fn forget_trust(&self, by: &ActorId, scope: Option<&TrustScope>) -> Result<usize, StoreError> {
        let doomed: Vec<(StatementId, TrustScope, ActorId)> = self
            .trust_statements_by(by)?
            .into_iter()
            .filter(|(_, in_scope, _)| scope.is_none_or(|scope| scope == in_scope))
            .collect();
        // Unpublish first, so nothing is served mid-way.
        self.update_published_trust(|file| match scope {
            Some(scope) => {
                if let Some(scopes) = file.get_mut(by.as_str()) {
                    scopes.remove(scope.as_str());
                }
            }
            None => {
                file.remove(by.as_str());
            }
        })?;
        for (id, _, trusted) in &doomed {
            // Indexes before the record: an interrupted forget leaves a
            // statement no index names, which `rebuild_indexes` would
            // restore, rather than an index naming a missing file.
            self.remove_trust_index_entry(trusted, by, id)?;
            self.remove_statement_by_actor_entry(by, id)?;
            remove_if_present(&self.shard_path(PROVENANCE_DIR, id.as_str(), JSON_SUFFIX)?)?;
            remove_if_present(&self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?)?;
        }
        Ok(doomed.len())
    }
}
