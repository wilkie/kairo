//! Probe-and-fall-back dispatch for read commands (CLI.md §3.3).
//!
//! Read commands resolve the active mode via [`resolve`] and
//! then branch on it: in `Mode::Daemon` they call the
//! `kairo-daemon-client` method, in `Mode::Direct` they hit the
//! `FilesystemStore` directly. Output shape is the same in both
//! paths — handlers normalize via the existing JSON-wrapper
//! deserializers.
//!
//! Write commands ignore this entirely. They always go direct
//! (the v1 daemon is read-only).
//!
//! Slice 8 wires three commands (`branch show`, `tag show`,
//! `trust show`) through this dispatch as a proof of pattern.
//! The remaining read commands are one-line follow-ups.

use std::time::Duration;

use kairo_daemon_client::Client;
use tokio::runtime::Runtime;

use crate::error::CliError;
use crate::store_paths::StorePaths;

const SOCKET_FILE: &str = "daemon.sock";
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Flags that decide dispatch behavior. Resolved from the
/// global `--daemon` / `--direct` / `--offline` parsed in
/// [`crate::cli::Cli`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct DispatchOptions {
    /// `--daemon` was passed: the daemon is required. A missing
    /// or unreachable socket returns exit 9 instead of falling
    /// back.
    pub require_daemon: bool,
    /// `--direct` or `--offline` was passed: skip the daemon
    /// even if it is reachable.
    pub force_direct: bool,
}

/// Resolved dispatch mode. `Daemon` carries the runtime so the
/// caller can `runtime.block_on(client.method(...))` without
/// building one per call.
pub(crate) enum Mode {
    Daemon { runtime: Runtime, client: Client },
    Direct,
}

/// Probe-and-fall-back per CLI.md §3.3.
///
/// 1. `--direct` / `--offline` → `Mode::Direct` immediately.
/// 2. No daemon socket on disk → `Mode::Direct` if `--daemon`
///    is unset, otherwise `CliError::DaemonUnavailable`.
/// 3. Socket exists: probe with a short timeout. Reachable
///    means `Mode::Daemon`; unreachable means `Mode::Direct`
///    (or `DaemonUnavailable` under `--daemon`).
pub(crate) fn resolve(paths: &StorePaths, opts: DispatchOptions) -> Result<Mode, CliError> {
    if opts.force_direct {
        return Ok(Mode::Direct);
    }

    let socket_path = paths.store.join(SOCKET_FILE);

    if !socket_path.exists() {
        if opts.require_daemon {
            return Err(CliError::DaemonUnavailable {
                socket: socket_path,
            });
        }
        return Ok(Mode::Direct);
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(CliError::DaemonRuntime)?;
    let client = Client::new(&socket_path);

    let live = runtime.block_on(client.probe(Some(PROBE_TIMEOUT)));
    if !live {
        if opts.require_daemon {
            return Err(CliError::DaemonUnavailable {
                socket: socket_path,
            });
        }
        return Ok(Mode::Direct);
    }

    Ok(Mode::Daemon { runtime, client })
}
