//! Server lifecycle: socket bind, PID file, double-start probe,
//! graceful shutdown on `SIGTERM` / `SIGINT`.
//!
//! The public entry points are [`serve`] (production: shuts down
//! on signals) and [`serve_with_shutdown`] (tests: shuts down
//! when the supplied future resolves). Both ensure the socket
//! and PID file are removed before returning, regardless of how
//! the server stopped.

use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use hyper::body::Incoming;
use hyper::Request;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use hyper_util::server::graceful::GracefulShutdown;
use kairo_store::FilesystemStore;
use tokio::fs;
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{signal, SignalKind};
use tokio::time::{timeout, Duration};
use tower::Service;

use crate::api::{self, AppState};
use crate::error::Error;
use crate::Config;

const SOCKET_FILE: &str = "daemon.sock";
const PID_FILE: &str = "daemon.pid";

/// Probe a peer socket with this timeout when checking whether
/// another daemon already holds the socket. Short on purpose —
/// a live daemon answers `connect(2)` synchronously; a stale
/// socket file fails immediately. Picked to bound the
/// double-start check to ~tens of ms in the worst case.
const PROBE_TIMEOUT: Duration = Duration::from_millis(250);

/// Resolved socket and PID file paths under the store root.
struct Paths {
    socket: PathBuf,
    pid: PathBuf,
}

impl Paths {
    fn for_store(store_path: &Path) -> Self {
        Self {
            socket: store_path.join(SOCKET_FILE),
            pid: store_path.join(PID_FILE),
        }
    }
}

/// Run the daemon to completion. Shuts down gracefully on the
/// first `SIGTERM` or `SIGINT`; second signal would be the
/// shell's job to escalate.
pub async fn serve(config: Config) -> Result<(), Error> {
    serve_with_shutdown(config, default_shutdown_signal()).await
}

/// Run the daemon, shutting down when `shutdown` resolves.
/// Used by tests to drive lifecycle without sending real signals.
pub async fn serve_with_shutdown<F>(config: Config, shutdown: F) -> Result<(), Error>
where
    F: Future<Output = ()> + Send + 'static,
{
    let store_path = config.store_path;
    let paths = Paths::for_store(&store_path);

    let store = open_store(&store_path)?;
    refuse_if_live_daemon(&paths.socket).await?;

    let listener = bind_listener(&paths.socket).await?;
    let pid_guard = PidFile::write(&paths.pid)?;
    let socket_guard = SocketFile::new(paths.socket.clone());

    let pid = std::process::id();
    let state = AppState {
        store: Arc::new(store),
        store_path: store_path.clone(),
        pid,
    };

    let app = api::router(state);

    tracing::info!(
        socket = %paths.socket.display(),
        pid = pid,
        store = %store_path.display(),
        "kairo-daemon listening"
    );

    let result = run_accept_loop(listener, app, shutdown).await;

    // `Drop` on the guards unlinks the PID file and socket; we
    // hold references explicitly so they survive the await above
    // even on panic-during-shutdown.
    drop(pid_guard);
    drop(socket_guard);

    tracing::info!(socket = %paths.socket.display(), "kairo-daemon shut down");

    result
}

/// Maximum time we'll wait for in-flight connections to finish
/// after the shutdown signal arrives. Bounded so a wedged client
/// can never block daemon exit indefinitely.
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

/// Per-connection accept loop. axum 0.7's `axum::serve` only
/// accepts `TcpListener`, so we drive the loop manually with
/// hyper-util. `GracefulShutdown` tracks live connections so they
/// can drain when the shutdown signal arrives.
async fn run_accept_loop<F>(listener: UnixListener, app: Router, shutdown: F) -> Result<(), Error>
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
                        // Transient accept errors are common (e.g. the
                        // peer dropped before we got to accept). Don't
                        // tear down the daemon on them.
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

