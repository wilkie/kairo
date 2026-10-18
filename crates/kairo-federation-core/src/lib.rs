//! `kairo-federation-core` — protocol-independent wire types for
//! Kairo federation.
//!
//! These are the JSON shapes a federation node serves and a
//! federation client parses for the object-pull MVP
//! (`specs/DECISIONS.md` §16, `specs/PHASE_2_FEDERATION.md` §2).
//! The crate does no I/O: the HTTP server and client live in
//! `kairo-federation-http`.
//!
//! Parsing is the boundary. Every id deserializes into its
//! `kairo-core` newtype, every top-level document rejects unknown
//! fields, and each `schema` field accepts only its exact string, so
//! a value that deserializes is well-formed. Checks that need more
//! than shape (the node actor derives from the genesis it ships, a
//! base URL is usable) are `validate` methods.
//!
//! Statements and blobs themselves are not modeled here. A node
//! serves statements as exactly the `kairo-statement` JSON the store
//! holds (embedded verbatim in a statement batch), and blobs as raw
//! bytes.

mod batch;
mod error;
mod listing;
mod node;
mod page;
mod schema;
mod wire_id;

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests;

pub use batch::{
    BatchedStatement, StatementBatch, StatementBatchRequest, StatementBatchRequestError,
    StatementBatchRequestSchema, StatementBatchSchema, MAX_BATCH_IDS, STATEMENT_BATCH_PATH,
};
pub use error::{FedErrorBody, FedErrorCode, FedErrorEnvelope, ERROR_SCHEMA};
pub use listing::{
    ActorListing, ActorListingError, ActorListingSchema, GitAvailability, ObjectListing,
    ObjectListingSchema, PublishedObjects, PublishedObjectsSchema,
};
pub use node::{
    NodeDocument, NodeDocumentError, NodeDocumentSchema, NodeSignature, UnsignedNodeDocument,
    NODE_DOCUMENT_DOMAIN,
};
pub use page::{
    Cursor, CursorError, PageRequest, PageRequestError, DEFAULT_PAGE_LIMIT, MAX_CURSOR_LEN,
    MAX_PAGE_LIMIT,
};

/// The protocol version the MVP speaks. A node lists it in
/// `protocol_versions`; a client refuses a node that doesn't.
pub const PROTOCOL_VERSION: &str = "kairo-fed/1";

/// `supports` value for the object-pull endpoint set.
pub const SUPPORTS_OBJECT_PULL: &str = "object-pull";

/// `supports` value for `POST /federation/v1/statement-batch`
/// (`specs/DECISIONS.md` §24).
pub const SUPPORTS_STATEMENT_BATCH: &str = "statement-batch";

/// Path of the node document, relative to the node's base URL.
pub const WELL_KNOWN_PATH: &str = "/.well-known/kairo-node";

/// Prefix every MVP federation endpoint sits under.
pub const API_PREFIX: &str = "/federation/v1";
