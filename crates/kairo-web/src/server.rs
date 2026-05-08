//! Server lifecycle: TCP bind, router composition, graceful
//! shutdown on `SIGTERM` / `SIGINT`.
//!
//! Mirrors `kairo_daemon::server` in shape but binds a TCP
//! listener instead of a Unix socket, and validates that the
//! address is loopback-only per `DECISIONS.md` §12.3.

use std::future::Future;
use std::net::IpAddr;
use std::path::Path;
use std::path::PathBuf;

use axum::routing::any;
use axum::Router;
use hyper::body::Incoming;
use hyper::Request;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use hyper_util::server::graceful::GracefulShutdown;
use tokio::net::{TcpListener, TcpStream};
use tokio::signal::unix::{signal, SignalKind};
use tokio::time::{timeout, Duration};
use tower::Service;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::error::Error;
use crate::proxy::{self, ProxyState};
use crate::Config;

/// Maximum time we'll wait for in-flight connections to finish
/// after the shutdown signal arrives. Same bound as the daemon —
/// the web server has no longer-running responses than the daemon
/// itself does.
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

/// Build the axum router used by `serve` and by tests that drive
/// the router directly without binding a TCP listener.
///
/// Layered, in order of precedence:
///
/// 1. `/api/v1/*path` — reverse proxy to the daemon Unix socket.
/// 2. Anything else — static SPA bundle served by `ServeDir` with
///    an `index.html` fallback so HTML5 history-mode routes work.
pub fn router(spa_dir: &Path, daemon_socket: PathBuf) -> Router {
    let proxy_state = ProxyState::new(daemon_socket);

    let serve_dir = ServeDir::new(spa_dir).fallback(tower_http::services::ServeFile::new(
        spa_dir.join("index.html"),
    ));

    Router::new()
        .route("/api/v1", any(proxy::handler))
        .route("/api/v1/*rest", any(proxy::handler))
        .with_state(proxy_state)
        .fallback_service(serve_dir)
        .layer(TraceLayer::new_for_http())
}

/// Run the web server to completion. Shuts down gracefully on the
/// first `SIGTERM` or `SIGINT`.
pub async fn serve(config: Config) -> Result<(), Error> {
    serve_with_shutdown(config, default_shutdown_signal()).await
}

/// Run the web server, shutting down when `shutdown` resolves.
/// Used by tests to drive lifecycle without sending real signals.
pub async fn serve_with_shutdown<F>(config: Config, shutdown: F) -> Result<(), Error>
where
    F: Future<Output = ()> + Send + 'static,
{
    validate_loopback(&config.bind_addr)?;
    validate_spa_dir(&config.spa_dir)?;
    validate_daemon_socket(&config.daemon_socket)?;

    let listener =
        TcpListener::bind(&config.bind_addr)
            .await
            .map_err(|source| Error::Bind {
                addr: config.bind_addr,
                source,
            })?;
    let bound = listener
        .local_addr()
        .map_err(|source| Error::Bind {
            addr: config.bind_addr,
            source,
        })?;

    let pid_guard = match config.pid_file.as_deref() {
        Some(path) => Some(PidFile::write(path)?),
        None => None,
    };

    let app = router(&config.spa_dir, config.daemon_socket.clone());

    tracing::info!(
        bind = %bound,
        spa_dir = %config.spa_dir.display(),
        daemon_socket = %config.daemon_socket.display(),
        pid_file = ?config.pid_file.as_deref().map(Path::display),
        "kairo-web listening"
    );

    let result = run_accept_loop(listener, app, shutdown).await;

    // `Drop` on the guard removes the PID file; held explicitly
    // so it survives the await above even on panic-during-shutdown.
    drop(pid_guard);

    tracing::info!(bind = %bound, "kairo-web shut down");

    result
}

/// PID file that unlinks itself on drop. Created with an atomic
/// rename so a partial write is never observable. Mirrors
/// `kairo_daemon::server::PidFile`.
struct PidFile {
    path: PathBuf,
}

impl PidFile {
    fn write(path: &Path) -> Result<Self, Error> {
        let pid = std::process::id();
        let tmp_path = path.with_extension(format!("pid.tmp.{pid}"));
        std::fs::write(&tmp_path, format!("{pid}\n")).map_err(|error| Error::PidIo {
            path: tmp_path.clone(),
            source: error,
        })?;
        std::fs::rename(&tmp_path, path).map_err(|error| Error::PidIo {
            path: path.to_path_buf(),
            source: error,
        })?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    path = %self.path.display(),
                    error = %error,
                    "failed to unlink PID file on shutdown",
                );
            }
        }
    }
}

