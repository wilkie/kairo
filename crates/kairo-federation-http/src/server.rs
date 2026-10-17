//! TCP bind and graceful shutdown.

use std::future::Future;

use tokio::net::TcpListener;

use crate::error::Error;
use crate::node::Node;
use crate::Config;

/// Run until `SIGTERM` / `SIGINT`.
pub async fn serve(config: Config) -> Result<(), Error> {
    serve_with_shutdown(config, shutdown_signal()).await
}

/// Run until `shutdown` resolves. Tests pass their own future.
pub async fn serve_with_shutdown(
    config: Config,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), Error> {
    let node = Node::open(&config)?;
    let listener = TcpListener::bind(config.bind_addr)
        .await
        .map_err(|source| Error::Bind {
            addr: config.bind_addr,
            source,
        })?;
    let local = listener.local_addr().map_err(Error::Serve)?;
    if !local.ip().is_loopback() {
        tracing::warn!(
            bind = %local,
            "binding a non-loopback address; this process serves plain HTTP and expects a \
             TLS-terminating reverse proxy in front of it (DECISIONS.md §16 sub-decision 6)"
        );
    }
    tracing::info!(
        bind = %local,
        base_url = %node.document().base_url,
        node_actor = %node.document().node_actor,
        "kairo-federation-http listening"
    );
    axum::serve(listener, node.router())
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(Error::Serve)
}

async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};

    let (Ok(mut terminate), Ok(mut interrupt)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) else {
        tracing::warn!("could not install signal handlers; stop with SIGKILL");
        return std::future::pending().await;
    };
    tokio::select! {
        _ = terminate.recv() => {}
        _ = interrupt.recv() => {}
    }
    tracing::info!("kairo-federation-http shutting down");
}
