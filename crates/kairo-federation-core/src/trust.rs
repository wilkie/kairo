//! Trust transport (`specs/DECISIONS.md` §27): the (truster, scope)
//! chains a node publishes, and one chain's statement ids.

use kairo_core::{ActorId, StatementId};
use serde::{Deserialize, Serialize};

use crate::schema::schema_tag;

/// Path of the published-chain list and, with `/{by}?scope=`, of one
/// chain, under [`crate::API_PREFIX`].
pub const TRUST_PATH: &str = "/trust";

schema_tag!(
    /// `"kairo.fed.published-trust.v1"`.
    PublishedTrustSchema = "kairo.fed.published-trust.v1"
);

schema_tag!(
    /// `"kairo.fed.trust-listing.v1"`.
    TrustListingSchema = "kairo.fed.trust-listing.v1"
);

/// One published chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustChainRef {
    #[serde(with = "crate::wire_id::one")]
    pub by: ActorId,
    /// A `TrustScope`, checked by whoever reads it.
    pub scope: String,
}

/// `GET /federation/v1/trust`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedTrust {
    pub schema: PublishedTrustSchema,
    pub chains: Vec<TrustChainRef>,
}

/// `GET /federation/v1/trust/{by}?scope=<scope>`: every `ActorTrust`
/// `by` signed in `scope`, withdrawals included. The truster's genesis
/// and key events are at `/federation/v1/actors/{by}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustListing {
    pub schema: TrustListingSchema,
    #[serde(with = "crate::wire_id::one")]
    pub by: ActorId,
    pub scope: String,
    #[serde(with = "crate::wire_id::many")]
    pub statements: Vec<StatementId>,
}
