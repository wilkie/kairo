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
mod protocol;
mod shard;
mod transport;

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub use cache::{FetchLimits, GitCache, RefSnapshot, SourceFetch, OBSERVED_PIN_PREFIX};
pub use kairo_statement::{GitObjectFormat, GitRevision};
pub use transport::{FetchedRef, GitCacheTransport, GitCli};

/// Compute the per-object bare repo path under a Git cache rooted
/// at `git_root`, without opening the cache. Useful for callers
/// that want to probe cache state cheaply (for example,
/// `verify object`'s precedence check) before deciding whether
/// to invoke `GitCache::open` (which initializes the pool repo if
/// absent and therefore requires the host's `git` binary).
pub fn object_repo_path(git_root: &Path, object_id: &str) -> Result<PathBuf, GitError> {
    shard::shard_path(git_root, object_id)
}

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
    /// An ingested pack did not contain the full closure (commit,
    /// ancestors, trees, blobs) of these required commits, so nothing
    /// was moved into the pool (`specs/DECISIONS.md` §21).
    IncompletePack { missing: Vec<String> },
    /// A repository or revision is in a different Git object format
    /// than the operation requires (`specs/DECISIONS.md` §19).
    FormatMismatch {
        context: String,
        expected: GitObjectFormat,
        found: GitObjectFormat,
    },
    /// A fetch wrote more than its byte limit and was stopped; nothing
    /// was moved into the pool.
    TransferTooLarge { limit: u64 },
    /// A fetch ran past its time limit and was stopped; nothing was
    /// moved into the pool.
    TransferTimedOut { after: std::time::Duration },
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
            Self::IncompletePack { missing } => write!(
                f,
                "the pack lacks part of {} required commit(s): {}",
                missing.len(),
                missing.join(", ")
            ),
            Self::FormatMismatch {
                context,
                expected,
                found,
            } => write!(
                f,
                "{context} uses Git object format {found}, but {expected} is required"
            ),
            Self::TransferTooLarge { limit } => {
                write!(f, "the Git transfer exceeded its {limit}-byte limit")
            }
            Self::TransferTimedOut { after } => write!(
                f,
                "the Git transfer did not finish within {} seconds",
                after.as_secs()
            ),
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
            | Self::CacheInvalidObjectId { .. }
            | Self::IncompletePack { .. }
            | Self::FormatMismatch { .. }
            | Self::TransferTooLarge { .. }
            | Self::TransferTimedOut { .. } => None,
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

    /// This repository's object format.
    pub fn object_format(&self) -> Result<GitObjectFormat, GitError> {
        match self.inner.object_hash() {
            gix::hash::Kind::Sha1 => Ok(GitObjectFormat::Sha1),
            gix::hash::Kind::Sha256 => Ok(GitObjectFormat::Sha256),
            #[allow(unreachable_patterns)]
            other => Err(GitError::Operation(
                format!("unsupported Git object format {other:?}").into(),
            )),
        }
    }

    /// Every direct reference and the object it points at, sorted by
    /// name. Symbolic refs (`HEAD`) are skipped; they resolve to one of
    /// the direct refs listed. Read without the `git` binary.
    pub fn ref_snapshot(&self) -> Result<Vec<(String, String)>, GitError> {
        let platform = self
            .inner
            .references()
            .map_err(|error| GitError::Operation(Box::new(error)))?;
        let mut refs = Vec::new();
        for reference in platform
            .all()
            .map_err(|error| GitError::Operation(Box::new(error)))?
        {
            let reference = reference.map_err(|error| GitError::Operation(Box::new(error)))?;
            if let Some(id) = reference.try_id() {
                refs.push((reference.name().as_bstr().to_string(), id.to_string()));
            }
        }
        refs.sort();
        Ok(refs)
    }

    /// Whether the repository has at least one reference.
    pub fn has_refs(&self) -> Result<bool, GitError> {
        let platform = self
            .inner
            .references()
            .map_err(|error| GitError::Operation(Box::new(error)))?;
        let mut all = platform
            .all()
            .map_err(|error| GitError::Operation(Box::new(error)))?;
        Ok(all.next().is_some())
    }

    /// The object id for `revision`, refusing a revision in another
    /// format than this repository's. Git can't resolve it, and the
    /// algorithm must come from the label, never the digest length.
    fn revision_oid(&self, revision: &GitRevision) -> Result<gix::ObjectId, GitError> {
        let format = self.object_format()?;
        if format != revision.format() {
            return Err(GitError::FormatMismatch {
                context: format!("repository {}", self.git_dir().display()),
                expected: revision.format(),
                found: format,
            });
        }
        parse_oid(revision.oid())
    }

    /// Look up `revision`'s commit. Returns `Ok(None)` when the commit
    /// is not present in this repository (a normal "not found"
    /// outcome); returns `Err` for a format mismatch or operational
    /// failures. Parent ids are plain hex in the same format.
    pub fn find_commit(&self, revision: &GitRevision) -> Result<Option<CommitInfo>, GitError> {
        let oid = self.revision_oid(revision)?;
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

    /// The commit `revision` names, peeling annotated tags; `None` when
    /// the object is absent or isn't a commit or a tag of one.
    pub fn peel_to_commit(&self, revision: &GitRevision) -> Result<Option<GitRevision>, GitError> {
        let oid = self.revision_oid(revision)?;
        let object = match self.inner.try_find_object(oid) {
            Ok(Some(object)) => object,
            Ok(None) => return Ok(None),
            Err(error) => return Err(GitError::Operation(Box::new(error))),
        };
        match object.peel_to_commit() {
            Ok(commit) => GitRevision::new(revision.format(), &commit.id().to_string())
                .map(Some)
                .map_err(|error| GitError::Operation(Box::new(error))),
            Err(_) => Ok(None),
        }
    }

    /// The submodules `revision`'s commit pins: every gitlink in its
    /// tree, with the URL `.gitmodules` gives for its path (`None` when
    /// `.gitmodules` has no entry for it). Sorted by path.
    pub fn submodules_at(&self, revision: &GitRevision) -> Result<Vec<SubmoduleLink>, GitError> {
        let oid = self.revision_oid(revision)?;
        let object = match self.inner.try_find_object(oid) {
            Ok(Some(object)) => object,
            Ok(None) => return Ok(Vec::new()),
            Err(error) => return Err(GitError::Operation(Box::new(error))),
        };
        let commit = object
            .try_into_commit()
            .map_err(|error| GitError::Operation(Box::new(error)))?;
        let tree = commit
            .tree()
            .map_err(|error| GitError::Operation(Box::new(error)))?;
        let mut gitlinks = Vec::new();
        collect_gitlinks(&tree, "", revision.format(), &mut gitlinks)?;
        if gitlinks.is_empty() {
            return Ok(Vec::new());
        }
        let urls = self
            .read_blob_at_path(revision, ".gitmodules")?
            .map(|bytes| parse_gitmodules(&String::from_utf8_lossy(&bytes)))
            .unwrap_or_default();
        let mut links: Vec<SubmoduleLink> = gitlinks
            .into_iter()
            .map(|(path, commit)| SubmoduleLink {
                url: urls
                    .iter()
                    .find(|(module_path, _)| module_path == &path)
                    .map(|(_, url)| url.clone()),
                path,
                commit,
            })
            .collect();
        links.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(links)
    }

    /// Whether the object `revision` names (of any type: commit, tag,
    /// tree, blob) is in this repository's object database.
    pub fn has_object(&self, revision: &GitRevision) -> Result<bool, GitError> {
        let oid = self.revision_oid(revision)?;
        self.inner
            .try_find_object(oid)
            .map(|object| object.is_some())
            .map_err(|error| GitError::Operation(Box::new(error)))
    }

    /// Read the bytes of a blob at `path` within `commit_oid`'s tree.
    /// Returns `Ok(None)` when either the commit is absent or the
    /// path does not exist (or is not a blob) within its tree.
    pub fn read_blob_at_path(
        &self,
        revision: &GitRevision,
        path: &str,
    ) -> Result<Option<Vec<u8>>, GitError> {
        let oid = self.revision_oid(revision)?;
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

        let entry = match tree.lookup_entry_by_path(path) {
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

    /// Write `revision`'s whole tree under `dest`, which must exist
    /// and be empty: regular files with their executable bit, symbolic
    /// links as links. Returns `Ok(false)` when the commit is absent.
    pub fn export_tree(&self, revision: &GitRevision, dest: &Path) -> Result<bool, GitError> {
        let oid = self.revision_oid(revision)?;
        let object = match self.inner.try_find_object(oid) {
            Ok(Some(object)) => object,
            Ok(None) => return Ok(false),
            Err(error) => return Err(GitError::Operation(Box::new(error))),
        };
        let commit = object
            .try_into_commit()
            .map_err(|error| GitError::Operation(Box::new(error)))?;
        let tree = commit
            .tree()
            .map_err(|error| GitError::Operation(Box::new(error)))?;
        export_tree_into(&tree, dest)?;
        Ok(true)
    }
}

/// One entry of a tree: its mode (in octal), name, and object id.
pub type TreeEntry = (String, String, String);

impl Repository {
    /// The top-level entries of `revision`'s tree, each as its mode (in
    /// octal), name, and object id, in the tree's order, or `None` when
    /// the commit isn't held. What a build's tree is, for its build key
    /// (`specs/DECISIONS.md` §52): an entry's id names its whole subtree
    /// or contents.
    pub fn root_entries(&self, revision: &GitRevision) -> Result<Option<Vec<TreeEntry>>, GitError> {
        let oid = self.revision_oid(revision)?;
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
        let mut entries = Vec::new();
        for entry in tree.iter() {
            let entry = entry.map_err(|error| GitError::Operation(Box::new(error)))?;
            entries.push((
                format!("{:o}", entry.mode().value()),
                String::from_utf8_lossy(entry.filename()).into_owned(),
                entry.oid().to_string(),
            ));
        }
        Ok(Some(entries))
    }
}

fn export_tree_into(tree: &gix::Tree<'_>, dest: &Path) -> Result<(), GitError> {
    for entry in tree.iter() {
        let entry = entry.map_err(|error| GitError::Operation(Box::new(error)))?;
        let name = std::str::from_utf8(entry.filename()).map_err(|error| {
            GitError::Operation(Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("tree entry name is not UTF-8: {error}"),
            )))
        })?;
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(GitError::Operation(Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsafe tree entry name {name:?}"),
            ))));
        }
        let path = dest.join(name);
        let mode = entry.mode();
        let object = entry
            .object()
            .map_err(|error| GitError::Operation(Box::new(error)))?;
        if mode.is_tree() {
            fs::create_dir(&path).map_err(|error| GitError::Operation(Box::new(error)))?;
            let subtree = object
                .try_into_tree()
                .map_err(|error| GitError::Operation(Box::new(error)))?;
            export_tree_into(&subtree, &path)?;
        } else if mode.is_link() {
            let target = std::str::from_utf8(&object.data).map_err(|error| {
                GitError::Operation(Box::new(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("symlink target is not UTF-8: {error}"),
                )))
            })?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, &path)
                .map_err(|error| GitError::Operation(Box::new(error)))?;
            #[cfg(not(unix))]
            fs::write(&path, target).map_err(|error| GitError::Operation(Box::new(error)))?;
        } else if mode.is_blob() {
            fs::write(&path, &object.data).map_err(|error| GitError::Operation(Box::new(error)))?;
            #[cfg(unix)]
            if mode.is_executable() {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                    .map_err(|error| GitError::Operation(Box::new(error)))?;
            }
        } else {
            // A submodule (commit entry) has no content to write.
            continue;
        }
    }
    Ok(())
}

