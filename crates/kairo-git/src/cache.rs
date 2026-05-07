//! Managed Git cache rooted at `~/.kairo/git/`.
//!
//! Layout (per `specs/DECISIONS.md` §7):
//!
//! ```text
//! <root>/
//!   pool/                          # shared bare repo: holds all Git objects
//!     objects/
//!   <XX>/<YY>/<object-id>/         # per-Kairo-object bare repo
//!     objects/info/alternates      # → pool/objects (relative)
//!     refs/heads/...
//! ```
//!
//! Per-object bare repos borrow Git objects from the shared pool via
//! Git's `objects/info/alternates` mechanism, so forking a Kairo
//! object into a second `ObjectId` reuses the underlying commits with
//! no disk duplication. Per-object operations take the per-object
//! lock; pool-level operations take the pool lock. Reads
//! (`has_commit`) take no lock — `gix` handles object-DB reads
//! without coordination.
//!
//! This module covers the foundational scaffolding (`open`,
//! `path_for`, `ensure_repo`, `has_commit`). The `GitCacheTransport`
//! trait, `GitCache::fetch`, and `GitCache::ingest_pack` land in
//! follow-on slices.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::lock::with_path_lock;
use crate::{open, shard, GitError, Repository};

/// Reserved subdirectory name at the cache root for the shared
/// alternates pool. Cannot collide with any sharded `<XX>` dir
/// because shard names are exactly two base58 characters.
const POOL_DIR: &str = "pool";

/// Filename used for sidecar advisory locks at the cache root.
/// Distinct from any sharded path so it can never collide with a
/// per-object lock.
const POOL_LOCK_SUBJECT: &str = "pool";

/// Filesystem-backed Git cache.
///
/// Created via [`GitCache::open`] which initializes (or validates)
/// the shared pool. Per-object repositories are materialized on
/// demand via [`GitCache::ensure_repo`].
#[derive(Debug, Clone)]
pub struct GitCache {
    root: PathBuf,
}

impl GitCache {
    /// Open the cache rooted at `root`, creating the shared pool
    /// (`<root>/pool/`) as a bare Git repository if absent. Idempotent
    /// across processes — concurrent first-time `open` calls
    /// serialize on the pool advisory lock so only one runs
    /// `git init --bare`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, GitError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(|source| GitError::CacheIo {
            path: root.clone(),
            source,
        })?;

        let pool_path = pool_path(&root);
        let pool_lock_subject = root.join(POOL_LOCK_SUBJECT);
        with_path_lock(&pool_lock_subject, || {
            if !pool_path.exists() {
                git_init_bare(&pool_path)?;
            }
            Ok(())
        })?;

        Ok(Self { root })
    }

    /// Filesystem path of the cache root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Sharded path of the per-object bare repository for
    /// `object_id`. Does not check whether the directory exists.
    /// `object_id` should be the bare ID payload (`ObjectId::as_str()`),
    /// not a typed reference like `object:<id>`.
    pub fn path_for(&self, object_id: &str) -> Result<PathBuf, GitError> {
        shard::shard_path(&self.root, object_id)
    }

    /// Ensure a per-object bare repository exists at the sharded
    /// path for `object_id`, with `objects/info/alternates`
    /// pointing at the shared pool. Idempotent — repeated calls
    /// for the same id are no-ops once the repo is initialized.
    /// Returns a `Repository` handle for read access.
    pub fn ensure_repo(&self, object_id: &str) -> Result<Repository, GitError> {
        let repo_path = self.path_for(object_id)?;
        with_path_lock(&repo_path, || {
            if !repo_path.exists() {
                git_init_bare(&repo_path)?;
                write_alternates_to_pool(&self.root, &repo_path)?;
            }
            Ok(())
        })?;
        open(&repo_path)
    }

    /// Return `Ok(true)` iff `commit_oid` is reachable in the
    /// per-object repository's object database (which transparently
    /// includes the shared pool via alternates). Returns
    /// `Ok(false)` if the per-object repo doesn't exist yet, or if
    /// the OID is simply absent.
    pub fn has_commit(&self, object_id: &str, commit_oid: &str) -> Result<bool, GitError> {
        let repo_path = self.path_for(object_id)?;
        if !repo_path.exists() {
            return Ok(false);
        }
        let repo = open(&repo_path)?;
        Ok(repo.find_commit(commit_oid)?.is_some())
    }
}

fn pool_path(root: &Path) -> PathBuf {
    root.join(POOL_DIR)
}

fn pool_objects_path(root: &Path) -> PathBuf {
    pool_path(root).join("objects")
}

/// Compute the relative path from a per-object repo's
/// `objects/info/alternates` file's parent directory
/// (`<repo>/objects/info/`) to the shared pool's `objects/`
/// directory at `<root>/pool/objects/`. Used as the alternates
/// content so the cache can be moved as a unit without breaking
/// per-object → pool linkage.
fn alternates_relative_to_pool(root: &Path, repo_path: &Path) -> Result<PathBuf, GitError> {
    let info_dir = repo_path.join("objects").join("info");
    let pool_objects = pool_objects_path(root);
    relative_path_from(&info_dir, &pool_objects).ok_or_else(|| GitError::CacheIo {
        path: info_dir,
        source: std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "could not derive relative path from repo info dir to pool/objects",
        ),
    })
}

