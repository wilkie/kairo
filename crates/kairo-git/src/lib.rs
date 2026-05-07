//! Git operations Kairo needs to verify, inspect, and cache
//! `ObjectRevision` statements.
//!
//! This crate isolates the `gix` dependency from the rest of the
//! workspace. Read-only operations (`discover`, `open`, `find_commit`,
//! `read_blob_at_path`) use `gix` and need no external `git` binary.
//! The managed Git cache ([`GitCache`]) is mutating; per
//! `specs/DECISIONS.md` §8 it shells out to the host's `git` binary
//! for init and (eventually) fetch, so callers that touch the cache
//! require `git ≥ 2.x` on PATH.

mod cache;
mod lock;
mod shard;

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

pub use cache::GitCache;

/// A handle to an opened Git repository.
pub struct Repository {
    inner: gix::Repository,
}

impl fmt::Debug for Repository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Repository")
            .field("git_dir", &self.inner.git_dir())
            .finish()
    }
}

/// Information extracted from a single Git commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub commit_id: String,
    pub parent_ids: Vec<String>,
}

#[derive(Debug)]
pub enum GitError {
    /// `gix::discover` could not find a repository walking up from
    /// the given path.
    Discovery(Box<dyn Error + Send + Sync>),
    /// Opening a repository at an explicit path failed.
    Open(Box<dyn Error + Send + Sync>),
    /// The given commit oid was not parseable as a hex object id.
    InvalidOid {
        input: String,
        source: Box<dyn Error + Send + Sync>,
    },
    /// Generic operation error from gix while traversing the repository.
    Operation(Box<dyn Error + Send + Sync>),
    /// I/O failure under the cache root (creating dirs, writing
    /// alternates, etc.). Kept separate from [`GitError::Open`]
    /// because the failing path is a cache structural step, not a
    /// repository open.
    CacheIo {
        path: PathBuf,
        source: std::io::Error,
    },
    /// `git init --bare` (or another shelled-out `git` invocation
    /// performed by the cache) failed. The captured stderr is
    /// surfaced verbatim per `DECISIONS.md` §8.
    CacheGitInvocation {
        command: String,
        stderr: String,
        exit_code: Option<i32>,
    },
    /// `git` binary not found on PATH or unable to be spawned.
    /// Surfaces lazily on the first cache-mutating operation per
    /// `DECISIONS.md` §8.
    CacheGitMissing { source: std::io::Error },
    /// Per-record advisory lock on a cache resource (pool or a
    /// per-object repo) was contended for longer than the bounded
    /// retry window. Mirrors `StoreError::LockTimeout` and
    /// `KeystoreError::LockTimeout`.
    CacheLockTimeout { path: PathBuf },
    /// Object id passed to a `GitCache` API was not shape-valid for
    /// sharding (too short, or split across a non-ASCII boundary).
    /// Kairo IDs are always pure base58 ASCII so this is an
    /// invariant violation, not a transient failure.
    CacheInvalidObjectId { id: String, reason: &'static str },
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(error) => write!(f, "no Git repository discovered: {error}"),
            Self::Open(error) => write!(f, "failed to open Git repository: {error}"),
            Self::InvalidOid { input, source } => {
                write!(f, "invalid Git object id {input:?}: {source}")
            }
            Self::Operation(error) => write!(f, "Git operation failed: {error}"),
            Self::CacheIo { path, source } => {
                write!(f, "Git cache I/O failed at {}: {source}", path.display())
            }
            Self::CacheGitInvocation {
                command,
                stderr,
                exit_code,
            } => match exit_code {
                Some(code) => write!(
                    f,
                    "git command failed (exit {code}): {command}\n--- stderr ---\n{stderr}"
                ),
                None => write!(
                    f,
                    "git command terminated by signal: {command}\n--- stderr ---\n{stderr}"
                ),
            },
            Self::CacheGitMissing { source } => write!(
                f,
                "git binary not available on PATH ({source}); install git ≥ 2.x to use the Git cache"
            ),
            Self::CacheLockTimeout { path } => write!(
                f,
                "timed out acquiring advisory lock on {}",
                path.display()
            ),
            Self::CacheInvalidObjectId { id, reason } => {
                write!(f, "invalid object id {id:?} for cache: {reason}")
            }
        }
    }
}

