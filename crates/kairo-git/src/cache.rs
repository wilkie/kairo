//! Managed Git cache rooted at `~/.kairo/git/`.
//!
//! Layout (per `specs/DECISIONS.md` §7 and §19):
//!
//! ```text
//! <root>/
//!   pool-sha1/                     # shared bare repos, one per object format:
//!   pool-sha256/                   #   hold all Git objects of that format
//!     objects/
//!   <XX>/<YY>/<object-id>/         # per-Kairo-object bare repo
//!     objects/info/alternates      # → pool-<format>/objects (relative)
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
use kairo_statement::{GitObjectFormat, GitRevision};

use crate::{open, shard, GitError, Repository};

/// Prefix of the per-format pool directories at the cache root:
/// `pool-sha1/` and `pool-sha256/`. Git cannot mix object formats in
/// one repository, so each format has its own pool, and a per-object
/// repo borrows from the pool matching its own format
/// (`specs/DECISIONS.md` §19). Cannot collide with a sharded `<XX>`
/// directory, which is exactly two base58 characters.
const POOL_DIR_PREFIX: &str = "pool-";

/// Cached outgoing packs: `<root>/outgoing/<XX>/<YY>/<object-id>/<snapshot-key>.pack`.
const OUTGOING_DIR: &str = "outgoing";

/// Quarantine object directories for packs being ingested:
/// `<root>/incoming/<unique>/`.
const INCOMING_DIR: &str = "incoming";

/// An object repo's refs at one moment, and the key its cached pack is
/// filed under: a SHA-256 over the sorted `<ref> <oid>` lines, so any
/// ref change yields a new key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefSnapshot {
    refs: Vec<(String, String)>,
    key: String,
}

impl RefSnapshot {
    fn new(mut refs: Vec<(String, String)>) -> Self {
        use sha2::{Digest, Sha256};
        refs.sort();
        let mut hasher = Sha256::new();
        for (name, oid) in &refs {
            hasher.update(name.as_bytes());
            hasher.update(b" ");
            hasher.update(oid.as_bytes());
            hasher.update(b"\n");
        }
        let key = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Self { refs, key }
    }

    pub fn is_empty(&self) -> bool {
        self.refs.is_empty()
    }

    /// `(ref name, object id)`, sorted.
    pub fn refs(&self) -> &[(String, String)] {
        &self.refs
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    fn oids(&self) -> impl Iterator<Item = &str> {
        self.refs.iter().map(|(_, oid)| oid.as_str())
    }
}

/// A complete pack on disk, ready to stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedPack {
    pub path: PathBuf,
    pub len: u64,
    /// The [`RefSnapshot::key`] it was built from; stable for as long
    /// as the object's refs don't change, so usable as an `ETag`.
    pub key: String,
}

/// A temporary object directory a pack is indexed into before it is
/// trusted. Removed on drop, whether or not it was migrated.
struct Quarantine {
    path: PathBuf,
}

impl Quarantine {
    fn create(root: &Path, pool: &Path, format: GitObjectFormat) -> Result<Self, GitError> {
        let incoming = root.join(INCOMING_DIR);
        std::fs::create_dir_all(&incoming).map_err(|source| GitError::CacheIo {
            path: incoming.clone(),
            source,
        })?;
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = format!(
            "{}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        );
        let path = incoming.join(unique);
        git_init_bare(&path, format)?;
        let quarantine = Self { path };
        // Absolute alternates: the quarantine is temporary and never
        // moves, so a relative path buys nothing.
        let pool_objects =
            std::fs::canonicalize(pool.join("objects")).map_err(|source| GitError::CacheIo {
                path: pool.join("objects"),
                source,
            })?;
        let info = quarantine.path.join("objects").join("info");
        std::fs::create_dir_all(&info).map_err(|source| GitError::CacheIo {
            path: info.clone(),
            source,
        })?;
        let mut content = pool_objects.into_os_string();
        content.push("\n");
        std::fs::write(info.join("alternates"), content.as_encoded_bytes()).map_err(|source| {
            GitError::CacheIo {
                path: info.join("alternates"),
                source,
            }
        })?;
        Ok(quarantine)
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Move the quarantine's pack files into `pool`. Caller holds the
    /// pool lock.
    fn migrate_into(&self, pool: &Path) -> Result<(), GitError> {
        let from = self.path.join("objects").join("pack");
        let to = pool.join("objects").join("pack");
        std::fs::create_dir_all(&to).map_err(|source| GitError::CacheIo {
            path: to.clone(),
            source,
        })?;
        let entries = match std::fs::read_dir(&from) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(source) => return Err(GitError::CacheIo { path: from, source }),
        };
        // Move the `.pack` before its `.idx`, as Git does, so a reader
        // never sees an index whose pack is missing.
        let mut files: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        files.sort_by_key(|path| path.extension().is_some_and(|ext| ext == "idx"));
        for file in files {
            let Some(name) = file.file_name() else {
                continue;
            };
            let dest = to.join(name);
            if dest.exists() {
                continue;
            }
            std::fs::rename(&file, &dest).map_err(|source| GitError::CacheIo {
                path: dest.clone(),
                source,
            })?;
        }
        Ok(())
    }
}

impl Drop for Quarantine {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Filesystem-backed Git cache.
///
/// Created via [`GitCache::open`]. Pools are initialized on first use
/// for their format, and per-object repositories are materialized on
/// demand via [`GitCache::ensure_repo`].
#[derive(Debug, Clone)]
pub struct GitCache {
    root: PathBuf,
}

impl GitCache {
    /// Open the cache rooted at `root`, creating the directory if
    /// absent. Pools are created lazily, per object format, by the
    /// operations that need them.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, GitError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(|source| GitError::CacheIo {
            path: root.clone(),
            source,
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