/// Validate that `addr` is a loopback address. v1 binds
/// 127.0.0.0/8 or ::1 only; remote exposure lands with the
/// post-v1 bearer-token / TLS work.
fn validate_loopback(addr: &std::net::SocketAddr) -> Result<(), Error> {
    let is_loopback = match addr.ip() {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback(),
    };
    if is_loopback {
        Ok(())
    } else {
        Err(Error::NonLoopbackBind { addr: *addr })
    }
}

fn validate_spa_dir(path: &Path) -> Result<(), Error> {
    if !path.exists() {
        return Err(Error::SpaDirInvalid {
            path: path.to_path_buf(),
            reason: "path does not exist".to_owned(),
        });
    }
    if !path.is_dir() {
        return Err(Error::SpaDirInvalid {
            path: path.to_path_buf(),
            reason: "path is not a directory".to_owned(),
        });
    }
    Ok(())
}

fn validate_daemon_socket(path: &Path) -> Result<(), Error> {
    if !path.exists() {
        return Err(Error::DaemonSocketMissing {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

/// Per-connection accept loop. axum 0.7's `axum::serve` accepts
/// `TcpListener` directly, but the daemon's manual loop pattern
/// keeps shutdown / drain semantics identical between the two
/// processes.
async fn run_accept_loop<F>(listener: TcpListener, app: Router, shutdown: F) -> Result<(), Error>
where
    F: Future<Output = ()> + Send + 'static,
{
    let graceful = GracefulShutdown::new();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => {
                tracing::info!("shutdown signal received; draining in-flight requests");
                break;
            }
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _addr)) => spawn_connection(stream, app.clone(), &graceful),
                    Err(error) => {
                        tracing::warn!(error = %error, "accept(2) failed");
                    }
                }
            }
        }
    }

    match timeout(SHUTDOWN_DRAIN_TIMEOUT, graceful.shutdown()).await {
        Ok(()) => Ok(()),
        Err(_) => {
            tracing::warn!(
                timeout_secs = SHUTDOWN_DRAIN_TIMEOUT.as_secs(),
                "drain timeout exceeded; abandoning in-flight connections"
            );
            Ok(())
        }
    }
}

fn spawn_connection(stream: TcpStream, app: Router, graceful: &GracefulShutdown) {
    let io = TokioIo::new(stream);
    let hyper_service = hyper::service::service_fn(move |req: Request<Incoming>| {
        let mut app = app.clone();
        async move { app.call(req).await }
    });

    let conn = auto::Builder::new(TokioExecutor::new())
        .serve_connection(io, hyper_service)
        .into_owned();
    let watched = graceful.watch(conn);

    tokio::spawn(async move {
        if let Err(error) = watched.await {
            tracing::warn!(error = %error, "connection terminated with error");
        }
    });
}

async fn default_shutdown_signal() {
    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(stream) => stream,
        Err(error) => {
            tracing::error!(error = %error, "failed to install SIGTERM handler; shutdown will only respond to SIGINT");
            futures_pending().await
        }
    };
    let mut sigint = match signal(SignalKind::interrupt()) {
        Ok(stream) => stream,
        Err(error) => {
            tracing::error!(error = %error, "failed to install SIGINT handler; shutdown will only respond to SIGTERM");
            futures_pending().await
        }
    };

    tokio::select! {
        _ = sigterm.recv() => tracing::info!("received SIGTERM"),
        _ = sigint.recv() => tracing::info!("received SIGINT"),
    }
}

async fn futures_pending() -> tokio::signal::unix::Signal {
    std::future::pending().await
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

    #[test]
    fn validates_ipv4_loopback() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7878);
        assert!(validate_loopback(&addr).is_ok());
    }

    #[test]
    fn validates_ipv4_loopback_other_octet() {
        // 127.0.0.0/8 is loopback, not just 127.0.0.1.
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 99, 99, 99)), 7878);
        assert!(validate_loopback(&addr).is_ok());
    }

    #[test]
    fn validates_ipv6_loopback() {
        let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 7878);
        assert!(validate_loopback(&addr).is_ok());
    }

    #[test]
    fn rejects_ipv4_non_loopback() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 7878);
        let err = validate_loopback(&addr).expect_err("expected error");
        assert!(matches!(err, Error::NonLoopbackBind { .. }));
    }

    #[test]
    fn rejects_routable_address() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)), 7878);
        let err = validate_loopback(&addr).expect_err("expected error");
        assert!(matches!(err, Error::NonLoopbackBind { .. }));
    }
}
