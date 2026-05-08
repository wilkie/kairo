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
use utoipa::ToSchema;

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StatusInfo {
    pub daemon_running: bool,
    pub store_path: String,
    pub store_schema_version: String,
    pub pid: u32,
    pub daemon_version: String,
}

/// Validation status summarizing a `verify-object` result.
///
/// Mirrors `specs/WEB_CLIENT.md` §10. Daemon v1 emits `valid`,
/// `invalid`, and `indeterminate`; `conflicted` (multi-actor
/// disagreement) and `unverified` (caller-skipped) are reserved
/// for future use so the wire enum is stable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Valid,
    Invalid,
    Conflicted,
    Indeterminate,
    Unverified,
}

/// Severity of a single validation issue (`specs/API.md` §28).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ValidationIssueSeverity {
    Info,
    Warning,
    Error,
}

/// One concrete finding from a `verify-object` run.
///
/// `kind` is a wire-stable identifier callers can switch on;
/// `message` is a human-readable explanation. `statement_id` and
/// `actor_id` are populated when the issue refers to a specific
/// statement / actor; `details` is reserved for future structured
/// payloads (mirroring `specs/API.md` §28's open-ended `details`
/// field). Daemon v1 always emits `details` as an empty object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ValidationIssue {
    pub kind: String,
    pub severity: ValidationIssueSeverity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    #[schema(value_type = Object)]
    pub details: serde_json::Value,
}

/// Response body for `GET /api/v1/verify-object/{id}`.
///
/// Combines the worst-of-fold `status` with the underlying issue
/// list. The genesis statement is identified by `object_id`
/// itself (the object id is the genesis's content address); the
/// revision / branch fields are populated when a head branch tip
/// could be resolved. Daemon v1 cannot prove `valid` for objects
/// with revisions because it lacks closure data (manifest, Git
/// commit) — the practical outcomes are `valid` (genesis-only)
/// and `indeterminate` (signature verifies, content layer
/// unprovable). `invalid` lights up when a check fails outright.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ValidationResult {
    pub object_id: String,
    pub status: ValidationStatus,
    pub issues: Vec<ValidationIssue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_statement_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_name: Option<String>,
}
