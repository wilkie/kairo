pub mod actors;
pub mod blobs;
pub mod branches;
pub mod capabilities;
pub mod federation;
pub mod keystore;
pub mod objects;
pub mod provenance;
pub mod revisions;
pub mod sessions;
pub mod statements;
pub mod status;
pub mod trust;
pub mod verify_object;
pub mod version;
pub mod version_tags;

/// Response header naming what a read-triggered acquisition did
/// (`specs/DECISIONS.md` §30 sub-decision 4), on a `404`.
pub(crate) const ACQUISITION_HEADER: &str = "kairo-acquisition";

/// Queue an acquisition of `object` if reads may (`on_read`), off the
/// async runtime. `None` when they may not.
pub(crate) async fn acquire_on_read(
    state: &crate::api::state::AppState,
    object: &kairo_core::ObjectId,
) -> Option<crate::federation::Acquisition> {
    let federation = state.federation.clone();
    let object = object.clone();
    tokio::task::spawn_blocking(move || federation.on_read(&object))
        .await
        .ok()
        .flatten()
}

/// `response`, with the acquisition header when one was requested.
pub(crate) fn with_acquisition(
    mut response: axum::response::Response,
    acquisition: Option<crate::federation::Acquisition>,
) -> axum::response::Response {
    if let Some(acquisition) = acquisition {
        response.headers_mut().insert(
            ACQUISITION_HEADER,
            axum::http::HeaderValue::from_static(acquisition.as_str()),
        );
    }
    response
}
