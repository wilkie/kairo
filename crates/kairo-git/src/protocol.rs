//! Git's smart protocol as the federation content plane
//! (`specs/DECISIONS.md` §22): `git upload-pack` serving an object's
//! repo, and `git fetch` pulling commits from a peer.
//!
//! Both sides run with every setting that affects safety pinned on
//! the command line, and with the host's system and global Git config
//! ignored. A peer must not be able to reach the operator's
//! credential helpers, `http.extraHeader`s, `insteadOf` rewrites, or
//! non-HTTP transports, and a node's serving behavior must not change
//! because of an operator's `~/.gitconfig`.

use std::path::Path;
use std::process::Command;

/// Environment variables that would point Git somewhere other than
/// the repo it is given, or re-enable config this module disables.
const SCRUBBED_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "GIT_PROTOCOL",
];

/// A `git` command isolated from the host's Git configuration.
fn isolated_git() -> Command {
    let mut command = Command::new("git");
    for name in SCRUBBED_ENV {
        command.env_remove(name);
    }
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

/// `git upload-pack` for `repo` over smart HTTP, protocol v2 only.
///
/// With `advertise`, it prints the v2 capability advertisement (the
/// body of `GET info/refs?service=git-upload-pack`); otherwise it
/// answers one stateless request from stdin (`POST git-upload-pack`).
///
/// Protocol v2's `upload-pack` serves any object it can read,
/// whatever `uploadpack.allow*SHA1InWant` says, and a per-object repo
/// reads the whole shared pool through its alternates. The caller
/// must therefore filter every request before it reaches this
/// process (`specs/DECISIONS.md` §22); the settings here only keep
/// the advertisement minimal.
pub(crate) fn upload_pack(repo: &Path, advertise: bool) -> Command {
    let mut command = isolated_git();
    command
        .env("GIT_PROTOCOL", "version=2")
        .args([
            "-c",
            "uploadpack.allowFilter=true",
            "-c",
            "uploadpack.allowRefInWant=false",
            "-c",
            "uploadpack.allowSidebandAll=false",
            "-c",
            "uploadpack.advertiseBundleURIs=false",
            "-c",
            "transfer.advertiseObjectInfo=false",
            "-c",
            "transfer.advertiseSID=false",
            "upload-pack",
            "--stateless-rpc",
            "--strict",
        ])
        .args(advertise.then_some("--advertise-refs"))
        .arg(repo);
    command
}

/// `git fetch` from `url` into the repo at `target`, reading
/// `<oid>:<ref>` refspecs from stdin.
///
/// Only `https` and `http` transports; no redirects; no credentials;
/// every fetched pack kept as a pack (never exploded into loose
/// objects, so a quarantine's packs are all there is to migrate); a
/// transfer slower than 1 KiB/s for 60 seconds aborts.
pub(crate) fn fetch(target: &Path, url: &str) -> Command {
    let mut command = isolated_git();
    command
        .arg("-C")
        .arg(target)
        .args([
            "-c",
            "protocol.version=2",
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.https.allow=always",
            "-c",
            "protocol.http.allow=always",
            "-c",
            "http.followRedirects=false",
            "-c",
            "http.lowSpeedLimit=1024",
            "-c",
            "http.lowSpeedTime=60",
            "-c",
            "fetch.unpackLimit=1",
            "-c",
            "transfer.unpackLimit=1",
            "-c",
            "fetch.writeCommitGraph=false",
            "-c",
            "gc.auto=0",
            "-c",
            "maintenance.auto=false",
            "fetch",
            "--no-tags",
            "--no-write-fetch-head",
            "--no-recurse-submodules",
            "--quiet",
            "--stdin",
        ])
        .arg(url);
    command
}
