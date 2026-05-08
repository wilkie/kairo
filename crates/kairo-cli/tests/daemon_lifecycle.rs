//! End-to-end lifecycle test for `kairo daemon start | status |
//! stop`. Spawns the actual `kairo` binary as a subprocess and
//! drives it through a complete round trip against a tempdir
//! store — the exit criterion in `specs/PHASE_2_DAEMON.md`
//! slice 4.

#![allow(clippy::expect_used, clippy::panic)]

// kairo-cli's full prod-dep set is visible to integration tests;
// silence the lint for ones the test file doesn't reference.
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

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

const KAIRO_BIN: &str = env!("CARGO_BIN_EXE_kairo");

/// Spawn `kairo daemon start --store <dir>` and wait until the
/// listening socket is ready to accept.
fn spawn_daemon(store_path: &Path) -> Child {
    let child = Command::new(KAIRO_BIN)
        .args(["--store", store_path.to_str().expect("utf-8 path")])
        .args(["daemon", "start"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kairo daemon start");

    wait_for_socket(&store_path.join("daemon.sock"), Duration::from_secs(5));
    child
}

fn wait_for_socket(socket: &Path, max: Duration) {
    let start = Instant::now();
    loop {
        if socket.exists() {
            // exists isn't enough — the daemon writes the socket
            // path before listen(2). Probe with connect.
            if std::os::unix::net::UnixStream::connect(socket).is_ok() {
                return;
            }
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

fn run_kairo(args: &[&str]) -> std::process::Output {
    Command::new(KAIRO_BIN)
        .args(args)
        .output()
        .expect("run kairo")
}

#[test]
fn daemon_start_status_stop_round_trip() {
    let dir = TempDir::new().expect("tempdir");
    let store = dir.path();

    let mut daemon = spawn_daemon(store);

    // status: running
    let status = run_kairo(&[
        "--store",
        store.to_str().expect("utf-8"),
        "daemon",
        "status",
    ]);
    assert!(status.status.success(), "status exit failed: {status:?}");
    let out = String::from_utf8_lossy(&status.stdout);
    assert!(out.contains("running"), "status stdout: {out}");
    assert!(out.contains("schema  = 1"), "status stdout: {out}");

    // stop --wait: should drain in well under 10s
    let stop = run_kairo(&[
        "--store",
        store.to_str().expect("utf-8"),
        "daemon",
        "stop",
        "--wait",
    ]);
    assert!(stop.status.success(), "stop failed: {stop:?}");
    let out = String::from_utf8_lossy(&stop.stdout);
    assert!(out.contains("stopped"), "stop stdout: {out}");

    // The daemon process should now have exited cleanly.
    let result = daemon.wait().expect("wait daemon");
    assert!(result.success(), "daemon exit status: {result:?}");

    // Socket and PID file should both be gone.
    assert!(!store.join("daemon.sock").exists());
    assert!(!store.join("daemon.pid").exists());
}

#[test]
fn daemon_status_without_daemon_exits_zero_with_not_running() {
    let dir = TempDir::new().expect("tempdir");
    let output = run_kairo(&[
        "--store",
        dir.path().to_str().expect("utf-8"),
        "daemon",
        "status",
    ]);
    assert!(output.status.success(), "exit: {:?}", output.status);
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("not running"), "stdout: {out}");
}

#[test]
fn daemon_status_with_require_daemon_flag_exits_nine_when_absent() {
    let dir = TempDir::new().expect("tempdir");
    let output = run_kairo(&[
        "--store",
        dir.path().to_str().expect("utf-8"),
        "--daemon",
        "daemon",
        "status",
    ]);
    let code = output.status.code().expect("exit code");
    assert_eq!(code, 9, "expected exit 9 (daemon_unavailable), got {code}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not running"), "stderr: {stderr}");
}

#[test]
fn daemon_stop_errors_when_pid_file_missing() {
    let dir = TempDir::new().expect("tempdir");
    let output = run_kairo(&[
        "--store",
        dir.path().to_str().expect("utf-8"),
        "daemon",
        "stop",
    ]);
    assert!(!output.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("daemon.pid"),
        "stderr should name the PID file: {stderr}"
    );
}
