//! `GET /api/v1/status` — returns the daemon's v1 status fields:
//! daemon-running, store path, store schema version, PID, daemon
//! version. Federation/runtime/task fields are post-v1 (see
//! `specs/API.md` §11.2 v1 surface note).
//!
//! `write_surface` (Slice 5 of `PHASE_2_DAEMON_WRITES.md`) lists
//! the statement kinds the daemon's write dispatcher accepts.
//! Sourced from `handlers::statements::SUPPORTED_WRITE_KINDS` so
//! status and dispatcher stay in lockstep.

use axum::extract::State;

use crate::api::dto::StatusInfo;
use crate::api::envelope::ApiResult;
use crate::api::handlers::statements::SUPPORTED_WRITE_KINDS;
use crate::api::state::AppState;

#[utoipa::path(
    get,
    path = "/api/v1/status",
    tag = "system",
    operation_id = "getStatus",
    responses(
        (status = 200, description = "Live daemon status (v1 fields only)", body = StatusInfo),
    ),
)]
pub async fn handler(State(state): State<AppState>) -> ApiResult<StatusInfo> {
    ApiResult(StatusInfo {
        daemon_running: true,
        store_path: state.store_path.display().to_string(),
        store_schema_version: kairo_store::SCHEMA_VERSION.to_owned(),
        pid: state.pid,
        daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
        write_surface: SUPPORTED_WRITE_KINDS.iter().map(|k| (*k).to_owned()).collect(),
        // Phase B (DECISIONS.md §14): the write surface requires
        // ed25519-signed requests with capability-statement authz.
        // Reads stay unauthenticated.
        auth_mode: "signed-request".to_owned(),
    })
}
