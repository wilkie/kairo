//! `GET /api/v1/blobs/{blob_id}` — stream the blob's bytes
//! verbatim with chunked transfer encoding. The daemon does
//! **not** materialize the full blob in memory: a
//! `tokio::fs::File` is wrapped in `tokio_util::io::ReaderStream`
//! and handed to axum's `Body::from_stream`, so backpressure
//! flows from the socket back to the read loop.
//!
//! Errors get the JSON envelope (400 / 404 / 500). Successful
//! responses use `application/octet-stream` and bypass the
//! envelope — the body is the raw bytes.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kairo_core::BlobId;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

use crate::api::envelope::ApiError;
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

/// Chunk size for `ReaderStream`. 64 KiB is hyper-util's
/// recommended default for HTTP/1 streaming and matches typical
/// kernel readahead — small enough that one slow client doesn't
/// pin a full blob in memory; large enough that syscall overhead
/// stays in the noise.
const STREAM_CHUNK_BYTES: usize = 64 * 1024;

#[utoipa::path(
    get,
    path = "/api/v1/blobs/{id}",
    tag = "blobs",
    operation_id = "getBlob",
    params(
        ("id" = String, Path, description = "Blob id (kairo:blob:...)"),
    ),
    responses(
        (
            status = 200,
            description = "Raw blob bytes streamed with chunked transfer encoding",
            content_type = "application/octet-stream",
            body = [u8],
        ),
        (status = 400, description = "Malformed blob id"),
        (status = 404, description = "Blob not found"),
    ),
)]
pub async fn handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let blob_id: BlobId = id
        .parse()
        .map_err(|error| ApiError::bad_request(format!("invalid blob id {id:?}: {error}")))?;

    // Open the file on a blocking task — `std::fs::File::open`
    // is a syscall.
    let store = state.store.clone();
    let std_file = tokio::task::spawn_blocking(move || store.open_blob(&blob_id))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, "open_blob"))?;

    // Hand the open file to tokio's async runtime. `from_std`
    // does not re-stat or seek; the descriptor is reused.
    let file = File::from_std(std_file);

    // `metadata` lets us emit Content-Length so clients can show
    // progress. tokio::fs::File::metadata is async (uses
    // spawn_blocking under the hood).
    let metadata = file
        .metadata()
        .await
        .map_err(|error| ApiError::store(format!("blob metadata read failed: {error}")))?;
    let content_length = metadata.len();

    let stream = ReaderStream::with_capacity(file, STREAM_CHUNK_BYTES);
    let body = Body::from_stream(stream);

    Ok((
        StatusCode::OK,
        [
            (CONTENT_TYPE, "application/octet-stream"),
            (CONTENT_LENGTH, &content_length.to_string()),
        ],
        body,
    )
        .into_response())
}