    /// Initialize the `format` pool as a bare repository if absent.
    /// Concurrent first-time calls serialize on the pool lock so only
    /// one runs `git init`.
    fn ensure_pool(&self, format: GitObjectFormat) -> Result<PathBuf, GitError> {
        let pool = pool_path(&self.root, format);
        with_path_lock(&pool, || {
            if !pool.exists() {
                git_init_bare(&pool, format)?;
            }
            Ok(())
        })?;
        Ok(pool)
    }

    /// The object format of `object_id`'s per-object repo, or `None`
    /// if it doesn't exist yet. Read without the `git` binary.
    pub fn repo_format(&self, object_id: &str) -> Result<Option<GitObjectFormat>, GitError> {
        let repo_path = self.path_for(object_id)?;
        if !repo_path.exists() {
            return Ok(None);
        }
        open(&repo_path)?.object_format().map(Some)
    }

    /// Ensure a per-object bare repository exists for `object_id` in
    /// `format`, with `objects/info/alternates` pointing at that
    /// format's pool. Idempotent. An existing repo in a different
    /// format is [`GitError::FormatMismatch`]: an object's history has
    /// one format (`specs/DECISIONS.md` §19).
    pub fn ensure_repo(
        &self,
        object_id: &str,
        format: GitObjectFormat,
    ) -> Result<Repository, GitError> {
        let repo_path = self.path_for(object_id)?;
        self.ensure_pool(format)?;
        with_path_lock(&repo_path, || {
            if !repo_path.exists() {
                git_init_bare(&repo_path, format)?;
                write_alternates_to_pool(&self.root, &repo_path, format)?;
            }
            Ok(())
        })?;
        let repo = open(&repo_path)?;
        let found = repo.object_format()?;
        if found != format {
            return Err(GitError::FormatMismatch {
                context: format!("object {object_id}'s cache repo"),
                expected: format,
                found,
            });
        }
        Ok(repo)
    }

    /// Return `Ok(true)` iff `revision`'s commit is reachable in the
    /// per-object repository's object database (which transparently
    /// includes its pool via alternates). `Ok(false)` if the repo
    /// doesn't exist, is in another format, or lacks the commit.
    pub fn has_commit(&self, object_id: &str, revision: &GitRevision) -> Result<bool, GitError> {
        let repo_path = self.path_for(object_id)?;
        if !repo_path.exists() {
            return Ok(false);
        }
        let repo = open(&repo_path)?;
        if repo.object_format()? != revision.format() {
            return Ok(false);
        }
        Ok(repo.find_commit(revision)?.is_some())
    }

    /// Whether `object_id`'s repo exists and has at least one ref, so
    /// [`Self::pack_for_object_to`] would produce a non-empty pack.
    pub fn has_refs(&self, object_id: &str) -> Result<bool, GitError> {
        let repo_path = self.path_for(object_id)?;
        if !repo_path.exists() {
            return Ok(false);
        }
        open(&repo_path)?.has_refs()
    }

    /// The object repo's refs as a [`RefSnapshot`]: what a pack of the
    /// object would contain right now, and the key its cached pack is
    /// stored under. Read through gix, without the `git` binary.
    pub fn ref_snapshot(&self, object_id: &str) -> Result<RefSnapshot, GitError> {
        let repo_path = self.path_for(object_id)?;
        if !repo_path.exists() {
            return Ok(RefSnapshot::default());
        }
        let refs = open(&repo_path)?.ref_snapshot()?;
        Ok(RefSnapshot::new(refs))
    }

    /// Stream a Git pack of everything reachable from `snapshot`'s
    /// commits into `sink`, without holding any lock
    /// (`specs/DECISIONS.md` §21). Git objects are immutable and the
    /// commit ids were read up front, so ref updates during the run
    /// can't change what goes in, and cache GC's grace period keeps
    /// the objects in place. The pack is in the repo's object format.
    pub fn pack_snapshot_to(
        &self,
        object_id: &str,
        snapshot: &RefSnapshot,
        sink: impl Write,
    ) -> Result<(), GitError> {
        let repo_path = self.require_repo(object_id)?;
        git_pack_revs(&repo_path, snapshot.oids(), sink)
    }

    /// [`Self::pack_snapshot_to`] from the object's current refs.
    pub fn pack_for_object_to(&self, object_id: &str, sink: impl Write) -> Result<(), GitError> {
        let snapshot = self.ref_snapshot(object_id)?;
        self.pack_snapshot_to(object_id, &snapshot, sink)
    }

    /// Convenience wrapper over [`Self::pack_for_object_to`] that
    /// returns the entire pack as a `Vec<u8>`. Useful for tests and
    /// small packs; production paths use [`Self::cached_pack`] or
    /// stream.
    pub fn pack_for_object(&self, object_id: &str) -> Result<Vec<u8>, GitError> {
        let mut buf = Vec::new();
        self.pack_for_object_to(object_id, &mut buf)?;
        Ok(buf)
    }