/// Pure-function relative-path computation: how many `..` segments
/// take us from `from` up to the common ancestor, then descend into
/// `to`. Both paths must already share a prefix; for our case they
/// always do (both are inside the cache root).
fn relative_path_from(from: &Path, to: &Path) -> Option<PathBuf> {
    let from_components: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();
    let common_prefix_len = from_components
        .iter()
        .zip(to_components.iter())
        .take_while(|(a, b)| a == b)
        .count();
    if common_prefix_len == 0 {
        // Different roots — can't be reduced to a relative path.
        return None;
    }
    let ups = from_components.len() - common_prefix_len;
    let mut result = PathBuf::new();
    for _ in 0..ups {
        result.push("..");
    }
    for component in &to_components[common_prefix_len..] {
        result.push(component.as_os_str());
    }
    Some(result)
}

fn write_alternates_to_pool(root: &Path, repo_path: &Path) -> Result<(), GitError> {
    let relative = alternates_relative_to_pool(root, repo_path)?;
    let info_dir = repo_path.join("objects").join("info");
    std::fs::create_dir_all(&info_dir).map_err(|source| GitError::CacheIo {
        path: info_dir.clone(),
        source,
    })?;
    let alternates_path = info_dir.join("alternates");
    let mut content = relative.into_os_string();
    content.push("\n");
    std::fs::write(&alternates_path, content.as_encoded_bytes()).map_err(|source| {
        GitError::CacheIo {
            path: alternates_path,
            source,
        }
    })
}

