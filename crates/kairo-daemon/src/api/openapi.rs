//! OpenAPI document and `/api/v1/openapi.json` handler.
//!
//! The schema is the cross-language contract between the daemon
//! and consumers (web client, future Rust SDKs, third-party
//! tooling). It is built via `utoipa` annotations on handlers
//! and DTOs, served live at `GET /api/v1/openapi.json`, and
//! checked into the repo at `openapi/kairo-daemon.json` (kept in
//! sync via `kairo-daemon dump-openapi`).
//!
//! All non-streaming responses are wrapped in the result envelope
//! `{ "ok": true, "schema": "kairo.api.result.v1", "result": <T> }`
//! and errors in the error envelope
//! `{ "ok": false, "schema": "kairo.api.error.v1",  "error": {...} }`.
//! v1 path annotations declare `body = T` (the inner type) for
//! brevity; the envelope shape itself is documented at the doc
//! level here. Slice 6 (`api-client`) is the right time to switch
//! to typed envelopes if the client benefits from it.

use axum::response::Json;
use utoipa::openapi::{InfoBuilder, OpenApiBuilder};
use utoipa::OpenApi;

use crate::api::handlers;

/// OpenAPI schema source-of-truth. Adding a new handler means
/// listing it under `paths(...)` and any new DTOs under
/// `components(schemas(...))`.
#[derive(Debug, OpenApi)]
#[openapi(
    info(
        title = "kairo-daemon",
        description = "\
Read-only HTTP+JSON API over a local Kairo store.

All non-streaming responses are wrapped in an envelope:
`{ \"ok\": true, \"schema\": \"kairo.api.result.v1\", \"result\": <T> }`.
Errors use the same envelope with `ok: false` and an `error` body
carrying a stable `code` and a human `message` (see `specs/API.md` §8).
The `body = T` declarations on each path describe the *inner* result
type; clients are expected to unwrap the envelope before consuming.

The blob endpoint (`GET /api/v1/blobs/{id}`) bypasses the envelope
entirely and streams `application/octet-stream` bytes.\
        ",
        version = env!("CARGO_PKG_VERSION"),
    ),
    tags(
        (name = "system", description = "Daemon metadata (version, status)"),
        (name = "actors", description = "Actor genesis lookups"),
        (name = "objects", description = "Object genesis lookups"),
        (name = "statements", description = "Polymorphic statement-by-id lookups"),
        (name = "branches", description = "Branch tip listing and resolution"),
        (name = "version-tags", description = "Version-tag listing and chain-leaf resolution"),
        (name = "revisions", description = "Object revision history"),
        (name = "trust", description = "First-person trust opinions"),
        (name = "capabilities", description = "Capability head listings"),
        (name = "blobs", description = "Raw blob streaming"),
        (name = "verify", description = "Object verification"),
    ),
    paths(
        handlers::version::handler,
        handlers::status::handler,
        handlers::actors::handler,
        handlers::actors::list_statements_handler,
        handlers::actors::list_objects_handler,
        handlers::objects::handler,
        handlers::statements::handler,
        handlers::branches::list_handler,
        handlers::branches::latest_handler,
        handlers::version_tags::list_handler,
        handlers::version_tags::latest_handler,
        handlers::revisions::list_handler,
        handlers::trust::handler,
        handlers::trust::list_about_handler,
        handlers::capabilities::list_from_handler,
        handlers::capabilities::list_for_object_handler,
        handlers::blobs::handler,
        handlers::verify_object::handler,
    ),
    components(schemas(
        kairo_daemon_client::dto::VersionInfo,
        kairo_daemon_client::dto::StatusInfo,
        kairo_daemon_client::dto::BranchTipDto,
        kairo_daemon_client::dto::CapabilityHeadDto,
        kairo_daemon_client::dto::VersionTagHeadDto,
        kairo_daemon_client::dto::RevisionHeadDto,
        kairo_daemon_client::dto::TrustHeadDto,
        kairo_daemon_client::dto::StatementByActorDto,
        kairo_daemon_client::dto::ObjectByActorDto,
        kairo_daemon_client::dto::ValidationStatus,
        kairo_daemon_client::dto::ValidationIssueSeverity,
        kairo_daemon_client::dto::ValidationIssue,
        kairo_daemon_client::dto::ValidationResult,
        kairo_identity::json::ActorGenesisJson,
        kairo_identity::json::PublicKeyJson,
        kairo_statement::json::SignatureJson,
        kairo_statement::json::ObjectGenesisStatementJson,
        kairo_statement::json::ObjectGenesisBodyJson,
        kairo_statement::json::ObjectRevisionStatementJson,
        kairo_statement::json::ObjectRevisionBodyJson,
        kairo_statement::json::ObjectBranchStatementJson,
        kairo_statement::json::ObjectBranchBodyJson,
        kairo_statement::json::ObjectVersionTagStatementJson,
        kairo_statement::json::ObjectVersionTagBodyJson,
        kairo_statement::json::ActorTrustStatementJson,
        kairo_statement::json::ActorTrustBodyJson,
        kairo_statement::json::CapabilityScopeJson,
        kairo_statement::json::CapabilityConstraintJson,
        kairo_statement::json::CapabilityJson,
    )),
)]
pub struct ApiDoc;

impl ApiDoc {
    /// Returns the schema as a `utoipa::openapi::OpenApi`. The
    /// derive macro generates `openapi()`; this thin wrapper
    /// gives callers a stable name regardless of macro internals.
    pub fn schema() -> utoipa::openapi::OpenApi {
        let mut doc = ApiDoc::openapi();
        // Rewrite the auto-generated info block to use a builder
        // call site — preserves any future tweaks (e.g., contact
        // info) in one place.
        doc.info = InfoBuilder::from(doc.info).build();
        // Scrub any default servers utoipa might inject; the
        // daemon serves over a Unix socket so no `servers` block
        // makes sense here.
        OpenApiBuilder::from(doc).servers(None::<Vec<_>>).build()
    }

    /// Pretty-printed JSON form of the schema, suitable for
    /// writing to `openapi/kairo-daemon.json`. Trailing newline
    /// included so editors don't reflow it.
    pub fn pretty_json() -> Result<String, serde_json::Error> {
        let mut buf = serde_json::to_string_pretty(&Self::schema())?;
        buf.push('\n');
        Ok(buf)
    }
}

/// `GET /api/v1/openapi.json` — live schema. The daemon's source
/// of truth; the on-disk `openapi/kairo-daemon.json` is generated
/// from this and validated against it in CI.
pub async fn handler() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::schema())
}