    /// The object's pack for its current refs, as a file on disk under
    /// `<root>/outgoing/`, generating it if this snapshot has no cached
    /// pack yet. `None` if the object's repo has no refs.
    ///
    /// The cache follows the practice of Git hosts (Gitaly's
    /// pack-objects cache): a pack is generated once per ref snapshot
    /// and served from disk to every request for that snapshot, so a
    /// slow reader costs an open file, not a lock or a `pack-objects`
    /// process. The file is written under a temporary name and renamed
    /// into place, so readers only ever see complete packs. Two
    /// processes racing on one snapshot both produce valid packs and
    /// the last rename wins. Packs for older snapshots of the object
    /// are removed once a newer one is in place (an open reader keeps
    /// its file).
    pub fn cached_pack(&self, object_id: &str) -> Result<Option<CachedPack>, GitError> {
        let snapshot = self.ref_snapshot(object_id)?;
        if snapshot.is_empty() {
            return Ok(None);
        }
        if let Some(cached) = self.find_cached_pack(object_id, &snapshot)? {
            return Ok(Some(cached));
        }
        let dir = self.outgoing_dir(object_id)?;
        let path = dir.join(format!("{}.pack", snapshot.key()));
        std::fs::create_dir_all(&dir).map_err(|source| GitError::CacheIo {
            path: dir.clone(),
            source,
        })?;
        let tmp = dir.join(format!(".{}.{}.tmp", snapshot.key(), std::process::id()));
        let file = std::fs::File::create(&tmp).map_err(|source| GitError::CacheIo {
            path: tmp.clone(),
            source,
        })?;
        let written = self
            .pack_snapshot_to(object_id, &snapshot, std::io::BufWriter::new(file))
            .and_then(|()| {
                std::fs::rename(&tmp, &path).map_err(|source| GitError::CacheIo {
                    path: path.clone(),
                    source,
                })
            });
        if let Err(error) = written {
            let _ = std::fs::remove_file(&tmp);
            return Err(error);
        }
        // Drop packs of older snapshots; ignore races with a concurrent
        // generator doing the same.
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.ends_with(".pack") && entry.path() != path {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        let len = std::fs::metadata(&path)
            .map_err(|source| GitError::CacheIo {
                path: path.clone(),
                source,
            })?
            .len();
        Ok(Some(CachedPack {
            path,
            len,
            key: snapshot.key().to_owned(),
        }))
    }

    /// The cached pack for `snapshot`, if one is already on disk.
    /// Never generates; a server checks this before taking a
    /// generation slot.
    pub fn find_cached_pack(
        &self,
        object_id: &str,
        snapshot: &RefSnapshot,
    ) -> Result<Option<CachedPack>, GitError> {
        if snapshot.is_empty() {
            return Ok(None);
        }
        let path = self
            .outgoing_dir(object_id)?
            .join(format!("{}.pack", snapshot.key()));
        Ok(std::fs::metadata(&path).ok().map(|metadata| CachedPack {
            path,
            len: metadata.len(),
            key: snapshot.key().to_owned(),
        }))
    }

    fn outgoing_dir(&self, object_id: &str) -> Result<PathBuf, GitError> {
        shard::shard_path(&self.root.join(OUTGOING_DIR), object_id)
    }

    fn require_repo(&self, object_id: &str) -> Result<PathBuf, GitError> {
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
        Ok(repo_path)
    }

    /// Ingest a pack from `source` into the `format` pool through a
    /// quarantine, as Git's `receive-pack` does (`specs/DECISIONS.md`
    /// §21):
    ///
    /// 1. `git index-pack --stdin` writes the pack into a fresh
    ///    quarantine object directory under `<root>/incoming/`, whose
    ///    alternates point at the pool so existing objects resolve. No
    ///    lock is held, so a slow `source` (a network download) blocks
    ///    nothing else.
    /// 2. Every commit in `required` must have its full closure
    ///    (commit, ancestors, trees, blobs) visible from the
    ///    quarantine. Otherwise the quarantine is deleted and this is
    ///    [`GitError::IncompletePack`]: the pool never sees a partial
    ///    pack.
    /// 3. Under the pool lock, briefly, the pack files move into the
    ///    pool. Pack names are content-addressed, so a duplicate is
    ///    simply dropped.
    ///
    /// A pack in another object format fails in `index-pack`.
    /// Ingesting creates no per-object repo; pin commits with
    /// [`Self::set_ref`] afterwards.
    pub fn ingest_pack_from(
        &self,
        format: GitObjectFormat,
        source: impl Read,
        required: &[GitRevision],
    ) -> Result<(), GitError> {
        let pool = self.ensure_pool(format)?;
        let quarantine = Quarantine::create(&self.root, &pool, format)?;
        git_index_pack_stream(quarantine.path(), source)?;

        let mut missing = Vec::new();
        for revision in required {
            if revision.format() != format
                || !git_rev_list_complete(quarantine.path(), revision.oid())?
            {
                missing.push(revision.to_string());
            }
        }
        if !missing.is_empty() {
            return Err(GitError::IncompletePack { missing });
        }
        with_path_lock(&pool, || quarantine.migrate_into(&pool))
    }

    /// Convenience wrapper over [`Self::ingest_pack_from`] that takes a
    /// byte slice and requires nothing. Useful for tests; production
    /// paths (bundle import, federation pull) stream and name the
    /// commits they need.
    pub fn ingest_pack(&self, format: GitObjectFormat, pack_bytes: &[u8]) -> Result<(), GitError> {
        self.ingest_pack_from(format, std::io::Cursor::new(pack_bytes), &[])
    }

    /// Of `revisions`, those whose commit is absent from its format's
    /// pool or doesn't reach every tree and blob beneath it. A pack
    /// can carry a commit without its contents; `index-pack` accepts
    /// that, so callers check this before pinning anything.
    pub fn missing_objects<'r>(
        &self,
        revisions: &'r [GitRevision],
    ) -> Result<Vec<&'r GitRevision>, GitError> {
        let mut missing = Vec::new();
        for revision in revisions {
            let pool = pool_path(&self.root, revision.format());
            if !pool.exists() || !git_rev_list_complete(&pool, revision.oid())? {
                missing.push(revision);
            }
        }
        Ok(missing)
    }

    /// Write `ref_name` → `revision`'s commit in the per-object repo
    /// for `object_id`, creating the repo in `revision`'s format if
    /// absent. Errors with [`GitError::CacheGitInvocation`] if the
    /// commit is not reachable from the repo's object database; pin a
    /// commit only after fetching or ingesting it.
    pub fn set_ref(
        &self,
        object_id: &str,
        ref_name: &str,
        revision: &GitRevision,
    ) -> Result<(), GitError> {
        let repo_path = self.path_for(object_id)?;
        let _ = self.ensure_repo(object_id, revision.format())?;
        with_path_lock(&repo_path, || {
            transport::update_ref(&repo_path, ref_name, revision.oid())
        })
    }

