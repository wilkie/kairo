//! Uploads (`specs/DECISIONS.md` §26 sub-decisions 6 and 8): the plan
//! exchange that keeps an upload to the delta, and the closure
//! inventory a plan and a closure bundle both carry.

use std::collections::BTreeSet;

use kairo_core::{ActorId, BlobId, ObjectId, StatementId};
use serde::{Deserialize, Serialize};

use crate::schema::schema_tag;

/// Path of the plan endpoint, under [`crate::API_PREFIX`].
pub const PUSH_PLAN_PATH: &str = "/push/plan";

/// Path of the upload endpoint, under [`crate::API_PREFIX`]. The
/// object goes in the query: `?object=<id>`.
pub const PUSH_PATH: &str = "/push";

/// Largest plan request body.
pub const MAX_PLAN_BYTES: usize = 8 * 1024 * 1024;

/// Most ids one inventory may name, across every list.
pub const MAX_INVENTORY_IDS: usize = 100_000;

schema_tag!(
    /// `"kairo.fed.push-plan-request.v1"`.
    PushPlanRequestSchema = "kairo.fed.push-plan-request.v1"
);

schema_tag!(
    /// `"kairo.fed.push-plan.v1"`.
    PushPlanSchema = "kairo.fed.push-plan.v1"
);

schema_tag!(
    /// `"kairo.fed.push-accepted.v1"`.
    PushAcceptedSchema = "kairo.fed.push-accepted.v1"
);

/// `202 Accepted` body for an upload: the bundle is in the inbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushAccepted {
    pub schema: PushAcceptedSchema,
    pub entry: String,
}

/// One actor in an inventory, with its key-event ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryActor {
    #[serde(with = "crate::wire_id::one")]
    pub actor: ActorId,
    #[serde(with = "crate::wire_id::many")]
    pub key_events: Vec<StatementId>,
}

/// An object's published closure as one node holds it: the ids its
/// object listing and actor listings would name, plus the typed Git
/// revisions (`git:<format>:<oid>`) its revisions point at.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClosureInventory {
    #[serde(with = "crate::wire_id::many")]
    pub statements: Vec<StatementId>,
    #[serde(with = "crate::wire_id::many")]
    pub authority: Vec<StatementId>,
    pub actors: Vec<InventoryActor>,
    #[serde(with = "crate::wire_id::many")]
    pub blobs: Vec<BlobId>,
    pub commits: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryError {
    TooMany {
        count: usize,
    },
    /// The same id twice, or an actor listed twice.
    Duplicate {
        id: String,
    },
}

impl std::fmt::Display for InventoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooMany { count } => write!(
                f,
                "an inventory names at most {MAX_INVENTORY_IDS} ids, not {count}"
            ),
            Self::Duplicate { id } => write!(f, "{id} is named twice"),
        }
    }
}

impl std::error::Error for InventoryError {}

impl ClosureInventory {
    /// Every id counted against [`MAX_INVENTORY_IDS`].
    pub fn len(&self) -> usize {
        self.statements.len()
            + self.authority.len()
            + self.blobs.len()
            + self.commits.len()
            + self
                .actors
                .iter()
                .map(|actor| 1 + actor.key_events.len())
                .sum::<usize>()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every statement id: object statements, authority, key events.
    pub fn statement_ids(&self) -> impl Iterator<Item = &StatementId> {
        self.statements
            .iter()
            .chain(&self.authority)
            .chain(self.actors.iter().flat_map(|actor| &actor.key_events))
    }

    /// Within [`MAX_INVENTORY_IDS`], and no id named twice. Commits
    /// are checked as typed revisions by whoever reads them.
    pub fn validate(&self) -> Result<(), InventoryError> {
        let count = self.len();
        if count > MAX_INVENTORY_IDS {
            return Err(InventoryError::TooMany { count });
        }
        let duplicate = |id: String| Err(InventoryError::Duplicate { id });
        let mut statements = BTreeSet::new();
        for id in self.statement_ids() {
            if !statements.insert(id) {
                return duplicate(format!("statement {id}"));
            }
        }
        let mut actors = BTreeSet::new();
        for actor in &self.actors {
            if !actors.insert(&actor.actor) {
                return duplicate(format!("actor {}", actor.actor));
            }
        }
        let mut blobs = BTreeSet::new();
        for blob in &self.blobs {
            if !blobs.insert(blob) {
                return duplicate(format!("blob {blob}"));
            }
        }
        let mut commits = BTreeSet::new();
        for commit in &self.commits {
            if !commits.insert(commit) {
                return duplicate(format!("commit {commit}"));
            }
        }
        Ok(())
    }
}

/// `POST /federation/v1/push/plan` body, signed with `KairoNode1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushPlanRequest {
    pub schema: PushPlanRequestSchema,
    #[serde(with = "crate::wire_id::one")]
    pub object: ObjectId,
    pub inventory: ClosureInventory,
}

/// The receiver's answer: what to send.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushPlan {
    pub schema: PushPlanSchema,
    /// The receiver lacks the object genesis.
    pub genesis: bool,
    /// Actors whose genesis the receiver lacks.
    #[serde(with = "crate::wire_id::many")]
    pub actors: Vec<ActorId>,
    /// Statements the receiver lacks, of any section.
    #[serde(with = "crate::wire_id::many")]
    pub statements: Vec<StatementId>,
    #[serde(with = "crate::wire_id::many")]
    pub blobs: Vec<BlobId>,
    /// Inventory commits the receiver already has: the pusher's Git
    /// pack excludes what they reach.
    pub have_commits: Vec<String>,
}

impl PushPlan {
    /// Nothing to send but, possibly, Git history.
    pub fn wants_no_records(&self) -> bool {
        !self.genesis
            && self.actors.is_empty()
            && self.statements.is_empty()
            && self.blobs.is_empty()
    }
}
