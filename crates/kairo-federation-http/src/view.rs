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

use kairo_core::{ActorId, BlobId, ObjectId, StatementId, Timestamp};
use kairo_federation_core::Inventory;
use kairo_identity::SecretSigningKey;
use kairo_store::{
    FilesystemStore, ObjectClosure, PublishedObjectStore, StoreError, TrustTransportStore,
};

#[derive(Debug, Default)]
pub(crate) struct PublishedView {
    /// Published objects whose genesis is present, ascending.
    pub objects: Vec<ObjectId>,
    pub closures: BTreeMap<ObjectId, ObjectClosure>,
    /// For each published native object, the sources its revisions pin
    /// that this node serves (`DECISIONS.md` §37 sub-decision 9).
    pub sources: BTreeMap<ObjectId, Vec<ObjectId>>,
    /// Each child object a published native object's revisions declare:
    /// its parent, target, and name (`DECISIONS.md` §45).
    pub children: BTreeMap<ObjectId, (ObjectId, String, String)>,
    /// Every actor named by some closure, and the node actor, with
    /// their key-event ids.
    pub actors: BTreeMap<ActorId, Vec<StatementId>>,
    /// Published trust chains, by (truster, scope).
    pub trust: BTreeMap<(ActorId, String), Vec<StatementId>>,
    /// Object statements, authority, key events, and published trust.
    pub statements: HashSet<StatementId>,
    pub blobs: HashSet<BlobId>,
    /// This node's inventory of `objects`, signed when the view was
    /// built (`DECISIONS.md` §29).
    pub inventory: Option<Inventory>,
}

impl PublishedView {
    pub(crate) fn build(store: &FilesystemStore, node_actor: &ActorId) -> Result<Self, StoreError> {
        let mut view = Self::default();
        let mut actors: BTreeSet<ActorId> = BTreeSet::from([node_actor.clone()]);
        let mut pinned: std::collections::BTreeMap<ObjectId, Vec<ObjectClosure>> =
            Default::default();
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
            // The sources its revisions pin are served at those commits
            // (`DECISIONS.md` §37 sub-decision 9, §38).
            let mut served = Vec::new();
            for (source, restricted) in
                crate::pinned_source_closures(store, &published.object_id, &closure.statements)?
            {
                pinned.entry(source.clone()).or_default().push(restricted);
                served.push(source);
            }
            if !served.is_empty() {
                view.sources.insert(published.object_id.clone(), served);
            }
            for (child, (target, name)) in
                crate::manifest_children(store, &published.object_id, &closure.statements)
            {
                view.children
                    .insert(child, (published.object_id.clone(), target, name));
            }
            view.objects.push(published.object_id.clone());
            view.closures.insert(published.object_id, closure);
        }
        // Pinned sources join the view, their commits merged with any
        // a direct publication already serves.
        for (source, closures) in pinned {
            for closure in closures {
                view.statements.extend(closure.statements.iter().cloned());
                view.blobs.extend(closure.blobs.iter().cloned());
                actors.extend(closure.actors.iter().cloned());
                match view.closures.get_mut(&source) {
                    Some(existing) => existing.commits.extend(closure.commits),
                    None => {
                        view.closures.insert(source.clone(), closure);
                    }
                }
            }
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
    base_url: String,
    signer: Arc<SecretSigningKey>,
    ttl: Duration,
    current: Mutex<Option<(Instant, Arc<PublishedView>)>>,
}

impl ViewCache {
    pub(crate) fn new(
        node_actor: ActorId,
        base_url: String,
        signer: Arc<SecretSigningKey>,
        ttl: Duration,
    ) -> Self {
        Self {
            node_actor,
            base_url,
            signer,
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
        let mut view = PublishedView::build(store, &self.node_actor)?;
        view.inventory = Some(Inventory::signed(
            self.node_actor.clone(),
            self.base_url.clone(),
            view.objects.iter().cloned(),
            &self.signer,
            Timestamp::now(),
        ));
        let view = Arc::new(view);
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
