//! Inventories (`specs/DECISIONS.md` §29): a node's signed claim of
//! which published objects it serves at its base URL, the first step
//! of discovery. A receiver keeps the newest per node as a hint; it is
//! never a permission.

use std::collections::BTreeSet;
use std::fmt;

use kairo_core::canonical::{encode_list, encode_str, CanonicalEncode};
use kairo_core::{ActorId, ObjectId, Timestamp};
use kairo_identity::{ActorResolver, SecretSigningKey};
use serde::{Deserialize, Serialize};

use crate::node::{
    parse_signed_at, sign_node_bytes, validate_base_url, verify_node_bytes, NodeDocumentError,
};
use crate::schema::schema_tag;
use crate::NodeSignature;

/// Path of a node's own inventory, under [`crate::API_PREFIX`].
pub const INVENTORY_PATH: &str = "/inventory";

/// Domain that starts an inventory's signed bytes.
pub const INVENTORY_DOMAIN: &str = "kairo.fed.inventory.v1";

/// Most objects one inventory may list.
pub const MAX_INVENTORY_OBJECTS: usize = 100_000;

schema_tag!(
    /// `"kairo.fed.inventory.v1"`.
    InventorySchema = "kairo.fed.inventory.v1"
);

/// `GET /federation/v1/inventory`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub schema: InventorySchema,
    #[serde(with = "crate::wire_id::one")]
    pub node_actor: ActorId,
    /// Where the node serves these objects.
    pub base_url: String,
    /// Published objects, ascending.
    #[serde(with = "crate::wire_id::many")]
    pub objects: Vec<ObjectId>,
    pub signed_at: String,
    pub signature: NodeSignature,
}

#[derive(Debug)]
pub enum InventoryShapeError {
    InvalidBaseUrl {
        value: String,
    },
    TooMany {
        count: usize,
    },
    /// Not strictly ascending: out of order or named twice.
    NotAscending {
        object: ObjectId,
    },
}

impl fmt::Display for InventoryShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBaseUrl { value } => write!(
                f,
                "base_url {value:?} is not an http(s) origin with no path or trailing slash"
            ),
            Self::TooMany { count } => write!(
                f,
                "an inventory lists at most {MAX_INVENTORY_OBJECTS} objects, not {count}"
            ),
            Self::NotAscending { object } => {
                write!(
                    f,
                    "objects must be strictly ascending; {object} is out of place"
                )
            }
        }
    }
}

impl std::error::Error for InventoryShapeError {}

impl Inventory {
    /// Build and sign `node_actor`'s inventory of `objects` as of
    /// `signed_at`. `objects` is sorted and deduplicated.
    pub fn signed(
        node_actor: ActorId,
        base_url: String,
        objects: impl IntoIterator<Item = ObjectId>,
        key: &SecretSigningKey,
        signed_at: Timestamp,
    ) -> Self {
        let objects: BTreeSet<ObjectId> = objects.into_iter().collect();
        let mut inventory = Self {
            schema: InventorySchema,
            node_actor,
            base_url,
            objects: objects.into_iter().collect(),
            signed_at: String::new(),
            signature: NodeSignature {
                key_id: String::new(),
                algorithm: String::new(),
                bytes: String::new(),
            },
        };
        inventory.sign(key, signed_at);
        inventory
    }

    /// (Re)sign the current fields.
    pub fn sign(&mut self, key: &SecretSigningKey, signed_at: Timestamp) {
        self.signed_at = signed_at.to_string();
        self.signature = sign_node_bytes(key, &self.signing_bytes(signed_at));
    }

    /// The bytes the signature covers (`schemas/canonical/inventory-v1.md`).
    fn signing_bytes(&self, signed_at: Timestamp) -> Vec<u8> {
        let mut out = Vec::new();
        encode_str(&mut out, INVENTORY_DOMAIN);
        encode_str(&mut out, InventorySchema::VALUE);
        encode_str(&mut out, self.node_actor.as_str());
        encode_str(&mut out, &self.base_url);
        encode_list(&mut out, &self.objects, |out, object| {
            encode_str(out, object.as_str());
        });
        signed_at.encode_canonical(&mut out);
        out
    }

    /// The base URL is usable and `objects` is strictly ascending and
    /// bounded, so one inventory has one encoding.
    pub fn validate(&self) -> Result<(), InventoryShapeError> {
        validate_base_url(&self.base_url).map_err(|_| InventoryShapeError::InvalidBaseUrl {
            value: self.base_url.clone(),
        })?;
        if self.objects.len() > MAX_INVENTORY_OBJECTS {
            return Err(InventoryShapeError::TooMany {
                count: self.objects.len(),
            });
        }
        for pair in self.objects.windows(2) {
            if pair[0] >= pair[1] {
                return Err(InventoryShapeError::NotAscending {
                    object: pair[1].clone(),
                });
            }
        }
        Ok(())
    }

    /// The signature is by `node_actor`'s key active at `signed_at`, not
    /// revoked then. Returns `signed_at`.
    pub fn verify_signature<R: ActorResolver>(
        &self,
        resolver: &R,
    ) -> Result<Timestamp, NodeDocumentError> {
        let signed_at = parse_signed_at(&self.signed_at)?;
        verify_node_bytes(
            resolver,
            &self.node_actor,
            signed_at,
            &self.signature,
            &self.signing_bytes(signed_at),
        )?;
        Ok(signed_at)
    }
}

/// Path of the relayed inventories, under [`crate::API_PREFIX`].
pub const RELAYED_INVENTORIES_PATH: &str = "/inventories";

schema_tag!(
    /// `"kairo.fed.relayed-inventories.v1"`.
    RelayedInventoriesSchema = "kairo.fed.relayed-inventories.v1"
);

/// One relayed inventory: the node's signed inventory, exactly as that
/// node served it, and its actor export so a receiver can verify a node
/// it has never contacted.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayedInventory {
    pub inventory: Box<serde_json::value::RawValue>,
    pub node: crate::ActorExport,
}

/// `GET /federation/v1/inventories` (`DECISIONS.md` §29 sub-decisions 2
/// and 4).
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayedInventories {
    pub schema: RelayedInventoriesSchema,
    pub inventories: Vec<RelayedInventory>,
}
