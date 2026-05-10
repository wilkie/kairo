//! Runners for `kairo daemon start | status | stop`.
//!
//! `start` runs the daemon in the foreground (foreground only in
//! v1 — `specs/DECISIONS.md` §10.1). `status` probes the socket
//! via `kairo-daemon-client`. `stop` reads the PID file and
//! sends SIGTERM, optionally waiting for the socket to disappear.
//!
//! All three runners are sync wrappers — they build a tokio
//! runtime locally for the async work. The rest of the CLI stays
//! sync (see `specs/PHASE_2_DAEMON.md` §1).

use std::path::Path;
use std::time::{Duration, Instant};

use kairo_daemon::{install_tracing, serve, Config};
use kairo_daemon_client::Client;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

use crate::cli::DaemonCommand;
use crate::error::CliError;
use crate::store_paths::StorePaths;

const SOCKET_FILE: &str = "daemon.sock";
const PID_FILE: &str = "daemon.pid";

/// Dispatch entry point for `kairo daemon ...`. `require_daemon`
/// is the resolved `--daemon` global flag — `daemon status`
/// consults it to choose between exit 0 + "not running" output
/// and exit 9 + an error. Slice 8 will plumb the same flag into
/// the read-command dispatch.
pub(crate) fn run_daemon_command(
    command: DaemonCommand,
    paths: &StorePaths,
    require_daemon: bool,
) -> Result<String, CliError> {
    match command {
        DaemonCommand::Start => run_start(paths),
        DaemonCommand::Status => run_status(paths, require_daemon),
        DaemonCommand::Stop { wait, wait_timeout } => {
            run_stop(paths, wait, Duration::from_secs(wait_timeout))
        }
    }
}

/// Run `kairo-daemon::serve` in the foreground until SIGTERM/
/// SIGINT. Installs the daemon's structured-text tracing
/// subscriber so the CLI shares the daemon's log shape on
/// stderr.
fn run_start(paths: &StorePaths) -> Result<String, CliError> {
    install_tracing();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(CliError::DaemonRuntime)?;

    runtime
        .block_on(serve(Config {
            store_path: paths.store.clone(),
        }))
        .map_err(|error| CliError::DaemonServe {
            source: Box::new(error),
        })?;

    // serve only returns when shutting down cleanly; nothing to
    // print on stdout. The daemon's own tracing on stderr is the
    // user-visible signal.
    Ok(String::new())
}

fn run_status(paths: &StorePaths, require_daemon: bool) -> Result<String, CliError> {
    let socket_path = paths.store.join(SOCKET_FILE);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(CliError::DaemonRuntime)?;

    let probe_result = runtime.block_on(async {
        let client = Client::new(&socket_path);
        if !client.probe(None).await {
            return None;
        }
        client.status().await.ok()
    });

    match probe_result {
        Some(status) => Ok(format!(
            "kairo-daemon: running\nsocket  = {}\npid     = {}\nstore   = {}\nschema  = {}\nversion = {}\n",
            socket_path.display(),
            status.pid,
            status.store_path,
            status.store_schema_version,
            status.daemon_version,
        )),
        None => {
            if require_daemon {
                Err(CliError::DaemonUnavailable {
                    socket: socket_path,
                })
            } else {
                Ok(format!(
                    "kairo-daemon: not running\nsocket = {}\n",
                    socket_path.display()
                ))
            }
        }
    }
}

fn run_stop(paths: &StorePaths, wait: bool, wait_timeout: Duration) -> Result<String, CliError> {
    let pid_path = paths.store.join(PID_FILE);
    let socket_path = paths.store.join(SOCKET_FILE);

    let pid = read_pid(&pid_path)?;

    kill(Pid::from_raw(pid), Signal::SIGTERM)
        .map_err(|error| CliError::DaemonKill { pid, source: error })?;

    if !wait {
        return Ok(format!("kairo-daemon: SIGTERM sent (pid {pid})\n"));
    }

    let waited = wait_for_socket_gone(&socket_path, wait_timeout)?;
    Ok(format!(
        "kairo-daemon: stopped (pid {pid}, socket gone after {:.1}s)\n",
        waited.as_secs_f64()
    ))
}

