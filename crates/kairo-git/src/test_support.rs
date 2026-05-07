//! Shared test fixtures for `cache` and `transport` test modules.
//!
//! Spins up a small non-bare Git repo with two commits via shell-out
//! to `git`. Tests then fetch from this source repo via a `file://`
//! URL, exercising the same code path as a remote fetch without
//! needing network access.

#![cfg(test)]
#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// Returns `(tempdir, file_url, branch_name, head_oid)` for a fresh
/// source repo. The repo is non-bare so `git fetch file://<path>`
/// works (bare repos at the file:// URL also work, but non-bare is
/// closer to what users actually point at).
pub(crate) fn init_source_repo() -> (TempDir, String, String, String) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path();
    run(path, &["init", "--initial-branch=main", "--quiet"]);
    run(path, &["config", "user.name", "Kairo Test"]);
    run(path, &["config", "user.email", "test@kairo.test"]);
    run(path, &["config", "commit.gpgsign", "false"]);
    fs::write(path.join("kairo.toml"), "[kairo]\nschema = 1\n").expect("write kairo.toml");
    run(path, &["add", "kairo.toml"]);
    run(path, &["commit", "-m", "first", "--quiet"]);
    fs::write(path.join("kairo.toml"), "[kairo]\nschema = 1\nname = \"two\"\n")
        .expect("write kairo.toml");
    run(path, &["add", "kairo.toml"]);
    run(path, &["commit", "-m", "second", "--quiet"]);
    let head = rev_parse(path, "HEAD");
    let url = format!("file://{}", path.display());
    (dir, url, "main".to_owned(), head)
}

fn run(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

fn rev_parse(dir: &Path, rev: &str) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", rev])
        .output()
        .expect("rev-parse");
    assert!(output.status.success(), "rev-parse {rev} failed");
    String::from_utf8(output.stdout)
        .expect("utf8")
        .trim()
        .to_owned()
}

/// Returns true if the host has no `git` binary on PATH; tests that
/// need git can early-return when this is true.
pub(crate) fn skip_if_no_git() -> bool {
    Command::new("git").arg("--version").output().is_err()
}