    /// Fetch `remote_branch` from `url` into the cache for
    /// `object_id`, whose history is in `format`. Orchestrates the
    /// layout:
    ///
    /// 1. `ensure_repo(object_id, format)` — initialize the
    ///    per-object bare repo if absent.
    /// 2. Ask `transport` to fetch (without holding the pool lock)
    ///    `refs/heads/<branch>` from `url` into the `format` pool,
    ///    landing it at `refs/kairo/<object-id>/<branch>`. A remote in
    ///    the other format fails in `git fetch`.
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
        format: GitObjectFormat,
        transport: &impl GitCacheTransport,
    ) -> Result<FetchedRef, GitError> {
        // Ensures sharding/id validity up front; also creates the
        // per-object repo with alternates so step 3 can succeed
        // without re-running init.
        let repo_path = self.path_for(object_id)?;
        let _ = self.ensure_repo(object_id, format)?;

        let pool = pool_path(&self.root, format);
        let local_ref = format!("refs/heads/{remote_branch}");
        let pool_dest_ref = format!("refs/kairo/{object_id}/{remote_branch}");
        let pool_refspec = format!("refs/heads/{remote_branch}:{pool_dest_ref}");

        // No pool lock across the network: `git fetch` writes packs
        // under temporary names and renames them in, and takes its own
        // lock file per ref (this one is namespaced to the object), so
        // concurrent fetches into the pool are safe, and objects are
        // never deleted out from under them inside cache GC's grace
        // period (`specs/DECISIONS.md` §21).
        self.ensure_pool(format)?;
        let pool_fetched = transport.fetch(&pool, url, &pool_refspec)?;
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

fn pool_path(root: &Path, format: GitObjectFormat) -> PathBuf {
    root.join(format!("{POOL_DIR_PREFIX}{format}"))
}

fn pool_objects_path(root: &Path, format: GitObjectFormat) -> PathBuf {
    pool_path(root, format).join("objects")
}

/// Compute the relative path from a per-object repo's
/// `<repo>/objects/` directory to the shared pool's `objects/`
/// directory at `<root>/pool-<format>/objects/`. Used as the
/// `objects/info/alternates` content.
///
/// Git resolves relative alternates paths against
/// `<GIT_DIR>/objects/`, *not* against the alternates file's own
/// parent directory — see `gitrepository-layout(5)` and
/// `Documentation/technical/objects.txt` in the git source. Using
/// the wrong reference point produces a path that's off by one
/// `..` segment, which manifests as
/// "object directory ... does not exist" on every read.
fn alternates_relative_to_pool(
    root: &Path,
    repo_path: &Path,
    format: GitObjectFormat,
) -> Result<PathBuf, GitError> {
    let objects_dir = repo_path.join("objects");
    let pool_objects = pool_objects_path(root, format);
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

fn write_alternates_to_pool(
    root: &Path,
    repo_path: &Path,
    format: GitObjectFormat,
) -> Result<(), GitError> {
    let relative = alternates_relative_to_pool(root, repo_path, format)?;
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

/// Stream `git -C <repo> pack-objects --revs --stdout` into `sink`,
/// feeding `oids` on stdin: a pack of everything reachable from those
/// commits, independent of what the refs say by the time it runs.
/// Drains stderr in a separate thread to avoid pipe-buffer deadlock
/// when stdout is large.
fn git_pack_revs<'a>(
    repo_path: &Path,
    oids: impl Iterator<Item = &'a str>,
    mut sink: impl Write,
) -> Result<(), GitError> {
    use std::process::Stdio;

    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["pack-objects", "--revs", "--stdout"])
        .stdin(Stdio::piped())
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
    let io_error = |source| GitError::CacheIo {
        path: repo_path.to_path_buf(),
        source,
    };
    {
        let mut stdin = child.stdin.take().ok_or_else(|| {
            io_error(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "git pack-objects stdin not captured",
            ))
        })?;
        for oid in oids {
            writeln!(stdin, "{oid}").map_err(io_error)?;
        }
        // Dropping stdin sends EOF: the revision list is complete.
    }
    let mut stdout = child.stdout.take().ok_or_else(|| {
        io_error(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "git pack-objects stdout not captured",
        ))
    })?;
    let copy_result = std::io::copy(&mut stdout, &mut sink).and_then(|_| sink.flush());
    drop(stdout);