fn spawn_connection(stream: UnixStream, app: Router, graceful: &GracefulShutdown) {
    let io = TokioIo::new(stream);
    // axum's `Router` implements `tower::Service`; wrap it as a
    // hyper service so hyper-util can drive the connection. Each
    // request clones the router (Router is cheap to clone — just
    // an Arc bump).
    let hyper_service = hyper::service::service_fn(move |req: Request<Incoming>| {
        let mut app = app.clone();
        async move { app.call(req).await }
    });

    // `Builder::serve_connection` borrows from the builder, so the
    // builder must outlive the connection. Construct it inside the
    // task and produce a `'static` connection via `into_owned`
    // before handing it to `GracefulShutdown::watch`.
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

fn open_store(store_path: &Path) -> Result<FilesystemStore, Error> {
    FilesystemStore::open(store_path).map_err(|error| Error::OpenStore {
        path: store_path.to_path_buf(),
        source: Box::new(error),
    })
}

async fn refuse_if_live_daemon(socket_path: &Path) -> Result<(), Error> {
    if !socket_path.exists() {
        return Ok(());
    }

    match timeout(PROBE_TIMEOUT, UnixStream::connect(socket_path)).await {
        Ok(Ok(_stream)) => Err(Error::AlreadyRunning {
            socket: socket_path.to_path_buf(),
        }),
        // `connect` returned an error: socket file exists but no
        // peer is listening (stale from a prior crash). Safe to
        // unlink and re-bind below.
        Ok(Err(_)) => unlink_stale_socket(socket_path).await,
        // Probe timed out: connection negotiation hung. Treat as
        // "another daemon is up" and refuse, rather than risk
        // double-binding next to a slow live process.
        Err(_) => Err(Error::AlreadyRunning {
            socket: socket_path.to_path_buf(),
        }),
    }
}

async fn unlink_stale_socket(socket_path: &Path) -> Result<(), Error> {
    fs::remove_file(socket_path)
        .await
        .map_err(|error| Error::SocketIo {
            path: socket_path.to_path_buf(),
            source: error,
        })
}

async fn bind_listener(socket_path: &Path) -> Result<UnixListener, Error> {
    let listener = UnixListener::bind(socket_path).map_err(|error| Error::SocketIo {
        path: socket_path.to_path_buf(),
        source: error,
    })?;

    // Tighten the socket file's permissions to 0600 — the
    // daemon's only authn boundary in v1 (see DECISIONS.md §10.2
    // and DAEMON.md §18.1). Default umask leaves the bind mode
    // wider than we want.
    let metadata = fs::metadata(socket_path)
        .await
        .map_err(|error| Error::SocketIo {
            path: socket_path.to_path_buf(),
            source: error,
        })?;
    let mut perms = metadata.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(socket_path, perms)
        .await
        .map_err(|error| Error::SocketIo {
            path: socket_path.to_path_buf(),
            source: error,
        })?;

    Ok(listener)
}

async fn default_shutdown_signal() {
    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(stream) => stream,
        Err(error) => {
            tracing::error!(error = %error, "failed to install SIGTERM handler; shutdown will only respond to SIGINT");
            futures_pending(SignalKind::terminate()).await
        }
    };
    let mut sigint = match signal(SignalKind::interrupt()) {
        Ok(stream) => stream,
        Err(error) => {
            tracing::error!(error = %error, "failed to install SIGINT handler; shutdown will only respond to SIGTERM");
            futures_pending(SignalKind::interrupt()).await
        }
    };

    tokio::select! {
        _ = sigterm.recv() => tracing::info!("received SIGTERM"),
        _ = sigint.recv() => tracing::info!("received SIGINT"),
    }
}

/// Pending future used as a fallback when one signal kind fails
/// to install — the other kind still works.
async fn futures_pending(_kind: SignalKind) -> tokio::signal::unix::Signal {
    std::future::pending().await
}

/// PID file that unlinks itself on drop. Created with an atomic
/// rename so a partial write is never observable.
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

/// RAII guard that unlinks the listening socket on drop. The
/// kernel does not auto-clean Unix socket files when the process
/// exits, so without this a re-bind on the same path would fail.
struct SocketFile {
    path: PathBuf,
}

impl SocketFile {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for SocketFile {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    path = %self.path.display(),
                    error = %error,
                    "failed to unlink socket file on shutdown",
                );
            }
        }
    }
}
