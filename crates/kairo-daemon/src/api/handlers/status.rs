//! `GET /api/v1/status` — returns the daemon's v1 status fields:
//! daemon-running, store path, store schema version, PID, daemon
//! version. Federation/runtime/task fields are post-v1 (see
//! `specs/API.md` §11.2 v1 surface note).

use axum::extract::State;

use crate::api::dto::StatusInfo;
use crate::api::envelope::ApiResult;
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
    })
}
