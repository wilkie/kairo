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
/// `GET /api/v1/capabilities/{grantor}` and
/// `GET /api/v1/capabilities/for-object/{object}`. One entry
/// per `(grantee, scope)` chain leaf — mirrors the shape
/// produced by `kairo capability list --grantor` in direct
/// mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CapabilityHeadDto {
    pub grantor: String,
    pub grantee: String,
    pub scope: CapabilityScopeJson,
    pub statement_id: String,
    pub created_at: String,
}

/// Version-tag head summary returned by
/// `GET /api/v1/version-tags/{object}`. Light shape, one entry
/// per `(actor, version)` chain leaf — callers who want the
/// signed tag follow up with
/// `GET /api/v1/version-tags/{object}/{version}` or
/// `GET /api/v1/statements/{statement_id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct VersionTagHeadDto {
    pub actor: String,
    pub object: String,
    pub version: String,
    pub statement_id: String,
    /// RFC 3339 UTC seconds.
    pub created_at: String,
}

/// Revision head summary returned by
/// `GET /api/v1/revisions/{object}`. Light shape, one entry
/// per `ObjectRevision` statement targeting the object —
/// callers who want the signed envelope follow up with
/// `GET /api/v1/statements/{statement_id}`.
///
/// `parents` carries the revision-id parent set (zero for
/// initial revisions, one for linear history, multiple for
/// merges); `manifest_hash` is the canonical blob id of the
/// revision's manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RevisionHeadDto {
    pub actor: String,
    pub object: String,
    pub revision_id: String,
    pub statement_id: String,
    pub parents: Vec<String>,
    pub manifest_hash: String,
    /// RFC 3339 UTC seconds.
    pub created_at: String,
}

/// One object created by an actor, returned by
/// `GET /api/v1/actors/{id}/objects`. Backed by the per-actor
/// materialized index in the store
/// (`objects_by_actor/<XX>/<YY>/<actor-id>.json`), so the call is
/// O(entries) — no full objects-tree scan even on large stores.
///
/// `object_kind` is the genesis body's `ObjectKind::as_str()`
/// discriminator (e.g. `"kairo/object"`, `"software"`),
/// denormalized into the index at write time so the inspector
/// can render the kind tag without a per-row genesis fetch. The
/// kind is fixed at genesis time, so denormalization is safe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ObjectByActorDto {
    pub actor: String,
    pub object_id: String,
    pub object_kind: String,
    /// RFC 3339 UTC seconds.
    pub created_at: String,
}

/// One signed statement authored by an actor, returned by
/// `GET /api/v1/actors/{id}/statements`. Backed by the per-actor
/// materialized index in the store
/// (`statements_by_actor/<XX>/<YY>/<actor-id>.json`), so the call is
/// O(entries) — no statement-tree scan even on large stores.
///
/// `kind` is the `StatementKind::as_str()` discriminator (e.g.
/// `"ObjectBranch"`, `"ActorTrust"`); callers either branch on it
/// directly or follow up with `GET /api/v1/statements/{statement_id}`
/// for the full envelope. `ObjectGenesis` is intentionally absent —
/// genesis carries `created_by` rather than the envelope `actor` field
/// every other statement type uses; the inspector folds it in
/// client-side via the actor's owned-objects view if needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StatementByActorDto {
    pub actor: String,
    pub statement_id: String,
    pub kind: String,
    /// RFC 3339 UTC seconds.
    pub created_at: String,
}

/// Trust head summary returned by
/// `GET /api/v1/trust/about/{actor}`. One entry per `by_actor`
/// chain leaf — callers who want the signed `ActorTrust` body
/// follow up with `GET /api/v1/trust/{by}/{of}` or
/// `GET /api/v1/statements/{statement_id}`.
///
/// `decision` is `"trusted"`, `"untrusted"`, or `null` (the
/// latter encodes a withdrawal — first-person "no opinion",
/// distinct from "never expressed an opinion").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TrustHeadDto {
    pub by_actor: String,
    pub trusted_actor: String,
    pub statement_id: String,
    /// RFC 3339 UTC seconds.
    pub created_at: String,
    pub decision: Option<String>,
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
///
/// `write_surface` lists the statement kinds the daemon's
/// `POST /api/v1/statements` dispatcher will accept. Added in
/// Slice 5 of `PHASE_2_DAEMON_WRITES.md` to surface Phase A's
/// expansion to write-mode without changing `daemon_running`
/// semantics. `#[serde(default)]` keeps older client builds
/// compatible with newer daemons (the field decodes as an empty
/// list); newer clients reading older daemons see the same.
///
/// `auth_mode` advertises the daemon's request-auth posture for
/// the write surface (`POST /api/v1/statements`). Phase A
/// daemons reported nothing — the field defaults to
/// `"filesystem-perms"` on absent for forward-compat with older
/// clients. Phase B daemons set it to `"signed-request"`,
/// matching `DECISIONS.md §14`'s `KairoActor1` envelope. The
/// read surface is always unauthenticated regardless.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StatusInfo {
    pub daemon_running: bool,
    pub store_path: String,
    pub store_schema_version: String,
    pub pid: u32,
    pub daemon_version: String,
    #[serde(default)]
    pub write_surface: Vec<String>,
    #[serde(default = "default_auth_mode")]
    pub auth_mode: String,
}

fn default_auth_mode() -> String {
    "filesystem-perms".to_string()
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

/// Response payload returned by `POST /api/v1/statements`.
///
/// Echoes the persisted statement back to the caller so write
/// flows don't need a follow-up read to discover the canonical
/// `statement_id` or to confirm the daemon stored the body
/// byte-for-byte. The `statement` field carries the same
/// polymorphic JSON shape the daemon's read endpoints return —
/// callers decode it against the typed `*StatementJson` of the
/// kind they sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PutStatementResult {
    pub statement_id: String,
    pub statement: StatementValue,
}

/// One entry in the response from
/// `GET /api/v1/keystore/actors` — the local keystore's listing
/// of actors with private keys available for daemon-side signing.
///
/// `display_name` is intentionally absent in v1; per
/// `PHASE_2_WEB_CLIENT_AUTH.md` §1.2 it lives in a future
/// `ActorProfile` statement and is additive when it lands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct KeystoreActorDto {
    pub actor_id: String,
}

/// Response body for `GET /api/v1/keystore/actors`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct KeystoreActorsResponse {
    pub actors: Vec<KeystoreActorDto>,
}

/// Request body for `POST /api/v1/sessions`. Names the actor the
/// session will be bound to; the daemon verifies the local
/// keystore has a usable (non-revoked-active-key) signing key for
/// the actor before opening the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OpenSessionRequest {
    pub actor_id: String,
}

/// Session metadata returned by `POST /api/v1/sessions` and
/// `GET /api/v1/sessions/current`.
///
/// `idle_expires_at` is the sliding deadline reset by every
/// authenticated touch; `expires_at` is the absolute hard cap
/// from `created_at`. The frontend may pre-emptively re-auth as
/// either deadline approaches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SessionInfo {
    pub actor_id: String,
    /// RFC 3339 UTC seconds.
    pub created_at: String,
    /// RFC 3339 UTC seconds. Absolute hard cap.
    pub expires_at: String,
    /// RFC 3339 UTC seconds. Sliding idle deadline.
    pub idle_expires_at: String,
}
