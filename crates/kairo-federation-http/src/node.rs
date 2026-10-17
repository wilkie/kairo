//! Startup checks and shared request state.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use kairo_core::ActorId;
use kairo_federation_core::{NodeDocument, PROTOCOL_VERSION, SUPPORTS_OBJECT_PULL};
use kairo_identity::json::ActorGenesisJson;
use kairo_identity::ActorKind;
use kairo_statement::StatementKind;
use kairo_store::{
    ActorStore, FilesystemStore, ObjectsByActorResolver, StatementByActorResolver, StoreError,
};

use crate::error::Error;
use crate::upload::UploadService;
use crate::view::ViewCache;
use crate::Config;

/// A configured federation node: the store it serves, its node
/// document, and the published-view cache. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Node {
    pub(crate) store: FilesystemStore,
    pub(crate) document: Arc<NodeDocument>,
    pub(crate) views: Arc<ViewCache>,
    pub(crate) uploads: Arc<UploadService>,
}

impl Node {
    /// Open the store and check the configuration:
    ///
    /// - the node actor is in the store with `actor_kind = "node"`
    ///   (refused otherwise, §16 sub-decision 1);
    /// - the assembled node document validates (base URL shape);
    /// - the node actor has not created objects or signed object or
    ///   capability statements (warned, not refused: the convention
    ///   that a node actor holds no authority is advice to the
    ///   operator, and nothing authorizes on `actor_kind`).
    pub fn open(config: &Config) -> Result<Self, Error> {
        let store = FilesystemStore::open(&config.store).map_err(|source| Error::OpenStore {
            path: config.store.clone(),
            source,
        })?;
        let node = Self::from_store(store, &config.node_actor, &config.base_url, config.view_ttl)?;
        Ok(match config.max_concurrent_packs {
            Some(max) => node.with_max_concurrent_packs(max),
            None => node,
        })
    }

    /// [`Node::open`] over an already-open store.
    pub fn from_store(
        store: FilesystemStore,
        node_actor: &ActorId,
        base_url: &str,
        view_ttl: Duration,
    ) -> Result<Self, Error> {
        let genesis = match store.get_actor(node_actor) {
            Ok(genesis) => genesis,
            Err(StoreError::Missing) => {
                return Err(Error::NodeActorMissing {
                    actor: node_actor.clone(),
                })
            }
            Err(source) => {
                return Err(Error::ReadNodeActor {
                    actor: node_actor.clone(),
                    source,
                })
            }
        };
        if genesis.actor_kind() != &ActorKind::node() {
            return Err(Error::NotANodeActor {
                actor: node_actor.clone(),
                kind: genesis.actor_kind().as_str().to_owned(),
            });
        }

        let document = NodeDocument {
            schema: Default::default(),
            node_actor: node_actor.clone(),
            actor_genesis: ActorGenesisJson::from_body(&genesis),
            base_url: base_url.to_owned(),
            protocol_versions: vec![PROTOCOL_VERSION.to_owned()],
            supports: vec![SUPPORTS_OBJECT_PULL.to_owned()],
        };
        document.validate().map_err(Error::NodeDocument)?;

        warn_if_node_actor_holds_authority(&store, node_actor);

        Ok(Self {
            store,
            document: Arc::new(document),
            views: Arc::new(ViewCache::new(view_ttl)),
            uploads: Arc::new(UploadService::new(UploadService::default_concurrency())),
        })
    }

    /// Cap concurrent Git fetches (`upload-pack` runs) at `max`
    /// (default: half the CPUs). Requests over the cap wait briefly,
    /// then get `503 fed_busy`.
    pub fn with_max_concurrent_packs(mut self, max: usize) -> Self {
        self.uploads = Arc::new(UploadService::new(max));
        self
    }

    pub fn document(&self) -> &NodeDocument {
        &self.document
    }

    /// The axum router for every endpoint in
    /// `specs/PHASE_2_FEDERATION.md` §2.1.
    pub fn router(&self) -> Router {
        crate::handlers::router(self.clone())
    }
}

fn warn_if_node_actor_holds_authority(store: &FilesystemStore, actor: &ActorId) {
    match store.list_objects_by_actor(actor) {
        Ok(objects) if !objects.is_empty() => tracing::warn!(
            node_actor = %actor,
            objects = objects.len(),
            "node actor created objects; a node actor should hold no object authority \
             (DECISIONS.md §16 sub-decision 1)"
        ),
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "could not check objects created by the node actor"),
    }
    match store.list_statements_by_actor(actor) {
        Ok(statements) => {
            let signed_authority = statements.iter().any(|statement| {
                matches!(
                    statement.kind,
                    StatementKind::ObjectRevision
                        | StatementKind::ObjectBranch
                        | StatementKind::ObjectVersionTag
                        | StatementKind::ActorCapabilityGrant
                        | StatementKind::ActorCapabilityRevocation
                )
            });
            if signed_authority {
                tracing::warn!(
                    node_actor = %actor,
                    "node actor has signed object or capability statements; keep the operator's \
                     personal actor separate (DECISIONS.md §16 sub-decision 1)"
                );
            }
        }
        Err(error) => tracing::warn!(%error, "could not check statements signed by the node actor"),
    }
}
