//! `git-remote-kairo <remote> <url>`: Git runs this for `kairo://`
//! URLs (`specs/DECISIONS.md` §31). See the library for the protocol.

// Dev-dependencies used only by `tests/`.
#[cfg(test)]
use kairo_identity as _;
#[cfg(test)]
use kairo_test_support as _;
#[cfg(test)]
use tempfile as _;
#[cfg(test)]
use tokio as _;

use std::env;
use std::io::{self, BufReader};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use kairo_git_remote::{serve, Context};

// The bin drives the lib; these silence `unused_crate_dependencies`.
use kairo_config as _;
use kairo_core as _;
use kairo_federation_http as _;
use kairo_federation_service as _;
use kairo_git as _;
use kairo_statement as _;
use kairo_store as _;

fn store_root() -> Option<PathBuf> {
    env::var_os("KAIRO_STORE")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".kairo")))
}

/// `git config --bool kairo.allowPeer` in the clone.
fn allow_peer(git_dir: &std::path::Path) -> bool {
    Command::new("git")
        .arg("--git-dir")
        .arg(git_dir)
        .args(["config", "--bool", "kairo.allowPeer"])
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "true")
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let Some(url) = args.get(2).cloned() else {
        eprintln!("usage: git-remote-kairo <remote> <kairo://host/object-id>");
        return ExitCode::from(2);
    };
    let (Some(store), Some(git_dir)) = (store_root(), env::var_os("GIT_DIR").map(PathBuf::from))
    else {
        eprintln!("git-remote-kairo: needs $GIT_DIR and $HOME or $KAIRO_STORE");
        return ExitCode::from(2);
    };
    // Git runs a helper inside the clone's environment (`GIT_DIR` and
    // friends). The Kairo pull runs its own `git` against the store's
    // cache, so it must not inherit them; commands aimed at the clone pass
    // `--git-dir` explicitly. Done before any thread or child starts.
    for (name, _) in env::vars_os() {
        let name = name.to_string_lossy().into_owned();
        if name.starts_with("GIT_") && !name.starts_with("GIT_TRACE") {
            env::remove_var(&name);
        }
    }
    let context = Context {
        allow_peer: allow_peer(&git_dir),
        url,
        store,
        git_dir,
    };
    let stdin = io::stdin();
    match serve(
        &context,
        BufReader::new(stdin.lock()),
        io::stdout(),
        io::stderr(),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("git-remote-kairo: {error}");
            ExitCode::from(1)
        }
    }
}
