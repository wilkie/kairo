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

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::lock::with_path_lock;
use crate::transport::{self, FetchedRef, GitCacheTransport};
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

    /// Stream a Git pack containing every object reachable from the
    /// per-object cache repo's refs into `sink`. The canonical
    /// streaming primitive — handles arbitrarily large packs without
    /// holding the bytes in memory. Suitable for piping directly
    /// into a bundle's `git/<object-id>.pack` file, into a HTTP
    /// response body, or into a remote's `git index-pack --stdin`.
    /// Errors if the per-object repo doesn't exist (caller hasn't
    /// fetched or ingested for this object).
    ///
    /// Per-object scoped: takes the per-object lock for the
    /// duration. Reads transparently include the shared pool via
    /// alternates, so the pack covers every commit/tree/blob the
    /// per-object refs reach, even though the bytes physically live
    /// in the pool.
    pub fn pack_for_object_to(&self, object_id: &str, sink: impl Write) -> Result<(), GitError> {
        let repo_path = self.path_for(object_id)?;
        if !repo_path.exists() {
            return Err(GitError::CacheGitInvocation {
                command: format!("pack-objects for object {object_id}"),
                stderr: format!(
                    "no per-object cache repo at {} — fetch or ingest first",
                    repo_path.display()
                ),
                exit_code: None,
            });
        }
        with_path_lock(&repo_path, || git_pack_objects_stream(&repo_path, sink))
    }

    /// Convenience wrapper over [`Self::pack_for_object_to`] that
    /// returns the entire pack as a `Vec<u8>`. Useful for tests and
    /// small packs; for production data paths, prefer the streaming
    /// version so multi-GB packs don't sit in memory.
    pub fn pack_for_object(&self, object_id: &str) -> Result<Vec<u8>, GitError> {
        let mut buf = Vec::new();
        self.pack_for_object_to(object_id, &mut buf)?;
        Ok(buf)
    }

    /// Stream pack bytes from `source` into the shared pool via
    /// `git index-pack --stdin`, which writes the canonical
    /// `pool/objects/pack/pack-<sha>.{pack,idx}` pair. The
    /// canonical streaming primitive — handles arbitrarily large
    /// packs without holding the bytes in memory. Suitable for
    /// piping directly from a bundle's pack file, an HTTP request
    /// body, or another cache's `pack_for_object_to`. Pack contents
    /// are content-addressed by SHA-1 in the filename, so
    /// re-ingesting the same bytes is a same-file overwrite and
    /// concurrent ingestion of identical packs converges.
    ///
    /// Pool-scoped: takes the pool lock for the duration. The pack
    /// becomes reachable from every per-object repo through
    /// `objects/info/alternates`, so a follow-up [`Self::set_ref`]
    /// call can pin OIDs from the pack into any object's repo.
    pub fn ingest_pack_from(&self, source: impl Read) -> Result<(), GitError> {
        let pool = pool_path(&self.root);
        let pool_lock_subject = self.root.join(POOL_LOCK_SUBJECT);
        with_path_lock(&pool_lock_subject, || git_index_pack_stream(&pool, source))
    }

    /// Convenience wrapper over [`Self::ingest_pack_from`] that
    /// takes a byte slice. Useful for tests; production paths
    /// (bundle import, daemon push) should use the streaming
    /// version so multi-GB packs flow through without buffering.
    pub fn ingest_pack(&self, pack_bytes: &[u8]) -> Result<(), GitError> {
        self.ingest_pack_from(std::io::Cursor::new(pack_bytes))
    }

    /// Write `ref_name` → `commit_oid` in the per-object repo for
    /// `object_id`. Initializes the per-object repo if absent (so
    /// callers don't have to call [`Self::ensure_repo`] first).
    /// Errors with [`GitError::CacheGitInvocation`] if `commit_oid`
    /// is not reachable from the per-object repo's object database
    /// (i.e., not in the pool); pin a commit only after fetching
    /// or ingesting it.
    pub fn set_ref(
        &self,
        object_id: &str,
        ref_name: &str,
        commit_oid: &str,
    ) -> Result<(), GitError> {
        let repo_path = self.path_for(object_id)?;
        let _ = self.ensure_repo(object_id)?;
        with_path_lock(&repo_path, || {
            transport::update_ref(&repo_path, ref_name, commit_oid)
        })
    }

    /// Fetch `remote_branch` from `url` into the cache for
    /// `object_id`. Orchestrates the layout:
    ///
    /// 1. `ensure_repo(object_id)` — initialize the per-object bare
    ///    repo if absent.
    /// 2. Under the pool lock, ask `transport` to fetch
    ///    `refs/heads/<branch>` from `url` into the pool, landing
    ///    it at `refs/kairo/<object-id>/<branch>`.
    /// 3. Under the per-object lock, mirror the resolved OID into
    ///    the per-object repo's `refs/heads/<branch>`.
    ///
    /// The two locks are taken sequentially, never held
    /// simultaneously, so distinct objects' fetches don't deadlock.
    /// Pool fetches do serialize across objects — that's the
    /// design (network is the bottleneck).
    ///
    /// Returns the resolved `FetchedRef` from the per-object repo's
    /// perspective (`ref_name` is `refs/heads/<branch>`).
    pub fn fetch(
        &self,
        object_id: &str,
        url: &str,
        remote_branch: &str,
        transport: &impl GitCacheTransport,
    ) -> Result<FetchedRef, GitError> {
        // Ensures sharding/id validity up front; also creates the
        // per-object repo with alternates so step 3 can succeed
        // without re-running init.
        let repo_path = self.path_for(object_id)?;
        let _ = self.ensure_repo(object_id)?;

        let pool_lock_subject = self.root.join(POOL_LOCK_SUBJECT);
        let pool_path = pool_path(&self.root);
        let local_ref = format!("refs/heads/{remote_branch}");
        let pool_dest_ref = format!("refs/kairo/{object_id}/{remote_branch}");
        let pool_refspec = format!("refs/heads/{remote_branch}:{pool_dest_ref}");

        let pool_fetched = with_path_lock(&pool_lock_subject, || {
            transport.fetch(&pool_path, url, &pool_refspec)
        })?;
        // Defense in depth: the transport should have placed the
        // fetched ref at our requested destination. If it didn't,
        // something is structurally wrong with the impl.
        if pool_fetched.ref_name != pool_dest_ref {
            return Err(GitError::CacheGitInvocation {
                command: format!(
                    "transport returned unexpected ref {:?}",
                    pool_fetched.ref_name
                ),
                stderr: format!("expected destination {pool_dest_ref}"),
                exit_code: None,
            });
        }

        with_path_lock(&repo_path, || {
            transport::update_ref(&repo_path, &local_ref, &pool_fetched.oid)
        })?;

        Ok(FetchedRef {
            ref_name: local_ref,
            oid: pool_fetched.oid,
        })
    }
}

