//! `GET /api/v1/objects/{object_id}` — returns the object's
//! genesis statement JSON.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kairo_core::ObjectId;
use kairo_statement::json::ObjectGenesisStatementJson;
use kairo_store::ObjectStore;

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::handlers::{acquire_on_read, with_acquisition};
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
pub async fn handler(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let object_id: ObjectId = match id.parse() {
        Ok(object_id) => object_id,
        Err(error) => {
            return ApiError::bad_request(format!("invalid object id {id:?}: {error}"))
                .into_response()
        }
    };

    let store = state.store.clone();
    let lookup = object_id.clone();
    let read = tokio::task::spawn_blocking(move || store.get_object_genesis(&lookup))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))
        .and_then(|read| read.map_err(|error| map_store_error(error, "get_object_genesis")));
    match read {
        Ok(statement) => {
            ApiResult(ObjectGenesisStatementJson::from_statement(&statement)).into_response()
        }
        // Not held: a read may queue an acquisition (DECISIONS.md §30).
        Err(error) if error.status == StatusCode::NOT_FOUND => {
            let acquisition = acquire_on_read(&state, &object_id).await;
            with_acquisition(error.into_response(), acquisition)
        }
        Err(error) => error.into_response(),
    }
}
