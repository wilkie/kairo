//! Id listings a node serves for its published objects
//! (`specs/PHASE_2_FEDERATION.md` §2.2).
//!
//! Listings carry ids only. The client diffs them against its own
//! store and fetches each missing record from the per-record
//! endpoints, which is what makes a re-pull incremental.

use std::fmt;

use kairo_core::{ActorId, BlobId, ObjectId, StatementId};
use kairo_identity::json::{ActorGenesisJson, ActorGenesisJsonError};
use kairo_identity::ActorGenesisBody;
use serde::{Deserialize, Serialize};

use crate::page::Cursor;
use crate::schema::schema_tag;

schema_tag!(
    /// `"kairo.fed.objects.v1"`.
    PublishedObjectsSchema = "kairo.fed.objects.v1"
);

schema_tag!(
    /// `"kairo.fed.object-listing.v1"`.
    ObjectListingSchema = "kairo.fed.object-listing.v1"
);

schema_tag!(
    /// `"kairo.fed.actor-listing.v1"`.
    ActorListingSchema = "kairo.fed.actor-listing.v1"
);

/// `GET /federation/v1/objects`: one page of the node's published
/// object ids, ascending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedObjects {
    pub schema: PublishedObjectsSchema,
    #[serde(with = "crate::wire_id::many")]
    pub objects: Vec<ObjectId>,
    pub next_cursor: Option<Cursor>,
}

/// `GET /federation/v1/objects/{id}`: one page of the ids in an
/// object's published closure (`specs/DECISIONS.md` §16
/// sub-decision 4).
///
/// Paging walks `statements`, `authority`, `actors`, then `blobs`,
/// each ascending, so a page may end partway through one list and
/// the next page resumes there with the earlier lists empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectListing {
    pub schema: ObjectListingSchema,
    #[serde(with = "crate::wire_id::one")]
    pub object: ObjectId,
    /// Every `ObjectRevision`, `ObjectBranch`, and `ObjectVersionTag`
    /// whose body names the object. `ObjectGenesis` has no statement
    /// id (its body derives the object id); it is served at
    /// `/federation/v1/objects/{id}/genesis`.
    #[serde(with = "crate::wire_id::many")]
    pub statements: Vec<StatementId>,
    /// `ActorCapabilityGrant`s scoped to the object and
    /// `ActorCapabilityRevocation`s of them: evidence the capability
    /// evaluator needs for cross-actor statements, not opinions.
    #[serde(with = "crate::wire_id::many")]
    pub authority: Vec<StatementId>,
    /// Every actor that signed a listed statement. Their genesis and
    /// key events come from `/federation/v1/actors/{id}`, not here:
    /// key knowledge belongs to the actor, not the object.
    #[serde(with = "crate::wire_id::many")]
    pub actors: Vec<ActorId>,
    #[serde(with = "crate::wire_id::many")]
    pub blobs: Vec<BlobId>,
    /// Present on the first page only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitAvailability>,
    pub next_cursor: Option<Cursor>,
}

impl ObjectListing {
    /// Total ids on this page across the four lists.
    pub fn len(&self) -> usize {
        self.statements.len() + self.authority.len() + self.actors.len() + self.blobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Whether `GET /federation/v1/objects/{id}/git` has a pack to serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitAvailability {
    pub available: bool,
}

/// `GET /federation/v1/actors/{id}`: an actor's genesis and the ids
/// of its key-event statements (rotations, revocations, emergency
/// and attestation kinds), ascending. Unpaged: key-event histories
/// are short.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorListing {
    pub schema: ActorListingSchema,
    #[serde(with = "crate::wire_id::one")]
    pub actor: ActorId,
    pub genesis: ActorGenesisJson,
    #[serde(with = "crate::wire_id::many")]
    pub key_events: Vec<StatementId>,
}

impl ActorListing {
    /// Check that `actor` derives from `genesis`. Returns the parsed
    /// genesis on success.
    pub fn validate(&self) -> Result<ActorGenesisBody, ActorListingError> {
        let genesis = self
            .genesis
            .to_body()
            .map_err(ActorListingError::InvalidGenesis)?;
        let derived = genesis.actor_id();
        if derived != self.actor {
            return Err(ActorListingError::ActorMismatch {
                claimed: self.actor.clone(),
                derived,
            });
        }
        Ok(genesis)
    }
}

#[derive(Debug)]
pub enum ActorListingError {
    InvalidGenesis(ActorGenesisJsonError),
    ActorMismatch { claimed: ActorId, derived: ActorId },
}

impl fmt::Display for ActorListingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGenesis(error) => write!(f, "invalid genesis: {error}"),
            Self::ActorMismatch { claimed, derived } => write!(
                f,
                "actor {claimed} does not derive from its genesis (derives {derived})"
            ),
        }
    }
}

impl std::error::Error for ActorListingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidGenesis(error) => Some(error),
            Self::ActorMismatch { .. } => None,
        }
    }
}

schema_tag!(
    /// `"kairo.fed.actor-export.v1"`.
    ActorExportSchema = "kairo.fed.actor-export.v1"
);

/// An actor's genesis and key events in one self-verifying file, for
/// learning a node that can't be reached (`specs/DECISIONS.md` §26
/// sub-decision 7). Each key event is the stored JSON, exactly.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorExport {
    pub schema: ActorExportSchema,
    #[serde(with = "crate::wire_id::one")]
    pub actor: ActorId,
    pub genesis: ActorGenesisJson,
    pub key_events: Vec<crate::BatchedStatement>,
}
