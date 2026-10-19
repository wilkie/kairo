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
    ActorTrustStatementJson, CapabilityConstraintJson, CapabilityScopeJson,
    ObjectBranchStatementJson, ObjectGenesisStatementJson, ObjectVersionTagStatementJson,
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
///
/// `statement_kinds`, `delegable`, and `constraints` are copied
/// from the head's `ActorCapabilityGrant` body; `revocation` is
/// the grantor's most-restrictive revocation of it, if any. These
/// are stored facts, not an evaluation: a head with no revocation
/// can still fail `evaluate_capability` (expiry, a broken chain
/// above the grantor). The four fields are `#[serde(default)]`
/// so clients built before they existed keep decoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CapabilityHeadDto {
    pub grantor: String,
    pub grantee: String,
    pub scope: CapabilityScopeJson,
    pub statement_id: String,
    pub created_at: String,
    #[serde(default)]
    pub statement_kinds: Vec<String>,
    #[serde(default)]
    pub delegable: bool,
    #[serde(default)]
    pub constraints: Vec<CapabilityConstraintJson>,
    #[serde(default)]
    pub revocation: Option<CapabilityRevocationDto>,
}

/// The revocation attached to a [`CapabilityHeadDto`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CapabilityRevocationDto {
    /// Statement id of the `ActorCapabilityRevocation`.
    pub statement_id: String,
    /// `true` invalidates the grant from inception
    /// (`CAPABILITIES.md` §6.3), not just from the revocation on.
    pub retroactive: bool,
    /// RFC 3339 UTC seconds.
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

/// One page of `GET /api/v1/objects/{id}/log`: revisions newest
/// first, walked along parent links from the object's refs or the
/// requested starting points (`specs/DECISIONS.md` §20).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RevisionLogPage {
    pub revisions: Vec<RevisionHeadDto>,
    /// Pass back as `cursor` for the next page; `null` at the end.
    pub next_cursor: Option<String>,
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
    /// The opinion's trust scope (`specs/DECISIONS.md` §18).
    pub scope: String,
    pub statement_id: String,
    /// RFC 3339 UTC seconds.
    pub created_at: String,
    pub decision: Option<String>,
}

/// One peer that served a record to this node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ProvenanceEntryDto {
    /// Base URL the record was fetched from.
    pub peer_url: String,
    /// The peer's node actor, authenticated by its signed node document.
    pub peer_actor: String,
    /// RFC 3339 UTC seconds.
    pub fetched_at: String,
}

/// Response body for `GET /api/v1/provenance/{record}`: which
/// federation peers served `record`, first-seen first
/// (`specs/DECISIONS.md` §16 sub-decision 8). `entries` is empty for
/// a record authored or imported locally. Provenance names only the
/// immediate peer; authorship is the signature's job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ProvenanceDto {
    /// The typed reference, e.g. `statement:<id>`.
    pub record: String,
    pub entries: Vec<ProvenanceEntryDto>,
}

/// One of an actor's chains (`specs/DECISIONS.md` §23).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ActorChainDto {
    /// `object:<id>`, `actor:<id>`, or `keys`.
    pub scope: String,
    /// The chain's current tips, ascending: the `previous` the actor's
    /// next statement in this scope names (at most 16; more merge over
    /// later writes).
    pub tips: Vec<String>,
}

/// Response body for `GET /api/v1/actors/{id}/chains`: what a client
/// that signs its own statements needs to set `previous`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ActorChainsDto {
    pub actor: String,
    /// Every scope the actor has statements in, ascending by scope.
    pub chains: Vec<ActorChainDto>,
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
    /// The actor chains the result depends on (`specs/DECISIONS.md`
    /// §23 sub-decision 8), complete or not. Any gap or contradiction
    /// also appears in `issues` and makes `status` `Indeterminate`.
    pub chains: Vec<ChainCompletenessDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_statement_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_name: Option<String>,
    /// What a read-triggered acquisition did when the object is
    /// incomplete and `[federation.on_demand] on_read` is on
    /// (`specs/DECISIONS.md` §30): `queued`, `running`, `cooling_down`,
    /// `no_sources`, or `disabled`. Absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acquisition: Option<String>,
}