impl Error for GitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Discovery(error) | Self::Open(error) | Self::Operation(error) => {
                Some(error.as_ref())
            }
            Self::InvalidOid { source, .. } => Some(source.as_ref()),
            Self::CacheIo { source, .. } | Self::CacheGitMissing { source } => Some(source),
            Self::CacheGitInvocation { .. }
            | Self::CacheLockTimeout { .. }
            | Self::CacheInvalidObjectId { .. } => None,
        }
    }
}

/// Walk upward from `path` looking for a Git repository (the
/// `git discover` algorithm). Returns the first repository found, or
/// `GitError::Discovery` if none exists between `path` and the
/// filesystem root or a mount boundary.
pub fn discover(path: &Path) -> Result<Repository, GitError> {
    gix::discover(path)
        .map(|inner| Repository { inner })
        .map_err(|error| GitError::Discovery(Box::new(error)))
}

/// Open a repository at the exact path `path` (no upward walk).
pub fn open(path: &Path) -> Result<Repository, GitError> {
    gix::open(path)
        .map(|inner| Repository { inner })
        .map_err(|error| GitError::Open(Box::new(error)))
}

impl Repository {
    /// Filesystem path of this repository's `.git` directory.
    pub fn git_dir(&self) -> &Path {
        self.inner.git_dir()
    }

    /// Look up a commit by its full hex object id. Returns `Ok(None)`
    /// when the commit is not present in this repository (a normal
    /// "not found" outcome); returns `Err` for parse or operational
    /// failures.
    pub fn find_commit(&self, hex_oid: &str) -> Result<Option<CommitInfo>, GitError> {
        let oid = parse_oid(hex_oid)?;
        let object = match self.inner.try_find_object(oid) {
            Ok(Some(object)) => object,
            Ok(None) => return Ok(None),
            Err(error) => return Err(GitError::Operation(Box::new(error))),
        };
        let commit = object
            .try_into_commit()
            .map_err(|error| GitError::Operation(Box::new(error)))?;

        let parent_ids = commit
            .parent_ids()
            .map(|id| id.to_string())
            .collect::<Vec<_>>();

        Ok(Some(CommitInfo {
            commit_id: commit.id().to_string(),
            parent_ids,
        }))
    }

    /// Read the bytes of a blob at `path` within `commit_oid`'s tree.
    /// Returns `Ok(None)` when either the commit is absent or the
    /// path does not exist (or is not a blob) within its tree.
    pub fn read_blob_at_path(
        &self,
        commit_hex_oid: &str,
        path: &str,
    ) -> Result<Option<Vec<u8>>, GitError> {
        let oid = parse_oid(commit_hex_oid)?;
        let object = match self.inner.try_find_object(oid) {
            Ok(Some(object)) => object,
            Ok(None) => return Ok(None),
            Err(error) => return Err(GitError::Operation(Box::new(error))),
        };
        let commit = object
            .try_into_commit()
            .map_err(|error| GitError::Operation(Box::new(error)))?;
        let tree = commit
            .tree()
            .map_err(|error| GitError::Operation(Box::new(error)))?;

        let mut buf = Vec::new();
        let entry = match tree.lookup_entry_by_path(path, &mut buf) {
            Ok(Some(entry)) => entry,
            Ok(None) => return Ok(None),
            Err(error) => return Err(GitError::Operation(Box::new(error))),
        };
        if !entry.mode().is_blob() {
            return Ok(None);
        }
        let blob = entry
            .object()
            .map_err(|error| GitError::Operation(Box::new(error)))?;
        Ok(Some(blob.data.clone()))
    }
}

