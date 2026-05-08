//! `GET /api/v1/objects/{object_id}` — returns the object's
//! genesis statement JSON.

use axum::extract::{Path, State};
use kairo_core::ObjectId;
use kairo_statement::json::ObjectGenesisStatementJson;
use kairo_store::ObjectStore;

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

#[utoipa::path(
    get,
    path = "/api/v1/objects/{id}",
    tag = "objects",
    operation_id = "getObject",
    params(
        ("id" = String, Path, description = "Object id (kairo:object:...)"),
    ),
    responses(
        (status = 200, description = "Object genesis statement JSON", body = ObjectGenesisStatementJson),
        (status = 400, description = "Malformed object id"),
        (status = 404, description = "Object not found"),
    ),
)]
pub async fn handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResult<ObjectGenesisStatementJson>, ApiError> {
    let object_id: ObjectId = id
        .parse()
        .map_err(|error| ApiError::bad_request(format!("invalid object id {id:?}: {error}")))?;

    let store = state.store.clone();
    let statement = tokio::task::spawn_blocking(move || store.get_object_genesis(&object_id))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, "get_object_genesis"))?;

    Ok(ApiResult(ObjectGenesisStatementJson::from_statement(
        &statement,
    )))
}
