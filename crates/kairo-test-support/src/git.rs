//! Real-`git`-binary fixtures: spin up source repos, build packs,
//! probe availability. Tests that hit the cache or bundle git-data
//! paths use these to avoid network access — `file://` URLs against
//! a temp repo exercise the same code paths as a real fetch.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// Returns true if the host has no `git` binary on PATH. Tests that
/// need `git` typically early-return when this is true rather than
/// failing — the build environment may legitimately lack `git` and
/// only the cache/bundle paths require it.
pub fn skip_if_no_git() -> bool {
    Command::new("git").arg("--version").output().is_err()
}

/// Spin up a temp non-bare SHA-1 Git repo with two commits and return
/// `(tempdir, file_url, branch_name, head_oid)` for it. The repo
/// holds a `kairo.toml` blob committed twice so callers can use
/// the head OID as a `git:sha1:` revision in test fixtures.
pub fn init_source_repo() -> (TempDir, String, String, String) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path();
    run(path, &["init", "--initial-branch=main", "--quiet"]);
    run(path, &["config", "user.name", "Kairo Test"]);
    run(path, &["config", "user.email", "test@kairo.test"]);
    run(path, &["config", "commit.gpgsign", "false"]);
    fs::write(path.join("kairo.toml"), "[kairo]\nschema = 1\n").expect("write kairo.toml");
    run(path, &["add", "kairo.toml"]);
    run(path, &["commit", "-m", "first", "--quiet"]);
    fs::write(
        path.join("kairo.toml"),
        "[kairo]\nschema = 1\nname = \"two\"\n",
    )
    .expect("write kairo.toml");
    run(path, &["add", "kairo.toml"]);
    run(path, &["commit", "-m", "second", "--quiet"]);
    let head = rev_parse(path, "HEAD");
    let url = format!("file://{}", path.display());
    (dir, url, "main".to_owned(), head)
}

/// Spin up a temp non-bare SHA-1 Git repo with `manifest_text` written
/// to `kairo.toml` and committed once. Returns `(tempdir, head_oid)`;
/// label the oid `git:sha1:`. Used by tests that need a specific
/// manifest content (e.g., matching a revision's signed manifest
/// hash).
pub fn init_git_repo_with_manifest(
    manifest_text: &str,
) -> Result<(TempDir, String), Box<dyn std::error::Error>> {
    init_git_repo_with_manifest_in(manifest_text, "sha1")
}

/// [`init_git_repo_with_manifest`] in a chosen Git object format
/// (`"sha1"` or `"sha256"`, as `git init --object-format` takes).
pub fn init_git_repo_with_manifest_in(
    manifest_text: &str,
    object_format: &str,
) -> Result<(TempDir, String), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let path = dir.path();
    let format_arg = format!("--object-format={object_format}");
    run_checked(
        path,
        &["init", "--initial-branch=main", "--quiet", &format_arg],
    )?;
    run_checked(path, &["config", "user.name", "Kairo Test"])?;
    run_checked(path, &["config", "user.email", "test@kairo.test"])?;
    run_checked(path, &["config", "commit.gpgsign", "false"])?;
    fs::write(path.join("kairo.toml"), manifest_text)?;
    run_checked(path, &["add", "kairo.toml"])?;
    run_checked(path, &["commit", "-m", "first", "--quiet"])?;
    let output = Command::new("git")
        .current_dir(path)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !output.status.success() {
        return Err("rev-parse failed".into());
    }
    let oid = String::from_utf8(output.stdout)?.trim().to_owned();
    Ok((dir, oid))
}

