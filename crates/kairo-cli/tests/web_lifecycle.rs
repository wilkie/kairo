//! End-to-end lifecycle test for `kairo web start | status | stop`.
//!
//! Spawns the actual `kairo` binary as a subprocess (twice — once
//! for the daemon, once for the web server) and drives them
//! through a complete round trip against a tempdir store and a
//! tempdir SPA bundle. The exit criterion in
//! `specs/PHASE_2_WEB_CLIENT.md` slice 4.

#![allow(clippy::expect_used, clippy::panic)]

// Integration tests inherit kairo-cli's full prod-dep set;
// silence the lint for ones the test doesn't reference directly.
use base64 as _;
use clap as _;
use ed25519_dalek as _;
use http_body_util as _;
use hyper as _;
use hyper_util as _;
use kairo_bundle as _;
use kairo_core as _;
use kairo_daemon as _;
use kairo_daemon_client as _;
use kairo_git as _;
use kairo_identity as _;
use kairo_keystore as _;
use kairo_object as _;
use kairo_statement as _;
use kairo_store as _;
use kairo_test_support as _;
use kairo_web as _;
use nix as _;
use serde_json as _;
use tokio as _;

use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

const KAIRO_BIN: &str = env!("CARGO_BIN_EXE_kairo");

/// Spawn `kairo daemon start --store <dir>` and wait until the
/// listening socket is ready.
fn spawn_daemon(store_path: &Path) -> Child {
    let child = Command::new(KAIRO_BIN)
        .args(["--store", store_path.to_str().expect("utf-8 path")])
        .args(["daemon", "start"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kairo daemon start");

    wait_for_unix_socket(&store_path.join("daemon.sock"), Duration::from_secs(5));
    child
}

/// Spawn `kairo web start --store <dir> --spa-dir <dir> --bind <addr>`
/// and wait until the TCP port is reachable.
fn spawn_web(store_path: &Path, spa_dir: &Path, bind: &SocketAddr) -> Child {
    let child = Command::new(KAIRO_BIN)
        .args(["--store", store_path.to_str().expect("utf-8 path")])
        .args(["web", "start"])
        .args(["--spa-dir", spa_dir.to_str().expect("utf-8 path")])
        .args(["--bind", &bind.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kairo web start");

    wait_for_tcp(bind, Duration::from_secs(5));
    child
}

fn wait_for_unix_socket(socket: &Path, max: Duration) {
    let start = Instant::now();
    loop {
        if socket.exists() && std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return;
        }
        assert!(
            start.elapsed() <= max,
            "daemon never bound socket at {} within {:?}",
            socket.display(),
            max,
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_tcp(addr: &SocketAddr, max: Duration) {
    let start = Instant::now();
    loop {
        if std::net::TcpStream::connect(addr).is_ok() {
            return;
        }
        assert!(
            start.elapsed() <= max,
            "web server never bound {addr} within {max:?}",
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn run_kairo(args: &[&str]) -> std::process::Output {
    Command::new(KAIRO_BIN)
        .args(args)
        .output()
        .expect("run kairo")
}

/// Pick a free loopback port by binding `127.0.0.1:0`, reading
/// the OS-picked port back, and dropping the listener so the
/// kairo-web binary can re-bind on the same port.
fn pick_free_port() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("probe bind");
    let bound = listener.local_addr().expect("local_addr");
    drop(listener);
    bound
}

fn write_spa_dir() -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(
        dir.path().join("index.html"),
        "<!doctype html><title>spa</title>",
    )
    .expect("write index.html");
    dir
}

#[test]
fn web_start_status_stop_round_trip() {
    let store_dir = TempDir::new().expect("tempdir");
    let store = store_dir.path();

    let mut daemon = spawn_daemon(store);

    let spa_dir = write_spa_dir();
    let bind = pick_free_port();
    let bind_str = bind.to_string();

    let mut web = spawn_web(store, spa_dir.path(), &bind);

    // status: running. Use the same --store so the CLI knows
    // where the PID file lives (status does not actually read it,
    // but matching the daemon test's shape keeps the CLI surface
    // consistent).
    let status = run_kairo(&[
        "--store",
        store.to_str().expect("utf-8"),
        "web",
        "status",
        "--bind",
        &bind_str,
    ]);
    assert!(status.status.success(), "status exit failed: {status:?}");
    let out = String::from_utf8_lossy(&status.stdout);
    assert!(out.contains("running"), "status stdout: {out}");
    assert!(
        out.contains("daemon_version"),
        "status stdout missing proxied daemon_version: {out}"
    );

    // stop --wait: should drain in well under 10s
    let stop = run_kairo(&[
        "--store",
        store.to_str().expect("utf-8"),
        "web",
        "stop",
        "--wait",
    ]);
    assert!(stop.status.success(), "stop failed: {stop:?}");
    let out = String::from_utf8_lossy(&stop.stdout);
    assert!(out.contains("stopped"), "stop stdout: {out}");

    // The web process should now have exited cleanly.
    let result = web.wait().expect("wait web");
    assert!(result.success(), "web exit status: {result:?}");

    // PID file should be gone.
    assert!(!store.join("web.pid").exists());

    // Tear down the daemon so the test is hermetic.
    let stop_daemon = run_kairo(&[
        "--store",
        store.to_str().expect("utf-8"),
        "daemon",
        "stop",
        "--wait",
    ]);
    assert!(stop_daemon.status.success(), "stop daemon: {stop_daemon:?}");
    let _ = daemon.wait().expect("wait daemon");
    drop(spa_dir);
    drop(store_dir);
}

#[test]
fn web_status_without_web_returns_not_running() {
    // Pick a port that isn't bound; CLI should report not running
    // and exit 0.
    let bind = pick_free_port();
    let output = run_kairo(&["web", "status", "--bind", &bind.to_string()]);
    assert!(output.status.success(), "exit: {:?}", output.status);
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("not running"), "stdout: {out}");
}

#[test]
fn web_stop_errors_when_pid_file_missing() {
    let dir = TempDir::new().expect("tempdir");
    let output = run_kairo(&[
        "--store",
        dir.path().to_str().expect("utf-8"),
        "web",
        "stop",
    ]);
    assert!(!output.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("web.pid"),
        "stderr should name the PID file: {stderr}"
    );
}

#[test]
fn web_start_rejects_non_loopback_bind() {
    // Don't actually need a daemon here — kairo_web::serve
    // validates the bind address before connecting to anything.
    let dir = TempDir::new().expect("tempdir");
    let spa_dir = write_spa_dir();
    let output = run_kairo(&[
        "--store",
        dir.path().to_str().expect("utf-8"),
        "web",
        "start",
        "--spa-dir",
        spa_dir.path().to_str().expect("utf-8"),
        "--bind",
        "0.0.0.0:0",
    ]);
    assert!(!output.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("loopback"),
        "stderr should mention loopback: {stderr}"
    );
    drop(spa_dir);
    drop(dir);
}

#[test]
fn web_start_works_without_spa_dir_in_api_only_mode() {
    // No --spa-dir → kairo-web runs as an API proxy only:
    // /api/v1/* still works; / returns 404 with a hint.
    let store_dir = TempDir::new().expect("tempdir");
    let store = store_dir.path();

    let mut daemon = spawn_daemon(store);

    let bind = pick_free_port();
    let bind_str = bind.to_string();

    let mut web = Command::new(KAIRO_BIN)
        .args(["--store", store.to_str().expect("utf-8 path")])
        .args(["web", "start"])
        .args(["--bind", &bind_str])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kairo web start");

    wait_for_tcp(&bind, Duration::from_secs(5));

    let status = run_kairo(&[
        "--store",
        store.to_str().expect("utf-8"),
        "web",
        "status",
        "--bind",
        &bind_str,
    ]);
    assert!(status.status.success(), "status: {status:?}");
    let out = String::from_utf8_lossy(&status.stdout);
    assert!(out.contains("running"), "status stdout: {out}");
    assert!(
        out.contains("daemon_version"),
        "status should still proxy daemon_version: {out}"
    );

    let stop = run_kairo(&[
        "--store",
        store.to_str().expect("utf-8"),
        "web",
        "stop",
        "--wait",
    ]);
    assert!(stop.status.success(), "stop: {stop:?}");
    let _ = web.wait().expect("wait web");

    // Tear down the daemon.
    let stop_daemon = run_kairo(&[
        "--store",
        store.to_str().expect("utf-8"),
        "daemon",
        "stop",
        "--wait",
    ]);
    assert!(stop_daemon.status.success(), "stop daemon: {stop_daemon:?}");
    let _ = daemon.wait().expect("wait daemon");
    drop(store_dir);
}