fn parse_oid(hex: &str) -> Result<gix::ObjectId, GitError> {
    gix::ObjectId::from_hex(hex.as_bytes()).map_err(|error| GitError::InvalidOid {
        input: hex.to_owned(),
        source: Box::new(error),
    })
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    /// Initialize a small git repo with two commits in a temp dir,
    /// returning (tempdir, first_commit_oid, second_commit_oid).
    fn init_repo() -> (TempDir, String, String) {
        let dir = TempDir::new().expect("tempdir");
        run(&dir, &["init", "--initial-branch=main", "--quiet"]);
        run(&dir, &["config", "user.name", "Kairo Test"]);
        run(&dir, &["config", "user.email", "test@kairo.test"]);
        run(&dir, &["config", "commit.gpgsign", "false"]);
        fs::write(dir.path().join("kairo.toml"), "[kairo]\nschema = 1\n")
            .expect("write kairo.toml");
        run(&dir, &["add", "kairo.toml"]);
        run(&dir, &["commit", "-m", "first", "--quiet"]);
        let first = rev_parse(&dir, "HEAD");
        fs::write(dir.path().join("kairo.toml"), "[kairo]\nschema = 1\nname = \"two\"\n")
            .expect("write kairo.toml");
        run(&dir, &["add", "kairo.toml"]);
        run(&dir, &["commit", "-m", "second", "--quiet"]);
        let second = rev_parse(&dir, "HEAD");
        (dir, first, second)
    }

    fn run(dir: &TempDir, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(dir.path())
            .args(args)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    fn rev_parse(dir: &TempDir, rev: &str) -> String {
        let output = Command::new("git")
            .current_dir(dir.path())
            .args(["rev-parse", rev])
            .output()
            .expect("rev-parse");
        assert!(output.status.success(), "rev-parse {rev} failed");
        String::from_utf8(output.stdout).expect("utf8").trim().to_owned()
    }

    #[test]
    fn discover_finds_repo_from_inside_working_tree() {
        let (dir, _, _) = init_repo();
        let nested = dir.path().join("nested-dir");
        fs::create_dir_all(&nested).expect("create nested");
        let repo = discover(&nested).expect("discover");
        assert!(repo.git_dir().ends_with(".git"));
    }

    #[test]
    fn discover_errors_outside_any_repo() {
        let dir = TempDir::new().expect("tempdir");
        let result = discover(dir.path());
        assert!(matches!(result, Err(GitError::Discovery(_))));
    }

    #[test]
    fn find_commit_returns_parents() {
        let (dir, first, second) = init_repo();
        let repo = discover(dir.path()).expect("discover");
        let info = repo
            .find_commit(&second)
            .expect("find")
            .expect("present");
        assert_eq!(info.commit_id, second);
        assert_eq!(info.parent_ids, vec![first]);
    }

    #[test]
    fn find_commit_returns_none_for_missing() {
        let (dir, _, _) = init_repo();
        let repo = discover(dir.path()).expect("discover");
        // A valid-looking sha that isn't in the repo.
        let result = repo
            .find_commit("0123456789abcdef0123456789abcdef01234567")
            .expect("call");
        assert!(result.is_none());
    }

    #[test]
    fn find_commit_errors_on_invalid_oid() {
        let (dir, _, _) = init_repo();
        let repo = discover(dir.path()).expect("discover");
        let result = repo.find_commit("not-a-hex-oid");
        assert!(matches!(result, Err(GitError::InvalidOid { .. })));
    }

    #[test]
    fn read_blob_at_path_returns_kairo_toml() {
        let (dir, _, second) = init_repo();
        let repo = discover(dir.path()).expect("discover");
        let bytes = repo
            .read_blob_at_path(&second, "kairo.toml")
            .expect("read")
            .expect("present");
        let text = String::from_utf8(bytes).expect("utf8");
        assert!(text.contains("name = \"two\""));
    }

    #[test]
    fn read_blob_at_path_returns_none_for_missing_file() {
        let (dir, _, second) = init_repo();
        let repo = discover(dir.path()).expect("discover");
        let result = repo
            .read_blob_at_path(&second, "nonexistent.toml")
            .expect("call");
        assert!(result.is_none());
    }

    #[test]
    fn read_blob_at_path_returns_none_for_missing_commit() {
        let (dir, _, _) = init_repo();
        let repo = discover(dir.path()).expect("discover");
        let result = repo
            .read_blob_at_path(
                "0123456789abcdef0123456789abcdef01234567",
                "kairo.toml",
            )
            .expect("call");
        assert!(result.is_none());
    }
}
