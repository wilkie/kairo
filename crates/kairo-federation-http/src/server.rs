//! TCP bind and graceful shutdown.

use std::future::Future;

use tokio::net::TcpListener;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use kairo_core::ActorId;

use crate::error::Error;
use crate::node::Node;

/// Default port: one above `kairo-web`'s 7878 family, easy to tell
/// apart in `ss -ltn`.
pub const DEFAULT_PORT: u16 = 7880;

/// Default bind: loopback, because TLS terminates at a reverse proxy
/// in front of this process (§16 sub-decision 6).
pub const DEFAULT_BIND: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_PORT);

/// How long a computed view of the published closures is reused
/// before recomputing. Bounds both the cost of a pull's many
/// per-record requests and how long an `unpublish` takes to stop
/// serving.
pub const DEFAULT_VIEW_TTL: Duration = Duration::from_secs(5);

/// Resolved server configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Store root to serve from. Opened read-only in practice: the
    /// server calls no `put_*`.
    pub store: PathBuf,
    pub bind_addr: SocketAddr,
    /// Public base URL peers reach this node at, e.g.
    /// `https://archive.example.org`. Goes into the node document.
    pub base_url: String,
    /// The node's own actor. Must be in the store with
    /// `actor_kind = "node"`.
    pub node_actor: ActorId,
    /// See [`DEFAULT_VIEW_TTL`]. Zero recomputes on every request.
    pub view_ttl: Duration,
}

/// Install the structured-text tracing subscriber on stderr, matching
/// `kairo-daemon` and `kairo-web`.
pub fn install_tracing() {
    use tracing_subscriber::fmt;
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .try_init();
}

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