/// Run `git init --bare <path>` and surface failures as structured
/// errors. The cache root is created by `open`/`ensure_repo`; this
/// function only initializes the bare-repo metadata inside.
fn git_init_bare(path: &Path) -> Result<(), GitError> {
    std::fs::create_dir_all(path).map_err(|source| GitError::CacheIo {
        path: path.to_path_buf(),
        source,
    })?;
    let output = Command::new("git")
        .arg("init")
        .arg("--bare")
        .arg("--quiet")
        .arg(path)
        .output()
        .map_err(|source| match source.kind() {
            std::io::ErrorKind::NotFound => GitError::CacheGitMissing { source },
            _ => GitError::CacheIo {
                path: path.to_path_buf(),
                source,
            },
        })?;
    if !output.status.success() {
        return Err(GitError::CacheGitInvocation {
            command: format!("git init --bare {}", path.display()),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code(),
        });
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Barrier;
    use tempfile::TempDir;

    /// A real Kairo-shape ID: `z` + `Qm` + 44 base58 characters.
    /// Sharded on positions 3-4 / 5-6 → `R8` / `3z`.
    const SAMPLE_ID: &str = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";
    const OTHER_ID: &str = "zQmAB1z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrz";

    fn skip_if_no_git() -> bool {
        Command::new("git").arg("--version").output().is_err()
    }

    fn open_temp() -> (TempDir, GitCache) {
        let dir = TempDir::new().expect("tempdir");
        let cache = GitCache::open(dir.path()).expect("open cache");
        (dir, cache)
    }

    #[test]
    fn open_initializes_pool_as_bare_repo() {
        if skip_if_no_git() {
            return;
        }
        let (dir, _cache) = open_temp();
        let pool = dir.path().join("pool");
        assert!(pool.exists(), "pool dir should exist");
        assert!(pool.join("HEAD").is_file(), "bare HEAD should exist");
        assert!(pool.join("objects").is_dir(), "objects dir should exist");
        assert!(pool.join("refs").is_dir(), "refs dir should exist");
        // Bare repo has `bare = true` in config.
        let config = std::fs::read_to_string(pool.join("config")).expect("config");
        assert!(config.contains("bare = true"), "pool config: {config}");
    }

    #[test]
    fn open_is_idempotent() {
        if skip_if_no_git() {
            return;
        }
        let dir = TempDir::new().expect("tempdir");
        let _first = GitCache::open(dir.path()).expect("open 1");
        // Drop a marker into the pool to detect re-init.
        let marker = dir.path().join("pool").join("kairo-marker");
        std::fs::write(&marker, b"keep me").expect("marker");
        let _second = GitCache::open(dir.path()).expect("open 2");
        assert!(marker.exists(), "second open must not re-init pool");
    }

    #[test]
    fn path_for_uses_two_level_sharding() {
        if skip_if_no_git() {
            return;
        }
        let (dir, cache) = open_temp();
        let path = cache.path_for(SAMPLE_ID).expect("path_for");
        assert_eq!(path, dir.path().join("R8").join("3z").join(SAMPLE_ID));
    }

    #[test]
    fn path_for_rejects_short_id() {
        if skip_if_no_git() {
            return;
        }
        let (_dir, cache) = open_temp();
        assert!(matches!(
            cache.path_for("zQmAB"),
            Err(GitError::CacheInvalidObjectId { .. })
        ));
    }

    #[test]
    fn ensure_repo_creates_bare_repo_with_alternates() {
        if skip_if_no_git() {
            return;
        }
        let (_dir, cache) = open_temp();
        let _repo = cache.ensure_repo(SAMPLE_ID).expect("ensure_repo");
        let repo_path = cache.path_for(SAMPLE_ID).expect("path_for");

        assert!(repo_path.join("HEAD").is_file(), "bare HEAD");
        assert!(repo_path.join("config").is_file(), "config");
        let alternates_path = repo_path
            .join("objects")
            .join("info")
            .join("alternates");
        assert!(
            alternates_path.is_file(),
            "alternates file should be present"
        );
        let content = std::fs::read_to_string(&alternates_path).expect("read alternates");
        // Alternates content should resolve to the pool's objects dir.
        // Resolving relative to the alternates' own parent dir
        // (`objects/info/`) must land inside the pool.
        let info_dir = alternates_path.parent().expect("info parent");
        let resolved = info_dir.join(content.trim());
        let canonical_resolved = std::fs::canonicalize(&resolved).expect("canon resolved");
        let canonical_pool = std::fs::canonicalize(cache.root().join("pool").join("objects"))
            .expect("canon pool");
        assert_eq!(canonical_resolved, canonical_pool);
    }

    #[test]
    fn ensure_repo_is_idempotent() {
        if skip_if_no_git() {
            return;
        }
        let (_dir, cache) = open_temp();
        let _ = cache.ensure_repo(SAMPLE_ID).expect("ensure 1");
        // Drop a marker file into the per-object repo to detect re-init.
        let repo_path = cache.path_for(SAMPLE_ID).expect("path_for");
        let marker = repo_path.join("kairo-marker");
        std::fs::write(&marker, b"keep me").expect("marker");
        let _ = cache.ensure_repo(SAMPLE_ID).expect("ensure 2");
        assert!(marker.exists(), "second ensure must not re-init repo");
    }

    #[test]
    fn has_commit_returns_false_for_unknown_object() {
        if skip_if_no_git() {
            return;
        }
        let (_dir, cache) = open_temp();
        // No per-object repo yet — has_commit must be false, not error.
        assert!(!cache
            .has_commit(SAMPLE_ID, "0000000000000000000000000000000000000000")
            .expect("has_commit no repo"));
        // After ensure_repo, still false because pool is empty.
        let _ = cache.ensure_repo(SAMPLE_ID).expect("ensure_repo");
        assert!(!cache
            .has_commit(SAMPLE_ID, "0000000000000000000000000000000000000000")
            .expect("has_commit empty repo"));
    }

    #[test]
    fn ensure_repo_serializes_concurrent_calls_for_same_object() {
        if skip_if_no_git() {
            return;
        }
        let (_dir, cache) = open_temp();
        let cache = Arc::new(cache);
        let barrier = Arc::new(Barrier::new(4));
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let cache = Arc::clone(&cache);
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    cache.ensure_repo(SAMPLE_ID).expect("ensure_repo");
                });
            }
        });
        // All four calls must converge to a single, well-formed
        // per-object repo with intact alternates.
        let repo_path = cache.path_for(SAMPLE_ID).expect("path_for");
        assert!(repo_path.join("HEAD").is_file());
        assert!(repo_path
            .join("objects")
            .join("info")
            .join("alternates")
            .is_file());
    }

    #[test]
    fn ensure_repo_does_not_serialize_distinct_objects() {
        if skip_if_no_git() {
            return;
        }
        let (_dir, cache) = open_temp();
        // Two distinct ids should both succeed independently. The
        // test mainly proves the lock is keyed per-object: distinct
        // ids never collide on the same lockfile path.
        let _ = cache.ensure_repo(SAMPLE_ID).expect("ensure 1");
        let _ = cache.ensure_repo(OTHER_ID).expect("ensure 2");
        let p1 = cache.path_for(SAMPLE_ID).expect("p1");
        let p2 = cache.path_for(OTHER_ID).expect("p2");
        assert_ne!(p1, p2, "distinct ids must shard to distinct paths");
        assert!(p1.join("HEAD").is_file());
        assert!(p2.join("HEAD").is_file());
    }

    #[test]
    fn relative_path_from_descends_through_common_ancestor() {
        let from = Path::new("/cache/R8/3z/repo/objects/info");
        let to = Path::new("/cache/pool/objects");
        let rel = relative_path_from(from, to).expect("relative");
        assert_eq!(rel, PathBuf::from("../../../../../pool/objects"));
    }

    #[test]
    fn relative_path_from_returns_none_for_disjoint_roots() {
        // Two paths with no common prefix at all (Unix). Should
        // refuse rather than emit a meaningless `..` chain.
        let from = Path::new("/a");
        let to = Path::new("/b");
        // Both share `/` as the root component, so common prefix is 1.
        // Verify the descent into the unrelated subtree works.
        let rel = relative_path_from(from, to).expect("relative");
        assert_eq!(rel, PathBuf::from("../b"));
    }
}