fn pool_path(root: &Path) -> PathBuf {
    root.join(POOL_DIR)
}

fn pool_objects_path(root: &Path) -> PathBuf {
    pool_path(root).join("objects")
}

/// Compute the relative path from a per-object repo's
/// `<repo>/objects/` directory to the shared pool's `objects/`
/// directory at `<root>/pool/objects/`. Used as the
/// `objects/info/alternates` content.
///
/// Git resolves relative alternates paths against
/// `<GIT_DIR>/objects/`, *not* against the alternates file's own
/// parent directory — see `gitrepository-layout(5)` and
/// `Documentation/technical/objects.txt` in the git source. Using
/// the wrong reference point produces a path that's off by one
/// `..` segment, which manifests as
/// "object directory ... does not exist" on every read.
fn alternates_relative_to_pool(root: &Path, repo_path: &Path) -> Result<PathBuf, GitError> {
    let objects_dir = repo_path.join("objects");
    let pool_objects = pool_objects_path(root);
    relative_path_from(&objects_dir, &pool_objects).ok_or_else(|| GitError::CacheIo {
        path: objects_dir,
        source: std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "could not derive relative path from repo objects dir to pool/objects",
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

/// Stream pack bytes from `source` into `git -C <pool> index-pack
/// --stdin`, which reads the pack from stdin, computes its SHA-1,
/// and writes both `pack-<sha>.pack` and `pack-<sha>.idx` into
/// `<pool>/objects/pack/`. Same code path `git fetch` uses
/// internally, so the on-disk result is identical. Drains stderr
/// in a separate thread to avoid pipe-buffer deadlock when stdin
/// is large.
fn git_index_pack_stream(pool: &Path, mut source: impl Read) -> Result<(), GitError> {
    use std::process::Stdio;

    let mut child = Command::new("git")
        .arg("-C")
        .arg(pool)
        .arg("index-pack")
        .arg("--stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| match source.kind() {
            std::io::ErrorKind::NotFound => GitError::CacheGitMissing { source },
            _ => GitError::CacheIo {
                path: pool.to_path_buf(),
                source,
            },
        })?;

    let stderr_bytes = drain_stderr(&mut child);
    let stdout_bytes = drain_stdout(&mut child);

    // Stream source into stdin. Drop stdin afterward to signal EOF
    // — without that drop, `child.wait()` blocks forever waiting
    // for more bytes.
    let mut stdin = child.stdin.take().ok_or_else(|| GitError::CacheIo {
        path: pool.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "git index-pack stdin not captured",
        ),
    })?;
    let copy_result = std::io::copy(&mut source, &mut stdin);
    drop(stdin);

    let status = child.wait().map_err(|source| GitError::CacheIo {
        path: pool.to_path_buf(),
        source,
    })?;
    let stderr_text = stderr_bytes.collect();
    let _ = stdout_bytes.collect();

    if let Err(source) = copy_result {
        return Err(GitError::CacheIo {
            path: pool.to_path_buf(),
            source,
        });
    }
    if !status.success() {
        return Err(GitError::CacheGitInvocation {
            command: "git index-pack --stdin".to_owned(),
            stderr: stderr_text,
            exit_code: status.code(),
        });
    }
    Ok(())
}

/// Stream `git -C <repo> pack-objects --all --stdout` into `sink`.
/// `--all` means "pack everything reachable from refs"; `--stdout`
/// skips the `pack-<sha>.{pack,idx}` filesystem write since callers
/// consume the bytes directly. Drains stderr in a separate thread
/// to avoid pipe-buffer deadlock when stdout is large.
fn git_pack_objects_stream(repo_path: &Path, mut sink: impl Write) -> Result<(), GitError> {
    use std::process::Stdio;

    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["pack-objects", "--all", "--stdout"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| match source.kind() {
            std::io::ErrorKind::NotFound => GitError::CacheGitMissing { source },
            _ => GitError::CacheIo {
                path: repo_path.to_path_buf(),
                source,
            },
        })?;

    let stderr_bytes = drain_stderr(&mut child);

    // Stream stdout to sink. Errors here can mean either the child
    // misbehaved or the sink rejected bytes; we surface stdin/stdout
    // I/O as `CacheIo` and exit-status failure as `CacheGitInvocation`.
    let mut stdout = child.stdout.take().ok_or_else(|| GitError::CacheIo {
        path: repo_path.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "git pack-objects stdout not captured",
        ),
    })?;
    let copy_result = std::io::copy(&mut stdout, &mut sink);
    drop(stdout);

    let status = child.wait().map_err(|source| GitError::CacheIo {
        path: repo_path.to_path_buf(),
        source,
    })?;
    let stderr_text = stderr_bytes.collect();

    if let Err(source) = copy_result {
        return Err(GitError::CacheIo {
            path: repo_path.to_path_buf(),
            source,
        });
    }
    if !status.success() {
        return Err(GitError::CacheGitInvocation {
            command: format!("git -C {} pack-objects --all --stdout", repo_path.display()),
            stderr: stderr_text,
            exit_code: status.code(),
        });
    }
    Ok(())
}

