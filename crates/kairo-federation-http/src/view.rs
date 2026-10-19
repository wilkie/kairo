//! The published view: every id this node may serve.
//!
//! Built from the publish list and each published object's
//! [`ObjectClosure`], plus the key events of every actor those
//! closures name and of the node actor, which is always served so a
//! peer can check the node document's signature (`DECISIONS.md` §25
//! sub-decision 2), plus each published trust chain (§27) and its
//! truster's key events. Every per-record endpoint checks membership here
//! first, so a record outside every published closure is
//! indistinguishable from one the store doesn't hold.
//!
//! Building walks the store (revisions have no per-object index), so
//! [`ViewCache`] reuses a view for the configured TTL. A pull issues
//! one request per record; without the cache each would recompute
//! every closure.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kairo_core::{ActorId, BlobId, ObjectId, StatementId};
use kairo_store::{
    FilesystemStore, ObjectClosure, PublishedObjectStore, StoreError, TrustTransportStore,
};

#[derive(Debug, Default)]
pub(crate) struct PublishedView {
    /// Published objects whose genesis is present, ascending.
    pub objects: Vec<ObjectId>,
    pub closures: BTreeMap<ObjectId, ObjectClosure>,
    /// Every actor named by some closure, and the node actor, with
    /// their key-event ids.
    pub actors: BTreeMap<ActorId, Vec<StatementId>>,
    /// Published trust chains, by (truster, scope).
    pub trust: BTreeMap<(ActorId, String), Vec<StatementId>>,
    /// Object statements, authority, key events, and published trust.
    pub statements: HashSet<StatementId>,
    pub blobs: HashSet<BlobId>,
}

impl PublishedView {
    pub(crate) fn build(store: &FilesystemStore, node_actor: &ActorId) -> Result<Self, StoreError> {
        let mut view = Self::default();
        let mut actors: BTreeSet<ActorId> = BTreeSet::from([node_actor.clone()]);
        for published in store.list_published_objects()? {
            let closure = match store.object_closure(&published.object_id) {
                Ok(closure) => closure,
                // Publishing requires the genesis, so this means it was
                // removed by hand afterwards. Serve nothing for it.
                Err(StoreError::Missing) => {
                    tracing::warn!(
                        object = %published.object_id,
                        "published object has no genesis in the store; not serving it"
                    );
                    continue;
                }
                Err(error) => return Err(error),
            };
            view.statements.extend(closure.statements.iter().cloned());
            view.statements.extend(closure.authority.iter().cloned());
            view.blobs.extend(closure.blobs.iter().cloned());
            actors.extend(closure.actors.iter().cloned());
            view.objects.push(published.object_id.clone());
            view.closures.insert(published.object_id, closure);
        }
        for chain in store.list_published_trust()? {
            let ids = store.trust_chain_ids(&chain.by, &chain.scope)?;
            view.statements.extend(ids.iter().cloned());
            actors.insert(chain.by.clone());
            view.trust.insert((chain.by, chain.scope.to_string()), ids);
        }
        for actor in actors {
            let key_events = store.actor_key_event_ids(&actor)?;
            view.statements.extend(key_events.iter().cloned());
            view.actors.insert(actor, key_events);
        }
        Ok(view)
    }
}

/// A [`PublishedView`] reused for `ttl`. Two requests racing past the
/// expiry may both rebuild; the later write wins, which is harmless.
#[derive(Debug)]
pub(crate) struct ViewCache {
    node_actor: ActorId,
    ttl: Duration,
    current: Mutex<Option<(Instant, Arc<PublishedView>)>>,
}

impl ViewCache {
    pub(crate) fn new(node_actor: ActorId, ttl: Duration) -> Self {
        Self {
            node_actor,
            ttl,
            current: Mutex::new(None),
        }
    }

    pub(crate) fn get(&self, store: &FilesystemStore) -> Result<Arc<PublishedView>, StoreError> {
        if let Some((built, view)) = self.lock().as_ref() {
            if built.elapsed() < self.ttl {
                return Ok(Arc::clone(view));
            }
        }
        let view = Arc::new(PublishedView::build(store, &self.node_actor)?);
        *self.lock() = Some((Instant::now(), Arc::clone(&view)));
        Ok(view)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<(Instant, Arc<PublishedView>)>> {
        // A panic while holding the lock leaves a stale-but-valid
        // view; keep serving rather than poisoning every request.
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
