//! `GET /api/v1/keystore/actors` — list the actor IDs the local
//! keystore can sign for.
//!
//! The endpoint is a directory listing of `~/.kairo/keys/`
//! mapped onto a wire DTO. No statement reads, no
//! `ActorGenesis` walks — `display_name` is intentionally
//! absent in v1 (`PHASE_2_WEB_CLIENT_AUTH.md` §1.2).
//!
//! The handler is anonymous: anyone who can reach the daemon's
//! Unix socket can list the local keystore's actor IDs. The
//! trust boundary is the socket itself; see §2.2 of the auth
//! slice spec.

use axum::extract::State;
use kairo_keystore::Keystore;

use crate::api::dto::KeystoreActorDto;
use crate::api::dto::KeystoreActorsResponse;
use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;

#[utoipa::path(
    get,
    path = "/api/v1/keystore/actors",
    tag = "keystore",
    operation_id = "listKeystoreActors",
    responses(
        (status = 200, description = "Actors with a signing key in the local keystore", body = KeystoreActorsResponse),
        (status = 500, description = "Keystore unreadable (perms, IO)"),
    ),
)]
pub async fn list_actors_handler(
    State(state): State<AppState>,
) -> Result<ApiResult<KeystoreActorsResponse>, ApiError> {
    let keystore = state.keystore.clone();
    let ids = tokio::task::spawn_blocking(move || keystore.list_actors())
        .await
        .map_err(|join| ApiError::internal(format!("keystore listing task: {join}")))?
        .map_err(|error| ApiError::internal(format!("keystore listing: {error}")))?;

    let mut actors: Vec<KeystoreActorDto> = ids
        .into_iter()
        .map(|id| KeystoreActorDto {
            actor_id: id.to_string(),
        })
        .collect();
    // Stable order so the frontend doesn't shuffle the picker
    // between requests. Keystore::list_actors documents that
    // order is not guaranteed.
    actors.sort_by(|a, b| a.actor_id.cmp(&b.actor_id));

    Ok(ApiResult(KeystoreActorsResponse { actors }))
}