/// One actor chain a verification consulted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ChainCompletenessDto {
    pub signer: String,
    /// `object:<id>` or `keys`.
    pub scope: String,
    /// Every statement the chain's held statements name is held.
    pub complete: bool,
    /// Named but not held: what a peer may have withheld.
    pub missing: Vec<String>,
    /// Statements whose `previous` names something outside the chain.
    pub contradictions: Vec<String>,
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

/// Session-mode payload for `POST /api/v1/statements` (per
/// `PHASE_2_WEB_CLIENT_AUTH.md` §2.3 "two payload shapes on
/// one endpoint"). Used when the request carries a
/// `kairo_session` cookie instead of `Authorization:
/// KairoActor1`.
///
/// `body_kind` matches the `type` discriminator the signed
/// envelopes carry (e.g. `"ActorTrust"`, `"ObjectRevision"`,
/// `"ObjectBranch"`). `body` is the kind's unsigned body fields
/// — the daemon supplies the envelope fields (`actor`,
/// `subject`, `created_at`), constructs the canonical bytes,
/// signs with the session's keystore key, and dispatches
/// through the same persistence path as the signed envelope
/// mode.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionWriteRequest {
    pub body_kind: String,
    #[schema(value_type = Object)]
    pub body: serde_json::Value,
}

/// `body` of a [`SessionWriteRequest`] with `body_kind =
/// "ObjectGenesis"`. Genesis has no envelope: its signature
/// covers the body itself, and the body carries the fields an
/// envelope would — so the daemon fills `created_by` (the session
/// actor), `created_at` (now), and a fresh 32-byte `nonce`, which
/// together with `object_kind` derive the new `ObjectId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SessionObjectGenesisBody {
    pub object_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_revision: Option<String>,
}

// --- federation (DECISIONS.md §28) -------------------------------------

/// One peer's part in a follow's last sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PeerOutcomeDto {
    pub peer_url: String,
    /// `fetched`, `not_published` (the peer answered 404), or `failed`.
    pub outcome: String,
    pub fetched: Option<u64>,
    pub already_held: Option<u64>,
    pub incomplete_chains: Option<u64>,
    pub flagged: Option<u64>,
    /// Why it failed.
    pub reason: Option<String>,
}

/// A follow and its last result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FollowStatusDto {
    /// `object:<id>` or `trust:<actor>:<scope>`.
    pub follow: String,
    /// A sync of it is running now.
    pub running: bool,
    pub added_at: String,
    pub last_attempt: Option<String>,
    pub last_success: Option<String>,
    /// Per-peer outcomes of the last attempt.
    pub peers: Vec<PeerOutcomeDto>,
}

/// One engine run, newest first. Operational state, not validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FederationRunDto {
    pub id: u64,
    /// `sync`, `inbox`, `discover`, or `fetch`.
    pub kind: String,
    /// The follow, for a sync.
    pub follow: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// `current` / `not_current` for a sync, counts for the inbox, or
    /// the error.
    pub result: Option<String>,
}

/// `GET /api/v1/federation/status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FederationStatusDto {
    pub enabled: bool,
    pub interval_secs: u64,
    pub inbox_secs: u64,
    pub max_concurrent: u64,
    pub follows: Vec<FollowStatusDto>,
    /// Unprocessed inbox entries.
    pub inbox_entries: u64,
    pub runs: Vec<FederationRunDto>,
}

/// `POST /api/v1/federation/sync` body: one follow, or none for all.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FederationSyncRequestDto {
    pub object: Option<String>,
    pub trust_by: Option<String>,
    pub scope: Option<String>,
}

/// `POST /api/v1/federation/sync` answer. Results arrive in status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FederationSyncStartedDto {
    pub started: Vec<String>,
    pub already_running: Vec<String>,
    /// Over `max_concurrent`; the scheduler starts them as slots free.
    pub deferred: Vec<String>,
}

/// `POST /api/v1/federation/fetch` body (`DECISIONS.md` §30).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FederationFetchRequestDto {
    pub object: String,
}

/// What an acquisition request did. Results arrive in the status as a
/// `fetch` run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FederationFetchDto {
    pub object: String,
    /// `queued`, `running`, `cooling_down`, `no_sources`, or `disabled`.
    pub acquisition: String,
}
