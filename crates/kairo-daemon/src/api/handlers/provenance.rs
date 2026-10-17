//! `GET /api/v1/provenance/{record}` — which federation peers served a
//! record (`specs/PHASE_2_FEDERATION.md` slice 5).
//!
//! `record` is a typed reference (`statement:<id>`, `object:<id>`,
//! `actor:<id>`, `blob:<id>`). The answer is local bookkeeping, kept
//! apart from the signed records' own JSON so a detail response stays
//! exactly what was signed; the inspector asks for it separately.

use axum::extract::{Path, State};
use kairo_core::KairoRef;
use kairo_daemon_client::dto::{ProvenanceDto, ProvenanceEntryDto};
use kairo_store::{ProvenanceStore, StoreError};

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

#[utoipa::path(
    get,
    path = "/api/v1/provenance/{record}",
    tag = "provenance",
    operation_id = "getProvenance",
    params(
        ("record" = String, Path, description = "Typed reference: statement:<id>, object:<id>, actor:<id>, or blob:<id>"),
    ),
    responses(
        (status = 200, description = "Peers that served the record; empty if it is local", body = ProvenanceDto),
        (status = 400, description = "Malformed or unsupported reference"),
    ),
)]
pub async fn handler(
    State(state): State<AppState>,
    Path(record): Path<String>,
) -> Result<ApiResult<ProvenanceDto>, ApiError> {
    let parsed: KairoRef = record.parse().map_err(|error| {
        ApiError::bad_request(format!("invalid record reference {record:?}: {error}"))
    })?;
    if matches!(parsed, KairoRef::ObjectSnapshot { .. }) {
        return Err(ApiError::bad_request(
            "snapshot references have no provenance".to_owned(),
        ));
    }

    let store = state.store.clone();
    let lookup = parsed.clone();
    let entries = tokio::task::spawn_blocking(move || store.get_provenance(&lookup))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error: StoreError| map_store_error(error, "get_provenance"))?;

    Ok(ApiResult(ProvenanceDto {
        record: parsed.to_string(),
        entries: entries
            .into_iter()
            .map(|entry| ProvenanceEntryDto {
                peer_url: entry.peer_url,
                peer_actor: entry.peer_actor.to_string(),
                fetched_at: entry.fetched_at.to_string(),
            })
            .collect(),
    }))
}