/// A submodule a commit pins: the gitlink's path and commit, and the
/// URL `.gitmodules` gives for that path, as written there (possibly
/// relative to the superproject's URL).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmoduleLink {
    pub path: String,
    pub url: Option<String>,
    pub commit: GitRevision,
}

fn collect_gitlinks(
    tree: &gix::Tree<'_>,
    prefix: &str,
    format: kairo_statement::GitObjectFormat,
    out: &mut Vec<(String, GitRevision)>,
) -> Result<(), GitError> {
    for entry in tree.iter() {
        let entry = entry.map_err(|error| GitError::Operation(Box::new(error)))?;
        let name = String::from_utf8_lossy(entry.filename()).into_owned();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let mode = entry.mode();
        if mode.is_commit() {
            let commit = GitRevision::new(format, &entry.oid().to_string())
                .map_err(|error| GitError::Operation(Box::new(error)))?;
            out.push((path, commit));
        } else if mode.is_tree() {
            let subtree = entry
                .object()
                .map_err(|error| GitError::Operation(Box::new(error)))?
                .try_into_tree()
                .map_err(|error| GitError::Operation(Box::new(error)))?;
            collect_gitlinks(&subtree, &path, format, out)?;
        }
    }
    Ok(())
}

/// `(path, url)` for each `[submodule "..."]` section of a
/// `.gitmodules` file that has both. A small reader of Git's config
/// syntax: sections, `key = value` lines, `#` and `;` comments, and
/// double-quoted values.
pub fn parse_gitmodules(text: &str) -> Vec<(String, String)> {
    let mut modules: Vec<(Option<String>, Option<String>)> = Vec::new();
    let mut in_submodule = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            in_submodule = line
                .trim_start_matches('[')
                .trim_start()
                .starts_with("submodule");
            if in_submodule {
                modules.push((None, None));
            }
            continue;
        }
        if !in_submodule {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(value)
            .to_owned();
        if let Some(module) = modules.last_mut() {
            match key.trim().to_ascii_lowercase().as_str() {
                "path" => module.0 = Some(value),
                "url" => module.1 = Some(value),
                _ => {}
            }
        }
    }
    modules
        .into_iter()
        .filter_map(|(path, url)| Some((path?, url?)))
        .collect()
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

    /// Initialize a small git repo in `format` with two commits in a
    /// temp dir, returning (tempdir, first_commit, second_commit).
    fn init_repo_in(format: GitObjectFormat) -> (TempDir, GitRevision, GitRevision) {
        let dir = TempDir::new().expect("tempdir");
        let object_format = format!("--object-format={format}");
        run(
            &dir,
            &["init", "--initial-branch=main", "--quiet", &object_format],
        );
        run(&dir, &["config", "user.name", "Kairo Test"]);
        run(&dir, &["config", "user.email", "test@kairo.test"]);
        run(&dir, &["config", "commit.gpgsign", "false"]);
        fs::write(dir.path().join("kairo.toml"), "[kairo]\nschema = 1\n")
            .expect("write kairo.toml");
        run(&dir, &["add", "kairo.toml"]);
        run(&dir, &["commit", "-m", "first", "--quiet"]);
        let first = rev_parse(&dir, "HEAD");
        fs::write(
            dir.path().join("kairo.toml"),
            "[kairo]\nschema = 1\nname = \"two\"\n",
        )
        .expect("write kairo.toml");
        run(&dir, &["add", "kairo.toml"]);
        run(&dir, &["commit", "-m", "second", "--quiet"]);
        let second = rev_parse(&dir, "HEAD");
        let revision = |oid: &str| GitRevision::new(format, oid).expect("oid in format");
        (dir, revision(&first), revision(&second))
    }

    fn init_repo() -> (TempDir, GitRevision, GitRevision) {
        init_repo_in(GitObjectFormat::Sha1)
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
        String::from_utf8(output.stdout)
            .expect("utf8")
            .trim()
            .to_owned()
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
        let info = repo.find_commit(&second).expect("find").expect("present");
        assert_eq!(info.commit_id, second.oid());
        assert_eq!(info.parent_ids, vec![first.oid().to_owned()]);
    }

    #[test]
    fn find_commit_returns_none_for_missing() {
        let (dir, _, _) = init_repo();
        let repo = discover(dir.path()).expect("discover");
        // A valid-looking sha that isn't in the repo.
        let absent = GitRevision::new(
            GitObjectFormat::Sha1,
            "0123456789abcdef0123456789abcdef01234567",
        )
        .expect("shape");
        let result = repo.find_commit(&absent).expect("call");
        assert!(result.is_none());
    }

    #[test]
    fn find_commit_refuses_a_revision_in_another_format() {
        let (dir, _, _) = init_repo();
        let repo = discover(dir.path()).expect("discover");
        assert_eq!(repo.object_format().expect("format"), GitObjectFormat::Sha1);
        let sha256 = GitRevision::new(GitObjectFormat::Sha256, &"a".repeat(64)).expect("shape");
        assert!(matches!(
            repo.find_commit(&sha256),
            Err(GitError::FormatMismatch {
                expected: GitObjectFormat::Sha256,
                found: GitObjectFormat::Sha1,
                ..
            })
        ));
    }

    #[test]
    fn sha256_repositories_resolve_commits_and_blobs() {
        let (dir, first, second) = init_repo_in(GitObjectFormat::Sha256);
        assert_eq!(second.oid().len(), 64);
        let repo = discover(dir.path()).expect("discover");
        assert_eq!(
            repo.object_format().expect("format"),
            GitObjectFormat::Sha256
        );
        let info = repo.find_commit(&second).expect("find").expect("present");
        assert_eq!(info.parent_ids, vec![first.oid().to_owned()]);
        let bytes = repo
            .read_blob_at_path(&second, "kairo.toml")
            .expect("read")
            .expect("present");
        assert!(String::from_utf8(bytes).expect("utf8").contains("two"));
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
    fn export_tree_writes_files_dirs_links_and_modes() {
        let (dir, _, second) = init_repo();
        fs::create_dir(dir.path().join("sub")).expect("mkdir");
        fs::write(dir.path().join("sub/data.txt"), "data\n").expect("write");
        fs::write(dir.path().join("run.sh"), "#!/bin/sh\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir.path().join("run.sh"), fs::Permissions::from_mode(0o755))
                .expect("chmod");
            std::os::unix::fs::symlink("run.sh", dir.path().join("link")).expect("symlink");
        }
        run(&dir, &["add", "."]);
        run(&dir, &["commit", "-m", "tree", "--quiet"]);
        let head = GitRevision::new(
            second.format(),
            String::from_utf8(
                Command::new("git")
                    .args([
                        "-C",
                        dir.path().to_str().expect("path"),
                        "rev-parse",
                        "HEAD",
                    ])
                    .output()
                    .expect("rev-parse")
                    .stdout,
            )
            .expect("utf8")
            .trim(),
        )
        .expect("revision");

        let repo = discover(dir.path()).expect("discover");
        // The top-level entries, each by mode, name, and id.
        let entries = repo.root_entries(&head).expect("entries").expect("held");
        let names: Vec<&str> = entries.iter().map(|(_, name, _)| name.as_str()).collect();
        assert!(
            names.contains(&"kairo.toml") && names.contains(&"sub"),
            "{names:?}"
        );
        assert!(entries
            .iter()
            .any(|(mode, name, _)| name == "sub" && mode == "40000"));
        let out = TempDir::new().expect("tempdir");
        assert!(repo.export_tree(&head, out.path()).expect("export"));
        assert_eq!(
            fs::read_to_string(out.path().join("sub/data.txt")).expect("read"),
            "data\n"
        );
        assert!(fs::read_to_string(out.path().join("kairo.toml"))
            .expect("read")
            .contains("name = \"two\""));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(out.path().join("run.sh"))
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "executable bit kept");
            assert_eq!(
                fs::read_link(out.path().join("link")).expect("link"),
                PathBuf::from("run.sh")
            );
        }

        let absent = GitRevision::new(second.format(), &"0".repeat(second.oid().len()))
            .expect("well-formed");
        let empty = TempDir::new().expect("tempdir");
        assert!(!repo
            .export_tree(&absent, empty.path())
            .expect("absent is Ok(false)"));
    }

    #[test]
    fn gitmodules_parse_paths_and_urls() {
        let text = "# comment\n[core]\n\turl = not-a-module\n\
                    [submodule \"M2libc\"]\n\tpath = M2libc\n\turl = ../M2libc\n\
                    [submodule \"quoted\"]\n\tPath = \"with space\"\n\tURL = https://example.org/q.git\n\
                    [submodule \"no-url\"]\n\tpath = lonely\n";
        assert_eq!(
            parse_gitmodules(text),
            [
                ("M2libc".to_owned(), "../M2libc".to_owned()),
                (
                    "with space".to_owned(),
                    "https://example.org/q.git".to_owned()
                ),
            ]
        );
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
        let absent = GitRevision::new(
            GitObjectFormat::Sha1,
            "0123456789abcdef0123456789abcdef01234567",
        )
        .expect("shape");
        let result = repo.read_blob_at_path(&absent, "kairo.toml").expect("call");
        assert!(result.is_none());
    }
}
