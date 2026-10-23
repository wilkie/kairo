//! `GET /federation/v1/children/{id}` (`specs/DECISIONS.md` §45): which
//! published object a child object belongs to. A child has no
//! statements of its own (§44), so a node fetching one by id fetches
//! its parent, and only a node holding the parent's manifest can say
//! which that is. The answer proves itself: the child's id derives from
//! `(parent, target, name)`, which the client re-derives.

use kairo_core::ObjectId;
use serde::{Deserialize, Serialize};

use crate::schema::schema_tag;

/// The endpoint, under [`crate::API_PREFIX`], followed by `/{id}`.
pub const CHILDREN_PATH: &str = "/children";

schema_tag!(
    /// `"kairo.fed.child.v1"`.
    ChildListingSchema = "kairo.fed.child.v1"
);

/// The parent, target, and name a child's id derives from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildListing {
    pub schema: ChildListingSchema,
    #[serde(with = "crate::wire_id::one")]
    pub child: ObjectId,
    #[serde(with = "crate::wire_id::one")]
    pub parent: ObjectId,
    pub target: String,
    pub name: String,
}
