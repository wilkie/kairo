//! HTTP API surface — router construction and the handler tree.
//!
//! All endpoints live under `/api/v1/`. The router is exposed
//! as a public function so tests (and a future Rust web-server
//! impl) can mount it on any transport without going through the
//! socket-bound `serve` path.

use axum::routing::get;
use axum::Router;
use tower_http::trace::TraceLayer;

pub mod dto;
pub mod envelope;
pub mod handlers;
pub mod state;

pub use state::AppState;

/// Build the v1 router with the supplied state.
///
/// Tests call this directly to drive handlers without binding a
/// socket; `serve` calls it and serves the result over the Unix
/// socket.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/version", get(handlers::version::handler))
        .route("/api/v1/status", get(handlers::status::handler))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}