/// Spin up a temp SHA-1 Git repo with `files` (`(path, contents)`,
/// paths relative, parent directories created) committed once.
/// Returns `(tempdir, head_oid)`; label the oid `git:sha1:`.
pub fn init_git_repo_with_files(
    files: &[(&str, &str)],
) -> Result<(TempDir, String), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let path = dir.path();
    run_checked(path, &["init", "--initial-branch=main", "--quiet"])?;
    run_checked(path, &["config", "user.name", "Kairo Test"])?;
    run_checked(path, &["config", "user.email", "test@kairo.test"])?;
    run_checked(path, &["config", "commit.gpgsign", "false"])?;
    for (name, contents) in files {
        let file = path.join(name);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(file, contents)?;
    }
    run_checked(path, &["add", "."])?;
    run_checked(path, &["commit", "-m", "first", "--quiet"])?;
    let output = Command::new("git")
        .current_dir(path)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !output.status.success() {
        return Err("rev-parse failed".into());
    }
    let oid = String::from_utf8(output.stdout)?.trim().to_owned();
    Ok((dir, oid))
}

/// Turn `dir`, already populated (binaries, symbolic links, anything
/// a tree holds), into a SHA-1 Git repo with one commit of its whole
/// contents. Returns the head oid; label it `git:sha1:`.
pub fn commit_dir(dir: &Path) -> Result<String, Box<dyn std::error::Error>> {
    run_checked(dir, &["init", "--initial-branch=main", "--quiet"])?;
    run_checked(dir, &["config", "user.name", "Kairo Test"])?;
    run_checked(dir, &["config", "user.email", "test@kairo.test"])?;
    run_checked(dir, &["config", "commit.gpgsign", "false"])?;
    run_checked(dir, &["add", "."])?;
    run_checked(dir, &["commit", "-m", "first", "--quiet"])?;
    let output = Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !output.status.success() {
        return Err("rev-parse failed".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// Build a Git pack from every commit reachable in `src` (via
/// `git pack-objects --all --stdout`) and return its raw bytes.
/// Suitable for piping into `kairo_git::GitCache::ingest_pack` or
/// writing to a bundle's `git/<object-id>.pack`.
pub fn build_pack_from(src: &Path) -> Vec<u8> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(src)
        .args(["pack-objects", "--all", "--stdout"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pack-objects");
    let mut buf = Vec::new();
    child
        .stdout
        .as_mut()
        .expect("stdout")
        .read_to_end(&mut buf)
        .expect("read stdout");
    let status = child.wait().expect("wait");
    assert!(
        status.success(),
        "pack-objects failed with exit {:?}",
        status.code()
    );
    buf
}

fn run(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

fn run_checked(dir: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("git").current_dir(dir).args(args).status()?;
    if !status.success() {
        return Err(format!("git {args:?} failed").into());
    }
    Ok(())
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

/// Repositories under a directory served read-only over `git://` on
/// localhost by `git daemon`, for tests of source objects, whose URLs
/// can't be `file://` (`specs/DECISIONS.md` §37). Stopped on drop.
#[derive(Debug)]
pub struct GitDaemon {
    child: std::process::Child,
    port: u16,
    pub base: TempDir,
}

impl GitDaemon {
    /// Start serving a fresh empty directory; put repositories in
    /// [`Self::base`] (bare or not) and address them by [`Self::url`].
    /// `None` when `git daemon` can't run here.
    pub fn start() -> Option<Self> {
        let base = TempDir::new().ok()?;
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .ok()?
            .local_addr()
            .ok()?
            .port();
        let child = Command::new("git")
            .arg("daemon")
            .arg("--reuseaddr")
            .arg("--export-all")
            .arg("--listen=127.0.0.1")
            .arg(format!("--port={port}"))
            .arg(format!("--base-path={}", base.path().display()))
            .arg(base.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let daemon = Self { child, port, base };
        for _ in 0..100 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return Some(daemon);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        None
    }

    /// `git://127.0.0.1:<port>/<name>`.
    pub fn url(&self, name: &str) -> String {
        format!("git://127.0.0.1:{}/{name}", self.port)
    }
}

impl Drop for GitDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Run `git <args>` in `dir`, returning trimmed stdout; panics on
/// failure. For building test histories.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Kairo Test")
        .env("GIT_AUTHOR_EMAIL", "test@kairo.test")
        .env("GIT_COMMITTER_NAME", "Kairo Test")
        .env("GIT_COMMITTER_EMAIL", "test@kairo.test")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
