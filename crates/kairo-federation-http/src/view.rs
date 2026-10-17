//! The published view: every id this node may serve.
//!
//! Built from the publish list and each published object's
//! [`ObjectClosure`], plus the key events of every actor those
//! closures name. Every per-record endpoint checks membership here
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
use kairo_store::{FilesystemStore, ObjectClosure, PublishedObjectStore, StoreError};

#[derive(Debug, Default)]
pub(crate) struct PublishedView {
    /// Published objects whose genesis is present, ascending.
    pub objects: Vec<ObjectId>,
    pub closures: BTreeMap<ObjectId, ObjectClosure>,
    /// Every actor named by some closure, with its key-event ids.
    pub actors: BTreeMap<ActorId, Vec<StatementId>>,
    /// Object statements, authority, and key events.
    pub statements: HashSet<StatementId>,
    pub blobs: HashSet<BlobId>,
}

impl PublishedView {
    pub(crate) fn build(store: &FilesystemStore) -> Result<Self, StoreError> {
        let mut view = Self::default();
        let mut actors: BTreeSet<ActorId> = BTreeSet::new();
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
    ttl: Duration,
    current: Mutex<Option<(Instant, Arc<PublishedView>)>>,
}

impl ViewCache {
    pub(crate) fn new(ttl: Duration) -> Self {
        Self {
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
        let view = Arc::new(PublishedView::build(store)?);
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