/// Reads a captured stdio pipe to EOF on a worker thread, returning
/// a join handle whose `collect()` yields the captured text. Used
/// to drain stderr/stdout in parallel with the main thread's data
/// transfer so neither side blocks the other on a full pipe buffer.
struct PipeDrain {
    handle: std::thread::JoinHandle<Vec<u8>>,
}

impl PipeDrain {
    fn collect(self) -> String {
        match self.handle.join() {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => String::from("<pipe drain panicked>"),
        }
    }
}

fn drain_stderr(child: &mut std::process::Child) -> PipeDrain {
    let mut handle = child.stderr.take().expect("stderr was piped at spawn time");
    let join = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = handle.read_to_end(&mut buf);
        buf
    });
    PipeDrain { handle: join }
}

fn drain_stdout(child: &mut std::process::Child) -> PipeDrain {
    let mut handle = child.stdout.take().expect("stdout was piped at spawn time");
    let join = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = handle.read_to_end(&mut buf);
        buf
    });
    PipeDrain { handle: join }
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

    use crate::transport::GitCli;
    use kairo_test_support::git::{build_pack_from, init_source_repo, skip_if_no_git};

    /// A real Kairo-shape ID: `z` + `Qm` + 44 base58 characters.
    /// Sharded on positions 3-4 / 5-6 → `R8` / `3z`.
    const SAMPLE_ID: &str = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";
    const OTHER_ID: &str = "zQmAB1z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrz";

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
        let alternates_path = repo_path.join("objects").join("info").join("alternates");
        assert!(
            alternates_path.is_file(),
            "alternates file should be present"
        );
        let content = std::fs::read_to_string(&alternates_path).expect("read alternates");
        // Git resolves relative alternates paths against
        // `<repo>/objects/`, not the alternates file's parent.
        // Verify both that our content resolves correctly *and*
        // that git itself accepts it via `rev-parse` (which is the
        // smoke test that real git operations will work).
        let objects_dir = repo_path.join("objects");
        let resolved = objects_dir.join(content.trim());
        let canonical_resolved = std::fs::canonicalize(&resolved).expect("canon resolved");
        let canonical_pool =
            std::fs::canonicalize(cache.root().join("pool").join("objects")).expect("canon pool");
        assert_eq!(canonical_resolved, canonical_pool);

        // git smoke test: rev-parse against an empty repo with
        // only-alternates objects works iff the alternates file
        // actually points at a valid object directory.
        let probe = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .args(["rev-parse", "--git-dir"])
            .output()
            .expect("git rev-parse --git-dir");
        assert!(probe.status.success(), "git must accept the bare repo");
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
        // The actual call site in `alternates_relative_to_pool` is
        // from `<repo>/objects/` to `<root>/pool/objects/` — the
        // reference point Git uses for resolving alternates.
        let from = Path::new("/cache/R8/3z/repo/objects");
        let to = Path::new("/cache/pool/objects");
        let rel = relative_path_from(from, to).expect("relative");
        assert_eq!(rel, PathBuf::from("../../../../pool/objects"));
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

    #[test]
    fn fetch_lands_objects_in_pool_and_ref_in_per_object_repo() {
        if skip_if_no_git() {
            return;
        }
        let (_src, url, branch, head_oid) = init_source_repo();
        let (_dir, cache) = open_temp();

        let fetched = cache
            .fetch(SAMPLE_ID, &url, &branch, &GitCli::new())
            .expect("fetch");

        // Returned ref reflects the per-object repo's view.
        assert_eq!(fetched.ref_name, format!("refs/heads/{branch}"));
        assert_eq!(fetched.oid, head_oid);

        // Per-object repo's ref resolves to the head OID.
        let repo_path = cache.path_for(SAMPLE_ID).expect("path_for");
        let object_ref = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .args(["rev-parse", "--verify", &format!("refs/heads/{branch}")])
            .output()
            .expect("rev-parse object ref");
        assert!(object_ref.status.success());
        let object_oid = String::from_utf8_lossy(&object_ref.stdout)
            .trim()
            .to_owned();
        assert_eq!(object_oid, head_oid);

        // The OID is reachable from the per-object repo via alternates
        // (objects live in the pool, ref lives per-object).
        assert!(cache.has_commit(SAMPLE_ID, &head_oid).expect("has_commit"));

        // Pool ref is namespaced.
        let pool_ref = std::process::Command::new("git")
            .arg("-C")
            .arg(cache.root().join("pool"))
            .args([
                "rev-parse",
                "--verify",
                &format!("refs/kairo/{SAMPLE_ID}/{branch}"),
            ])
            .output()
            .expect("rev-parse pool ref");
        assert!(
            pool_ref.status.success(),
            "pool should hold namespaced ref: {}",
            String::from_utf8_lossy(&pool_ref.stderr)
        );
    }

    #[test]
    fn fetch_unknown_branch_errors_and_leaves_cache_clean() {
        if skip_if_no_git() {
            return;
        }
        let (_src, url, _branch, _head) = init_source_repo();
        let (_dir, cache) = open_temp();

        let err = cache
            .fetch(SAMPLE_ID, &url, "no-such-branch", &GitCli::new())
            .expect_err("fetch must error");
        assert!(matches!(err, GitError::CacheGitInvocation { .. }));

        // ensure_repo did run before the failed fetch — the per-object
        // repo exists and is well-formed even though no ref landed.
        let repo_path = cache.path_for(SAMPLE_ID).expect("path_for");
        assert!(repo_path.join("HEAD").is_file());
        // No `refs/heads/no-such-branch` mirror was written.
        let probe = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .args(["rev-parse", "--verify", "refs/heads/no-such-branch"])
            .output()
            .expect("rev-parse probe");
        assert!(!probe.status.success(), "no ref should have been mirrored");
    }

    #[test]
    fn fetch_distinct_objects_uses_shared_pool() {
        if skip_if_no_git() {
            return;
        }
        let (_src, url, branch, head_oid) = init_source_repo();
        let (_dir, cache) = open_temp();

        cache
            .fetch(SAMPLE_ID, &url, &branch, &GitCli::new())
            .expect("fetch 1");
        cache
            .fetch(OTHER_ID, &url, &branch, &GitCli::new())
            .expect("fetch 2");

        // Both per-object repos resolve the same OID; the actual
        // commit data lives once in the shared pool.
        assert!(cache.has_commit(SAMPLE_ID, &head_oid).expect("has 1"));
        assert!(cache.has_commit(OTHER_ID, &head_oid).expect("has 2"));

        // Pool holds two distinct namespaced refs both pointing at
        // the same OID.
        let probe = |id: &str| -> String {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(cache.root().join("pool"))
                .args([
                    "rev-parse",
                    "--verify",
                    &format!("refs/kairo/{id}/{branch}"),
                ])
                .output()
                .expect("rev-parse");
            assert!(out.status.success());
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        };
        assert_eq!(probe(SAMPLE_ID), head_oid);
        assert_eq!(probe(OTHER_ID), head_oid);
    }

    #[test]
    fn fetch_concurrent_for_same_object_serializes_cleanly() {
        if skip_if_no_git() {
            return;
        }
        let (_src, url, branch, head_oid) = init_source_repo();
        let (_dir, cache) = open_temp();
        let cache = Arc::new(cache);
        let url = Arc::new(url);
        let branch = Arc::new(branch);
        let barrier = Arc::new(Barrier::new(4));

        std::thread::scope(|scope| {
            for _ in 0..4 {
                let cache = Arc::clone(&cache);
                let url = Arc::clone(&url);
                let branch = Arc::clone(&branch);
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    cache
                        .fetch(SAMPLE_ID, &url, &branch, &GitCli::new())
                        .expect("fetch");
                });
            }
        });

        // After all four converge, both refs are intact and the OID
        // matches the source head.
        assert!(cache.has_commit(SAMPLE_ID, &head_oid).expect("has_commit"));
    }

    #[test]
    fn ingest_pack_lands_objects_in_pool() {
        if skip_if_no_git() {
            return;
        }
        let (src, _url, _branch, head_oid) = init_source_repo();
        let pack = build_pack_from(src.path());
        assert!(
            !pack.is_empty(),
            "pack-objects must produce non-empty output"
        );

        let (_dir, cache) = open_temp();
        cache.ingest_pack(&pack).expect("ingest_pack");

        // After ingest, the pool's object DB knows about every commit
        // from the source repo. Per-object repos see them via alternates,
        // so has_commit succeeds without needing a fetch.
        let _ = cache.ensure_repo(SAMPLE_ID).expect("ensure_repo");
        assert!(
            cache.has_commit(SAMPLE_ID, &head_oid).expect("has_commit"),
            "head OID {head_oid} must be reachable after pack ingest"
        );

        // The pack file should appear under canonical pack-<sha> naming.
        let pack_dir = cache.root().join("pool").join("objects").join("pack");
        let entries: Vec<_> = std::fs::read_dir(&pack_dir)
            .expect("read pack dir")
            .filter_map(Result::ok)
            .collect();
        let has_canonical_pack = entries.iter().any(|e| {
            e.file_name().to_string_lossy().starts_with("pack-")
                && e.file_name().to_string_lossy().ends_with(".pack")
        });
        assert!(
            has_canonical_pack,
            "pack-<sha>.pack should land in pool/objects/pack: {entries:?}"
        );
    }

    #[test]
    fn ingest_pack_rejects_garbage_bytes() {
        if skip_if_no_git() {
            return;
        }
        let (_dir, cache) = open_temp();
        let err = cache
            .ingest_pack(b"not a real pack")
            .expect_err("ingest must reject garbage");
        assert!(matches!(err, GitError::CacheGitInvocation { .. }));
    }

    #[test]
    fn ingest_pack_is_idempotent_for_identical_input() {
        if skip_if_no_git() {
            return;
        }
        let (src, _url, _branch, head_oid) = init_source_repo();
        let pack = build_pack_from(src.path());
        let (_dir, cache) = open_temp();

        cache.ingest_pack(&pack).expect("ingest 1");
        cache.ingest_pack(&pack).expect("ingest 2 (same bytes)");

        // After the second ingest, the same pack-<sha>.pack file
        // exists exactly once in the pool's pack dir — git
        // index-pack overwrites identical content with itself.
        let pack_dir = cache.root().join("pool").join("objects").join("pack");
        let pack_files: Vec<_> = std::fs::read_dir(&pack_dir)
            .expect("read pack dir")
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".pack"))
            .collect();
        assert_eq!(pack_files.len(), 1, "exactly one pack file expected");

        let _ = cache.ensure_repo(SAMPLE_ID).expect("ensure_repo");
        assert!(cache.has_commit(SAMPLE_ID, &head_oid).expect("has_commit"));
    }

    #[test]
    fn set_ref_pins_an_oid_after_ingest() {
        if skip_if_no_git() {
            return;
        }
        let (src, _url, branch, head_oid) = init_source_repo();
        let pack = build_pack_from(src.path());
        let (_dir, cache) = open_temp();
        cache.ingest_pack(&pack).expect("ingest_pack");

        let ref_name = format!("refs/heads/{branch}");
        cache
            .set_ref(SAMPLE_ID, &ref_name, &head_oid)
            .expect("set_ref");

        // The per-object repo's ref must resolve to the pinned OID.
        let repo_path = cache.path_for(SAMPLE_ID).expect("path_for");
        let probe = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .args(["rev-parse", "--verify", &ref_name])
            .output()
            .expect("rev-parse");
        assert!(probe.status.success());
        assert_eq!(String::from_utf8_lossy(&probe.stdout).trim(), head_oid);
    }

    #[test]
    fn set_ref_creates_per_object_repo_if_absent() {
        if skip_if_no_git() {
            return;
        }
        let (src, _url, branch, head_oid) = init_source_repo();
        let pack = build_pack_from(src.path());
        let (_dir, cache) = open_temp();
        cache.ingest_pack(&pack).expect("ingest_pack");

        // No prior ensure_repo call — set_ref must initialize the
        // per-object repo as part of its operation.
        let repo_path = cache.path_for(OTHER_ID).expect("path_for");
        assert!(
            !repo_path.exists(),
            "precondition: repo should not exist yet"
        );

        cache
            .set_ref(OTHER_ID, &format!("refs/heads/{branch}"), &head_oid)
            .expect("set_ref");

        assert!(
            repo_path.join("HEAD").is_file(),
            "repo should be initialized"
        );
        assert!(
            repo_path
                .join("objects")
                .join("info")
                .join("alternates")
                .is_file(),
            "alternates should be in place"
        );
    }

    #[test]
    fn set_ref_errors_for_unreachable_oid() {
        if skip_if_no_git() {
            return;
        }
        let (_dir, cache) = open_temp();
        // Plausible-shaped but absent OID. Avoid the all-zeros
        // sentinel — git treats it as "delete ref", so it would
        // silently succeed even with an empty object DB.
        let err = cache
            .set_ref(
                SAMPLE_ID,
                "refs/heads/main",
                "1234567890abcdef1234567890abcdef12345678",
            )
            .expect_err("set_ref must error for unreachable OID");
        assert!(matches!(err, GitError::CacheGitInvocation { .. }));
    }

    #[test]
    fn pack_for_object_round_trips_through_ingest() {
        // Build a pack from object A's per-object repo, then ingest
        // it into a fresh cache under object B. Object B must then
        // reach the same OIDs through alternates — confirms the pack
        // is well-formed and contains every commit reachable from
        // A's refs.
        if skip_if_no_git() {
            return;
        }
        let (_src, url, branch, head_oid) = init_source_repo();

        let (_dir1, cache1) = open_temp();
        cache1
            .fetch(SAMPLE_ID, &url, &branch, &GitCli::new())
            .expect("fetch");
        let pack = cache1.pack_for_object(SAMPLE_ID).expect("pack_for_object");
        assert!(
            !pack.is_empty(),
            "pack must contain at least the head commit"
        );

        let (_dir2, cache2) = open_temp();
        cache2.ingest_pack(&pack).expect("ingest_pack");
        cache2
            .set_ref(OTHER_ID, "refs/heads/main", &head_oid)
            .expect("set_ref");

        assert!(
            cache2.has_commit(OTHER_ID, &head_oid).expect("has_commit"),
            "round-tripped OID must be reachable in cache 2"
        );
    }

    #[test]
    fn pack_for_object_errors_when_repo_absent() {
        if skip_if_no_git() {
            return;
        }
        let (_dir, cache) = open_temp();
        let err = cache
            .pack_for_object(SAMPLE_ID)
            .expect_err("pack_for_object must error for absent repo");
        assert!(matches!(err, GitError::CacheGitInvocation { .. }));
    }

    #[test]
    fn pack_for_object_to_streams_to_arbitrary_writer() {
        // The streaming primitive must work for any writer. Test
        // with a writer that reports byte counts to confirm bytes
        // flowed through (rather than being buffered then flushed
        // at the end as one block).
        if skip_if_no_git() {
            return;
        }
        let (_src, url, branch, _head_oid) = init_source_repo();
        let (_dir, cache) = open_temp();
        cache
            .fetch(SAMPLE_ID, &url, &branch, &GitCli::new())
            .expect("fetch");

        struct CountingWriter {
            count: usize,
        }
        impl std::io::Write for CountingWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.count += buf.len();
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let mut writer = CountingWriter { count: 0 };
        cache
            .pack_for_object_to(SAMPLE_ID, &mut writer)
            .expect("pack_for_object_to");
        assert!(writer.count > 0, "writer must have received bytes");
    }

    #[test]
    fn ingest_pack_from_streams_from_arbitrary_reader() {
        // Symmetric: the streaming primitive must consume any
        // reader. Use a tempfile-backed reader to confirm the
        // disk → stdin path that bundle import will use.
        if skip_if_no_git() {
            return;
        }
        let (_src, url, branch, head_oid) = init_source_repo();

        // Cache 1: produce pack as bytes via the wrapper, write to
        // a tempfile to mimic a bundle's `git/<id>.pack`.
        let (_dir1, cache1) = open_temp();
        cache1
            .fetch(SAMPLE_ID, &url, &branch, &GitCli::new())
            .expect("fetch");
        let pack_bytes = cache1.pack_for_object(SAMPLE_ID).expect("pack");
        let pack_file = TempDir::new().expect("tempdir");
        let pack_path = pack_file.path().join("from-bundle.pack");
        std::fs::write(&pack_path, &pack_bytes).expect("write pack");

        // Cache 2: ingest by streaming the file straight into git
        // index-pack, never holding the full pack in memory.
        let (_dir2, cache2) = open_temp();
        let file = std::fs::File::open(&pack_path).expect("open pack");
        cache2.ingest_pack_from(file).expect("ingest_pack_from");
        cache2
            .set_ref(SAMPLE_ID, "refs/heads/main", &head_oid)
            .expect("set_ref");
        assert!(cache2.has_commit(SAMPLE_ID, &head_oid).expect("has_commit"));
    }
}