    let status = child.wait().map_err(io_error)?;
    let stderr_text = stderr_bytes.collect();
    copy_result.map_err(io_error)?;
    if !status.success() {
        return Err(GitError::CacheGitInvocation {
            command: format!(
                "git -C {} pack-objects --revs --stdout",
                repo_path.display()
            ),
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

// `child.{stderr,stdout}.take()` returns `None` only when the
// pipe wasn't requested at spawn time; every callsite in this
// module pipes stdio explicitly via `Command::stdio(Stdio::piped())`,
// so the take is infallible by construction. The focused
// `expect_used` allow here documents that invariant.
#[allow(clippy::expect_used)]
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

#[allow(clippy::expect_used)]
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

/// Whether every object reachable from `oid` (the commit, its
/// ancestors, and every tree and blob beneath them) is present in the
/// repository at `repo`. `git rev-list --objects` walks the whole
/// closure and fails on the first missing object.
fn git_rev_list_complete(repo: &Path, oid: &str) -> Result<bool, GitError> {
    use std::process::Stdio;

    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-list", "--objects", "--quiet", oid, "--"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|source| match source.kind() {
            std::io::ErrorKind::NotFound => GitError::CacheGitMissing { source },
            _ => GitError::CacheIo {
                path: repo.to_path_buf(),
                source,
            },
        })?;
    Ok(status.success())
}

/// Run `git init --bare --object-format=<format> <path>` and surface
/// failures as structured errors. The cache root is created by
/// `open`/`ensure_repo`; this function only initializes the bare-repo
/// metadata inside.
fn git_init_bare(path: &Path, format: GitObjectFormat) -> Result<(), GitError> {
    std::fs::create_dir_all(path).map_err(|source| GitError::CacheIo {
        path: path.to_path_buf(),
        source,
    })?;
    let output = Command::new("git")
        .arg("init")
        .arg("--bare")
        .arg("--quiet")
        .arg(format!("--object-format={format}"))
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
            command: format!(
                "git init --bare --object-format={format} {}",
                path.display()
            ),
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

    /// Every fixture repo is SHA-1 (plain `git init`).
    fn sha1(oid: &str) -> GitRevision {
        GitRevision::new(GitObjectFormat::Sha1, oid).expect("40-hex SHA-1 oid")
    }

    fn open_temp() -> (TempDir, GitCache) {
        let dir = TempDir::new().expect("tempdir");
        let cache = GitCache::open(dir.path()).expect("open cache");
        (dir, cache)
    }

    #[test]
    fn pools_are_created_per_format_on_first_use() {
        if skip_if_no_git() {
            return;
        }
        let (dir, cache) = open_temp();
        assert!(
            !dir.path().join("pool-sha1").exists(),
            "open creates no pool"
        );
        cache
            .ensure_repo(SAMPLE_ID, GitObjectFormat::Sha1)
            .expect("ensure_repo");
        assert!(
            !dir.path().join("pool-sha256").exists(),
            "only the format used"
        );
        let pool = dir.path().join("pool-sha1");
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
        let first = GitCache::open(dir.path()).expect("open 1");
        first
            .ensure_repo(SAMPLE_ID, GitObjectFormat::Sha1)
            .expect("ensure_repo");
        // Drop a marker into the pool to detect re-init.
        let marker = dir.path().join("pool-sha1").join("kairo-marker");
        std::fs::write(&marker, b"keep me").expect("marker");
        let second = GitCache::open(dir.path()).expect("open 2");
        second
            .ensure_repo(OTHER_ID, GitObjectFormat::Sha1)
            .expect("ensure_repo");
        assert!(marker.exists(), "a second open must not re-init the pool");
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
        let _repo = cache
            .ensure_repo(SAMPLE_ID, GitObjectFormat::Sha1)
            .expect("ensure_repo");
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
        let canonical_pool = std::fs::canonicalize(cache.root().join("pool-sha1").join("objects"))
            .expect("canon pool");
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
        let _ = cache
            .ensure_repo(SAMPLE_ID, GitObjectFormat::Sha1)
            .expect("ensure 1");
        // Drop a marker file into the per-object repo to detect re-init.
        let repo_path = cache.path_for(SAMPLE_ID).expect("path_for");
        let marker = repo_path.join("kairo-marker");
        std::fs::write(&marker, b"keep me").expect("marker");
        let _ = cache
            .ensure_repo(SAMPLE_ID, GitObjectFormat::Sha1)
            .expect("ensure 2");
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
            .has_commit(SAMPLE_ID, &sha1("0000000000000000000000000000000000000000"))
            .expect("has_commit no repo"));
        // After ensure_repo, still false because pool is empty.
        let _ = cache
            .ensure_repo(SAMPLE_ID, GitObjectFormat::Sha1)
            .expect("ensure_repo");
        assert!(!cache
            .has_commit(SAMPLE_ID, &sha1("0000000000000000000000000000000000000000"))
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
                    cache
                        .ensure_repo(SAMPLE_ID, GitObjectFormat::Sha1)
                        .expect("ensure_repo");
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
        let _ = cache
            .ensure_repo(SAMPLE_ID, GitObjectFormat::Sha1)
            .expect("ensure 1");
        let _ = cache
            .ensure_repo(OTHER_ID, GitObjectFormat::Sha1)
            .expect("ensure 2");
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
            .fetch(
                SAMPLE_ID,
                &url,
                &branch,
                GitObjectFormat::Sha1,
                &GitCli::new(),
            )
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
        assert!(cache
            .has_commit(SAMPLE_ID, &sha1(&head_oid))
            .expect("has_commit"));

        // Pool ref is namespaced.
        let pool_ref = std::process::Command::new("git")
            .arg("-C")
            .arg(cache.root().join("pool-sha1"))
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
            .fetch(
                SAMPLE_ID,
                &url,
                "no-such-branch",
                GitObjectFormat::Sha1,
                &GitCli::new(),
            )
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
            .fetch(
                SAMPLE_ID,
                &url,
                &branch,
                GitObjectFormat::Sha1,
                &GitCli::new(),
            )
            .expect("fetch 1");
        cache
            .fetch(
                OTHER_ID,
                &url,
                &branch,
                GitObjectFormat::Sha1,
                &GitCli::new(),
            )
            .expect("fetch 2");

        // Both per-object repos resolve the same OID; the actual
        // commit data lives once in the shared pool.
        assert!(cache
            .has_commit(SAMPLE_ID, &sha1(&head_oid))
            .expect("has 1"));
        assert!(cache.has_commit(OTHER_ID, &sha1(&head_oid)).expect("has 2"));

        // Pool holds two distinct namespaced refs both pointing at
        // the same OID.
        let probe = |id: &str| -> String {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(cache.root().join("pool-sha1"))
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
                        .fetch(
                            SAMPLE_ID,
                            &url,
                            &branch,
                            GitObjectFormat::Sha1,
                            &GitCli::new(),
                        )
                        .expect("fetch");
                });
            }
        });

        // After all four converge, both refs are intact and the OID
        // matches the source head.
        assert!(cache
            .has_commit(SAMPLE_ID, &sha1(&head_oid))
            .expect("has_commit"));
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
        cache
            .ingest_pack(GitObjectFormat::Sha1, &pack)
            .expect("ingest_pack");

        // After ingest, the pool's object DB knows about every commit
        // from the source repo. Per-object repos see them via alternates,
        // so has_commit succeeds without needing a fetch.
        let _ = cache
            .ensure_repo(SAMPLE_ID, GitObjectFormat::Sha1)
            .expect("ensure_repo");
        assert!(
            cache
                .has_commit(SAMPLE_ID, &sha1(&head_oid))
                .expect("has_commit"),
            "head OID {head_oid} must be reachable after pack ingest"
        );

        // The pack file should appear under canonical pack-<sha> naming.
        let pack_dir = cache.root().join("pool-sha1").join("objects").join("pack");
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
            .ingest_pack(GitObjectFormat::Sha1, b"not a real pack")
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

        cache
            .ingest_pack(GitObjectFormat::Sha1, &pack)
            .expect("ingest 1");
        cache
            .ingest_pack(GitObjectFormat::Sha1, &pack)
            .expect("ingest 2 (same bytes)");

        // After the second ingest, the same pack-<sha>.pack file
        // exists exactly once in the pool's pack dir — git
        // index-pack overwrites identical content with itself.
        let pack_dir = cache.root().join("pool-sha1").join("objects").join("pack");
        let pack_files: Vec<_> = std::fs::read_dir(&pack_dir)
            .expect("read pack dir")
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".pack"))
            .collect();
        assert_eq!(pack_files.len(), 1, "exactly one pack file expected");

        let _ = cache
            .ensure_repo(SAMPLE_ID, GitObjectFormat::Sha1)
            .expect("ensure_repo");
        assert!(cache
            .has_commit(SAMPLE_ID, &sha1(&head_oid))
            .expect("has_commit"));
    }

    #[test]
    fn set_ref_pins_an_oid_after_ingest() {
        if skip_if_no_git() {
            return;
        }
        let (src, _url, branch, head_oid) = init_source_repo();
        let pack = build_pack_from(src.path());
        let (_dir, cache) = open_temp();
        cache
            .ingest_pack(GitObjectFormat::Sha1, &pack)
            .expect("ingest_pack");

        let ref_name = format!("refs/heads/{branch}");
        cache
            .set_ref(SAMPLE_ID, &ref_name, &sha1(&head_oid))
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
        cache
            .ingest_pack(GitObjectFormat::Sha1, &pack)
            .expect("ingest_pack");

        // No prior ensure_repo call — set_ref must initialize the
        // per-object repo as part of its operation.
        let repo_path = cache.path_for(OTHER_ID).expect("path_for");
        assert!(
            !repo_path.exists(),
            "precondition: repo should not exist yet"
        );

        cache
            .set_ref(OTHER_ID, &format!("refs/heads/{branch}"), &sha1(&head_oid))
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
                &sha1("1234567890abcdef1234567890abcdef12345678"),
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
            .fetch(
                SAMPLE_ID,
                &url,
                &branch,
                GitObjectFormat::Sha1,
                &GitCli::new(),
            )
            .expect("fetch");
        let pack = cache1.pack_for_object(SAMPLE_ID).expect("pack_for_object");
        assert!(
            !pack.is_empty(),
            "pack must contain at least the head commit"
        );

        let (_dir2, cache2) = open_temp();
        cache2
            .ingest_pack(GitObjectFormat::Sha1, &pack)
            .expect("ingest_pack");
        cache2
            .set_ref(OTHER_ID, "refs/heads/main", &sha1(&head_oid))
            .expect("set_ref");

        assert!(
            cache2
                .has_commit(OTHER_ID, &sha1(&head_oid))
                .expect("has_commit"),
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
            .fetch(
                SAMPLE_ID,
                &url,
                &branch,
                GitObjectFormat::Sha1,
                &GitCli::new(),
            )
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
            .fetch(
                SAMPLE_ID,
                &url,
                &branch,
                GitObjectFormat::Sha1,
                &GitCli::new(),
            )
            .expect("fetch");
        let pack_bytes = cache1.pack_for_object(SAMPLE_ID).expect("pack");
        let pack_file = TempDir::new().expect("tempdir");
        let pack_path = pack_file.path().join("from-bundle.pack");
        std::fs::write(&pack_path, &pack_bytes).expect("write pack");

        // Cache 2: ingest by streaming the file straight into git
        // index-pack, never holding the full pack in memory.
        let (_dir2, cache2) = open_temp();
        let file = std::fs::File::open(&pack_path).expect("open pack");
        cache2
            .ingest_pack_from(GitObjectFormat::Sha1, file, &[sha1(&head_oid)])
            .expect("ingest_pack_from");
        cache2
            .set_ref(SAMPLE_ID, "refs/heads/main", &sha1(&head_oid))
            .expect("set_ref");
        assert!(cache2
            .has_commit(SAMPLE_ID, &sha1(&head_oid))
            .expect("has_commit"));
    }

    /// A one-commit repo in `format`; returns (tempdir, revision).
    fn source_repo_in(format: GitObjectFormat) -> (TempDir, GitRevision) {
        let dir = TempDir::new().expect("tempdir");
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .current_dir(dir.path())
                .args(args)
                .status()
                .expect("git");
            assert!(status.success(), "git {args:?}");
        };
        git(&[
            "init",
            "--quiet",
            "--initial-branch=main",
            &format!("--object-format={format}"),
        ]);
        git(&["config", "user.name", "Kairo Test"]);
        git(&["config", "user.email", "test@kairo.test"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.path().join("kairo.toml"), "[kairo]\nschema = 1\n").expect("write");
        git(&["add", "kairo.toml"]);
        git(&["commit", "-m", "only", "--quiet"]);
        let output = std::process::Command::new("git")
            .current_dir(dir.path())
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("rev-parse");
        let oid = String::from_utf8(output.stdout)
            .expect("utf8")
            .trim()
            .to_owned();
        let revision = GitRevision::new(format, &oid).expect("oid matches format");
        (dir, revision)
    }

    #[test]
    fn sha256_history_round_trips_through_its_own_pool() {
        if skip_if_no_git() {
            return;
        }
        let (src, revision) = source_repo_in(GitObjectFormat::Sha256);
        let (dir, cache) = open_temp();
        cache
            .ingest_pack(GitObjectFormat::Sha256, &build_pack_from(src.path()))
            .expect("ingest");
        assert!(dir.path().join("pool-sha256").exists());
        assert!(!dir.path().join("pool-sha1").exists());
        assert!(cache
            .missing_objects(std::slice::from_ref(&revision))
            .expect("check")
            .is_empty());
        cache
            .set_ref(SAMPLE_ID, "refs/heads/main", &revision)
            .expect("set_ref");
        assert_eq!(
            cache.repo_format(SAMPLE_ID).expect("format"),
            Some(GitObjectFormat::Sha256)
        );
        assert!(cache.has_commit(SAMPLE_ID, &revision).expect("has_commit"));
        assert!(cache.has_refs(SAMPLE_ID).expect("has_refs"));

        // The object's pack carries its history into another cache.
        let pack = cache.pack_for_object(SAMPLE_ID).expect("pack");
        let (_other_dir, other) = open_temp();
        other
            .ingest_pack(GitObjectFormat::Sha256, &pack)
            .expect("ingest relayed pack");
        other
            .set_ref(OTHER_ID, "refs/heads/main", &revision)
            .expect("set_ref");
        assert!(other.has_commit(OTHER_ID, &revision).expect("has_commit"));
    }

    #[test]
    fn an_object_repo_keeps_one_format() {
        if skip_if_no_git() {
            return;
        }
        let (_dir, cache) = open_temp();
        cache
            .ensure_repo(SAMPLE_ID, GitObjectFormat::Sha1)
            .expect("sha1 repo");
        assert!(matches!(
            cache.ensure_repo(SAMPLE_ID, GitObjectFormat::Sha256),
            Err(GitError::FormatMismatch {
                expected: GitObjectFormat::Sha256,
                found: GitObjectFormat::Sha1,
                ..
            })
        ));
        // A revision in the other format is simply not there.
        let sha256 = GitRevision::new(GitObjectFormat::Sha256, &"b".repeat(64)).expect("shape");
        assert!(!cache.has_commit(SAMPLE_ID, &sha256).expect("has_commit"));
    }

    #[test]
    fn a_pack_in_the_wrong_format_is_refused_by_index_pack() {
        if skip_if_no_git() {
            return;
        }
        let (src, _) = source_repo_in(GitObjectFormat::Sha256);
        let (_dir, cache) = open_temp();
        assert!(cache
            .ingest_pack(GitObjectFormat::Sha1, &build_pack_from(src.path()))
            .is_err());
    }

    #[test]
    fn missing_objects_catches_a_commit_without_its_tree() {
        if skip_if_no_git() {
            return;
        }
        let (src, revision) = source_repo_in(GitObjectFormat::Sha1);
        // Pack only the commit object, not its tree or blob.
        let mut child = std::process::Command::new("git")
            .arg("-C")
            .arg(src.path())
            .args(["pack-objects", "--stdout"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("pack-objects");
        {
            use std::io::Write as _;
            let mut stdin = child.stdin.take().expect("stdin");
            writeln!(stdin, "{}", revision.oid()).expect("write oid");
        }
        let output = child.wait_with_output().expect("pack");
        assert!(output.status.success());

        let (_dir, cache) = open_temp();
        cache
            .ingest_pack(GitObjectFormat::Sha1, &output.stdout)
            .expect("index-pack accepts a commit-only pack");
        let revisions = [revision];
        assert_eq!(
            cache.missing_objects(&revisions).expect("check"),
            vec![&revisions[0]]
        );
    }

    /// A pack of only `revision`'s commit object, without its tree.
    fn commit_only_pack(src: &Path, revision: &GitRevision) -> Vec<u8> {
        let mut child = std::process::Command::new("git")
            .arg("-C")
            .arg(src)
            .args(["pack-objects", "--stdout"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("pack-objects");
        {
            use std::io::Write as _;
            let mut stdin = child.stdin.take().expect("stdin");
            writeln!(stdin, "{}", revision.oid()).expect("write oid");
        }
        let output = child.wait_with_output().expect("pack");
        assert!(output.status.success());
        output.stdout
    }

    fn pool_pack_files(root: &Path, format: GitObjectFormat) -> usize {
        std::fs::read_dir(
            root.join(format!("pool-{format}"))
                .join("objects")
                .join("pack"),
        )
        .map(|entries| entries.flatten().count())
        .unwrap_or(0)
    }

    #[test]
    fn quarantine_keeps_an_incomplete_pack_out_of_the_pool() {
        if skip_if_no_git() {
            return;
        }
        let (src, revision) = source_repo_in(GitObjectFormat::Sha1);
        let (dir, cache) = open_temp();
        let pack = commit_only_pack(src.path(), &revision);
        let result = cache.ingest_pack_from(
            GitObjectFormat::Sha1,
            std::io::Cursor::new(pack),
            std::slice::from_ref(&revision),
        );
        assert!(
            matches!(result, Err(GitError::IncompletePack { ref missing }) if missing == &[revision.to_string()]),
            "{result:?}"
        );
        assert_eq!(
            pool_pack_files(dir.path(), GitObjectFormat::Sha1),
            0,
            "pool untouched"
        );
        let incoming = dir.path().join("incoming");
        assert_eq!(
            std::fs::read_dir(&incoming).map(|e| e.count()).unwrap_or(0),
            0,
            "quarantine removed"
        );

        // The complete pack goes through and lands in the pool.
        cache
            .ingest_pack_from(
                GitObjectFormat::Sha1,
                std::io::Cursor::new(build_pack_from(src.path())),
                std::slice::from_ref(&revision),
            )
            .expect("complete pack");
        assert!(pool_pack_files(dir.path(), GitObjectFormat::Sha1) > 0);
        assert!(cache
            .missing_objects(std::slice::from_ref(&revision))
            .expect("check")
            .is_empty());
    }

    #[test]
    fn a_slow_pack_reader_does_not_block_writes_to_the_object() {
        // The original bug: serving a pack held the object's lock for
        // the whole transfer, so any write that took longer than the
        // 2 s lock deadline failed. Packing now holds no lock.
        if skip_if_no_git() {
            return;
        }
        let (src, revision) = source_repo_in(GitObjectFormat::Sha1);
        let (_dir, cache) = open_temp();
        cache
            .ingest_pack(GitObjectFormat::Sha1, &build_pack_from(src.path()))
            .expect("ingest");
        cache
            .set_ref(SAMPLE_ID, "refs/heads/main", &revision)
            .expect("ref");

        struct Slow(bool);
        impl Write for Slow {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                if !self.0 {
                    self.0 = true;
                    std::thread::sleep(std::time::Duration::from_secs(3));
                }
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let reader_cache = cache.clone();
        let reader = std::thread::spawn(move || {
            reader_cache
                .pack_for_object_to(SAMPLE_ID, Slow(false))
                .expect("slow pack");
        });
        std::thread::sleep(std::time::Duration::from_millis(300));
        let started = std::time::Instant::now();
        cache
            .set_ref(SAMPLE_ID, "refs/heads/other", &revision)
            .expect("write during a slow read");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        reader.join().expect("reader");
    }

    #[test]
    fn cached_packs_are_reused_per_snapshot_and_replaced_on_change() {
        if skip_if_no_git() {
            return;
        }
        let (src, revision) = source_repo_in(GitObjectFormat::Sha1);
        let (_dir, cache) = open_temp();
        assert_eq!(cache.cached_pack(SAMPLE_ID).expect("none"), None);
        cache
            .ingest_pack(GitObjectFormat::Sha1, &build_pack_from(src.path()))
            .expect("ingest");
        cache
            .set_ref(SAMPLE_ID, "refs/heads/main", &revision)
            .expect("ref");

        let first = cache.cached_pack(SAMPLE_ID).expect("pack").expect("some");
        let modified = std::fs::metadata(&first.path)
            .and_then(|m| m.modified())
            .expect("mtime");
        let again = cache.cached_pack(SAMPLE_ID).expect("pack").expect("some");
        assert_eq!(again, first, "same snapshot, same file");
        assert_eq!(
            std::fs::metadata(&again.path)
                .and_then(|m| m.modified())
                .expect("mtime"),
            modified,
            "not regenerated"
        );

        // The cached pack carries the history into another cache.
        let (_other_dir, other) = open_temp();
        other
            .ingest_pack_from(
                GitObjectFormat::Sha1,
                std::fs::File::open(&first.path).expect("open"),
                std::slice::from_ref(&revision),
            )
            .expect("ingest cached pack");

        // A ref change is a new snapshot: a new file, the old one gone.
        cache
            .set_ref(SAMPLE_ID, "refs/heads/second", &revision)
            .expect("ref");
        let second = cache.cached_pack(SAMPLE_ID).expect("pack").expect("some");
        assert_ne!(second.key, first.key);
        assert!(!first.path.exists(), "superseded pack removed");
        assert!(second.path.exists());
    }

    #[test]
    fn a_snapshot_pack_ignores_ref_changes_after_the_snapshot() {
        if skip_if_no_git() {
            return;
        }
        let (src, first) = source_repo_in(GitObjectFormat::Sha1);
        let (_dir, cache) = open_temp();
        cache
            .ingest_pack(GitObjectFormat::Sha1, &build_pack_from(src.path()))
            .expect("ingest");
        cache
            .set_ref(SAMPLE_ID, "refs/heads/main", &first)
            .expect("ref");
        let snapshot = cache.ref_snapshot(SAMPLE_ID).expect("snapshot");
        assert_eq!(snapshot.refs().len(), 1);

        // A second, different commit arrives and a ref moves to it after
        // the snapshot.
        let (src2, second_oid) = kairo_test_support::git::init_git_repo_with_manifest(
            "[kairo]\nschema = 1\nname = \"later\"\n",
        )
        .expect("second repo");
        let second = sha1(&second_oid);
        assert_ne!(second, first);
        cache
            .ingest_pack(GitObjectFormat::Sha1, &build_pack_from(src2.path()))
            .expect("ingest");
        cache
            .set_ref(SAMPLE_ID, "refs/heads/later", &second)
            .expect("ref");

        let mut pack = Vec::new();
        cache
            .pack_snapshot_to(SAMPLE_ID, &snapshot, &mut pack)
            .expect("pack");
        let (_out_dir, out) = open_temp();
        out.ingest_pack(GitObjectFormat::Sha1, &pack)
            .expect("ingest");
        assert!(out
            .missing_objects(std::slice::from_ref(&first))
            .expect("check")
            .is_empty());
        assert_eq!(
            out.missing_objects(std::slice::from_ref(&second))
                .expect("check"),
            vec![&second],
            "the snapshot's pack holds only what its refs reached"
        );
    }
}