fn read_pid(path: &Path) -> Result<i32, CliError> {
    let contents = std::fs::read_to_string(path).map_err(|source| CliError::ReadPid {
        path: path.to_path_buf(),
        source,
    })?;
    let trimmed = contents.trim();
    trimmed.parse::<i32>().map_err(|_| CliError::InvalidPid {
        path: path.to_path_buf(),
        contents: trimmed.to_owned(),
    })
}

fn wait_for_socket_gone(socket: &Path, max: Duration) -> Result<Duration, CliError> {
    let start = Instant::now();
    loop {
        if !socket.exists() {
            return Ok(start.elapsed());
        }
        if start.elapsed() >= max {
            return Err(CliError::DaemonStopTimeout {
                socket: socket.to_path_buf(),
                waited: max,
            });
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn read_pid_parses_trailing_newline() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("daemon.pid");
        std::fs::write(&path, "12345\n").expect("write pid");

        let pid = read_pid(&path).expect("parse");
        assert_eq!(pid, 12345);
    }

    #[test]
    fn read_pid_errors_on_missing_file() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("daemon.pid");

        match read_pid(&path) {
            Err(CliError::ReadPid { .. }) => {}
            other => panic!("expected ReadPid, got {other:?}"),
        }
    }

    #[test]
    fn read_pid_errors_on_garbage() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("daemon.pid");
        std::fs::write(&path, "not-a-number").expect("write");

        match read_pid(&path) {
            Err(CliError::InvalidPid { contents, .. }) => {
                assert_eq!(contents, "not-a-number");
            }
            other => panic!("expected InvalidPid, got {other:?}"),
        }
    }

    #[test]
    fn run_status_without_daemon_returns_not_running() {
        let dir = TempDir::new().expect("tempdir");
        let paths = StorePaths {
            store: dir.path().to_path_buf(),
            keys: dir.path().join("keys"),
        };

        let output = run_status(&paths, false).expect("status");
        assert!(output.contains("not running"), "got: {output}");
    }

    #[test]
    fn run_status_with_require_daemon_errors_when_absent() {
        let dir = TempDir::new().expect("tempdir");
        let paths = StorePaths {
            store: dir.path().to_path_buf(),
            keys: dir.path().join("keys"),
        };

        match run_status(&paths, true) {
            Err(CliError::DaemonUnavailable { .. }) => {}
            other => panic!("expected DaemonUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn run_stop_errors_when_pid_file_missing() {
        let dir = TempDir::new().expect("tempdir");
        let paths = StorePaths {
            store: dir.path().to_path_buf(),
            keys: dir.path().join("keys"),
        };

        match run_stop(&paths, false, Duration::from_secs(1)) {
            Err(CliError::ReadPid { .. }) => {}
            other => panic!("expected ReadPid, got {other:?}"),
        }
    }

    #[test]
    fn run_stop_errors_when_pid_is_garbage() {
        let dir = TempDir::new().expect("tempdir");
        let paths = StorePaths {
            store: dir.path().to_path_buf(),
            keys: dir.path().join("keys"),
        };
        std::fs::write(paths.store.join(PID_FILE), "garbage").expect("write");

        match run_stop(&paths, false, Duration::from_secs(1)) {
            Err(CliError::InvalidPid { .. }) => {}
            other => panic!("expected InvalidPid, got {other:?}"),
        }
    }

    #[test]
    fn run_stop_errors_when_pid_does_not_exist() {
        // PID 1 always exists; pick an obviously-invalid PID
        // that's well above any real process: signed i32::MAX is
        // never a live PID on Linux.
        let dir = TempDir::new().expect("tempdir");
        let paths = StorePaths {
            store: dir.path().to_path_buf(),
            keys: dir.path().join("keys"),
        };
        std::fs::write(paths.store.join(PID_FILE), format!("{}", i32::MAX)).expect("write");

        match run_stop(&paths, false, Duration::from_secs(1)) {
            Err(CliError::DaemonKill { pid, .. }) => assert_eq!(pid, i32::MAX),
            other => panic!("expected DaemonKill, got {other:?}"),
        }
    }
}
