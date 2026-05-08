//! Wire DTOs shared by the daemon and its Rust clients.
//!
//! The client crate owns these types so there is one definition
//! per response shape; the daemon (which serializes) and the
//! client (which deserializes) both import from here. Adding a
//! field anywhere else would mean two definitions to keep in
//! sync, which is exactly the foot-gun this layout avoids.
//!
//! New endpoint shapes added in slices 5–7 land here and the
//! daemon's handlers serialize them.

use serde::{Deserialize, Serialize};

// Re-export the canonical JSON wrapper types from upstream so
// callers don't need to depend on `kairo-identity` /
// `kairo-statement` directly. The wire format matches what the
// daemon serves in the success envelope's `result` field.
pub use kairo_identity::json::ActorGenesisJson;
pub use kairo_statement::json::{
    ActorTrustStatementJson, CapabilityScopeJson, ObjectBranchStatementJson,
    ObjectGenesisStatementJson, ObjectVersionTagStatementJson,
};

/// Polymorphic statement payload: any signed statement variant
/// the store keeps under `statements/`. The kind discriminator
/// is inside the JSON (each `*StatementJson` shape has a `body`
/// with a tagged kind), so callers either match on the embedded
/// type field or feed the bytes back into `serde_json::from_value`
/// against the typed shape they expect.
pub type StatementValue = serde_json::Value;

/// Branch tip summary returned by `GET /api/v1/branches/{object}`.
///
/// Light shape — just identity fields. Callers who need the full
/// `ObjectBranchStatementJson` follow up with
/// `GET /api/v1/statements/{statement_id}` (or
/// `GET /api/v1/branches/{object}/{name}/latest` for the
/// resolved head).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchTipDto {
    pub actor: String,
    pub object: String,
    pub name: String,
    pub statement_id: String,
    /// RFC 3339 UTC seconds.
    pub created_at: String,
}

/// Capability head summary returned by
/// `GET /api/v1/capabilities/{grantor}`. One entry per
/// `(grantee, scope)` chain leaf — mirrors the shape produced
/// by `kairo capability list --grantor` in direct mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityHeadDto {
    pub grantor: String,
    pub grantee: String,
    pub scope: CapabilityScopeJson,
    pub statement_id: String,
    pub created_at: String,
}

/// Response body for `GET /api/v1/version`.
///
/// All four fields are crate-version strings (semver), drawn
/// from the corresponding `Cargo.toml` package versions at
/// build time. `api_version` is the URL-prefix version, not a
/// crate version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionInfo {
    pub daemon_version: String,
    pub api_version: String,
    pub core_version: String,
    pub store_version: String,
}

/// Response body for `GET /api/v1/status`.
///
/// V1 fields only: federation, runtime, and task counts are
/// omitted because those subsystems are post-v1 (see
/// `specs/API.md` §11.2 v1 surface note).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusInfo {
    pub daemon_running: bool,
    pub store_path: String,
    pub store_schema_version: String,
    pub pid: u32,
    pub daemon_version: String,
}
