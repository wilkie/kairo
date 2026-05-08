//! `GET /api/v1/statements/{statement_id}` — returns the
//! statement's JSON envelope by id, polymorphic across
//! statement kinds.
//!
//! Reads raw JSON bytes from the store via
//! `FilesystemStore::get_statement_bytes`, parses to a
//! `serde_json::Value` so axum can re-serialize them inside
//! the success envelope. We don't decode into a typed shape
//! here — the kind discriminator is inside the body, and
//! callers either match on it or feed the value back into a
//! typed `serde_json::from_value`.

use axum::extract::{Path, State};
use kairo_core::StatementId;
use serde_json::Value;

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

pub async fn handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResult<Value>, ApiError> {
    let statement_id: StatementId = id.parse().map_err(|error| {
        ApiError::bad_request(format!("invalid statement id {id:?}: {error}"))
    })?;

    let store = state.store.clone();
    let bytes = tokio::task::spawn_blocking(move || store.get_statement_bytes(&statement_id))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, "get_statement_bytes"))?;

    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        ApiError::store(format!("statement JSON parse failed: {error}"))
    })?;

    Ok(ApiResult(value))
}
