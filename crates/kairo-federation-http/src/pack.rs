//! Serving an object's Git pack (`specs/PHASE_2_FEDERATION.md` slice
//! 4, `specs/DECISIONS.md` §21).
//!
//! Following Git hosts (Gitaly's pack-objects cache and concurrency
//! limits), a pack is generated once per ref snapshot into a file under
//! the cache's `outgoing/` directory and streamed from that file:
//!
//! - **No lock across the network.** Generation snapshots the refs and
//!   packs from those commit ids without holding the object's lock, and
//!   streaming reads an immutable file. A slow or stalled peer costs one
//!   open file and one task, never a lock other writers wait on.
//! - **One generation per object at a time.** Concurrent requests for
//!   the same object wait for the first to finish generating and then
//!   share its file.
//! - **A cap on concurrent generation** across all objects. Over the cap,
//!   a request waits briefly for a slot, then gets `503 fed_busy` with
//!   `Retry-After`.
//! - **A server-side transfer deadline**, sized to the pack at a minimum
//!   throughput, so a peer that stops reading is disconnected.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes};
use http_body::Frame;
use kairo_git::{CachedPack, GitCache};
use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, Semaphore};

/// Chunks in flight between the file reader and the socket.
const CHANNEL_DEPTH: usize = 8;
const CHUNK_BYTES: usize = 64 * 1024;

/// How long a request waits for a generation slot before `503`.
const SLOT_WAIT: Duration = Duration::from_secs(10);
/// `Retry-After` on a `503 fed_busy`, in seconds.
pub(crate) const RETRY_AFTER_SECS: u64 = 5;

/// Transfer deadline: this much time plus the pack at this rate.
const TRANSFER_BASE: Duration = Duration::from_secs(30);
const MIN_TRANSFER_BYTES_PER_SEC: u64 = 64 * 1024;

type Chunk = Result<Bytes, io::Error>;

/// Whether the managed cache under `git_root` has a per-object repo
/// for `object_id` with at least one ref, so its pack is non-empty.
/// Read through gix without opening the cache, so this process never
/// writes to the store or needs the `git` binary to answer.
pub(crate) fn has_pack(git_root: &Path, object_id: &str) -> bool {
    let Ok(path) = kairo_git::object_repo_path(git_root, object_id) else {
        return false;
    };
    path.exists() && kairo_git::open(&path).is_ok_and(|repo| repo.has_refs().unwrap_or(false))
}

/// Generation slots and per-object single-flight, shared by every
/// request a node serves.
#[derive(Debug)]
pub(crate) struct PackService {
    slots: Arc<Semaphore>,
    slot_wait: Duration,
    generating: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

/// Why a pack couldn't be served.
#[derive(Debug)]
pub(crate) enum PackError {
    /// No generation slot came free in time.
    Busy,
    /// The repo has no refs (nothing to pack).
    Empty,
    Git(String),
}

impl PackService {
    pub(crate) fn new(max_concurrent: usize) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(max_concurrent.max(1))),
            slot_wait: SLOT_WAIT,
            generating: Mutex::new(HashMap::new()),
        }
    }

    /// Default cap: half the available CPUs, at least one. Each
    /// `pack-objects` run is CPU-heavy (delta compression).
    pub(crate) fn default_concurrency() -> usize {
        std::thread::available_parallelism()
            .map(|n| (n.get() / 2).max(1))
            .unwrap_or(1)
    }

    fn object_lock(&self, object_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut generating = self
            .generating
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(generating.entry(object_id.to_owned()).or_default())
    }

    /// The cached pack for the object's current refs, generating it if
    /// needed. Only generation takes a slot; a cache hit doesn't.
    pub(crate) async fn pack(
        &self,
        git_root: &Path,
        object_id: &str,
    ) -> Result<CachedPack, PackError> {
        let lock = self.object_lock(object_id);
        let _single_flight = lock.lock().await;

        let cache = GitCache::open(git_root).map_err(|e| PackError::Git(e.to_string()))?;
        let lookup_cache = cache.clone();
        let lookup_object = object_id.to_owned();
        let found = tokio::task::spawn_blocking(move || {
            let snapshot = lookup_cache.ref_snapshot(&lookup_object)?;
            lookup_cache.find_cached_pack(&lookup_object, &snapshot)
        })
        .await
        .map_err(|e| PackError::Git(e.to_string()))?
        .map_err(|e| PackError::Git(e.to_string()))?;
        if let Some(cached) = found {
            return Ok(cached);
        }

        let _slot = tokio::time::timeout(self.slot_wait, Arc::clone(&self.slots).acquire_owned())
            .await
            .map_err(|_| PackError::Busy)?
            .map_err(|e| PackError::Git(e.to_string()))?;
        let object = object_id.to_owned();
        tokio::task::spawn_blocking(move || cache.cached_pack(&object))
            .await
            .map_err(|e| PackError::Git(e.to_string()))?
            .map_err(|e| PackError::Git(e.to_string()))?
            .ok_or(PackError::Empty)
    }
}

/// The deadline for sending `len` bytes.
fn transfer_deadline(len: u64) -> Duration {
    TRANSFER_BASE + Duration::from_secs(len / MIN_TRANSFER_BYTES_PER_SEC)
}

/// A response body streaming `pack`'s file. A task reads the file into
/// a bounded channel; if the peer hasn't taken the whole pack by the
/// transfer deadline, the body ends in an error and the connection
/// drops.
pub(crate) fn file_body(pack: &CachedPack) -> Body {
    let (sender, receiver) = mpsc::channel::<Chunk>(CHANNEL_DEPTH);
    let path = pack.path.clone();
    let deadline = tokio::time::Instant::now() + transfer_deadline(pack.len);
    tokio::spawn(async move {
        let mut file = match tokio::fs::File::open(&path).await {
            Ok(file) => file,
            Err(error) => {
                let _ = sender.send(Err(error)).await;
                return;
            }
        };
        let mut buf = vec![0u8; CHUNK_BYTES];
        loop {
            let read = match file.read(&mut buf).await {
                Ok(0) => return,
                Ok(read) => read,
                Err(error) => {
                    let _ = sender.send(Err(error)).await;
                    return;
                }
            };
            let chunk = Ok(Bytes::copy_from_slice(&buf[..read]));
            match tokio::time::timeout_at(deadline, sender.send(chunk)).await {
                Ok(Ok(())) => {}
                // The peer hung up.
                Ok(Err(_)) => return,
                Err(_) => {
                    tracing::warn!(
                        pack = %path.display(),
                        "pack transfer exceeded its deadline; dropping the connection"
                    );
                    let _ = sender.try_send(Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "pack transfer deadline exceeded",
                    )));
                    return;
                }
            }
        }
    });
    Body::new(ChannelBody { receiver })
}

/// Body end: yields chunks until the reader side drops.
struct ChannelBody {
    receiver: mpsc::Receiver<Chunk>,
}

impl http_body::Body for ChannelBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.receiver
            .poll_recv(cx)
            .map(|chunk| chunk.map(|result| result.map(Frame::data)))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use kairo_git::{GitObjectFormat, GitRevision};
    use kairo_test_support::git::{build_pack_from, init_git_repo_with_manifest, skip_if_no_git};

    const OBJECT: &str = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";

    /// A cache whose object repo has one ref.
    fn cache_with_object() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let (repo, oid) = init_git_repo_with_manifest("[kairo]\nschema = 1\n").expect("repo");
        let cache = GitCache::open(dir.path()).expect("cache");
        cache
            .ingest_pack(GitObjectFormat::Sha1, &build_pack_from(repo.path()))
            .expect("ingest");
        let revision = GitRevision::new(GitObjectFormat::Sha1, &oid).expect("oid");
        cache
            .set_ref(OBJECT, "refs/heads/main", &revision)
            .expect("ref");
        dir
    }

    #[tokio::test]
    async fn generation_over_the_cap_is_busy_but_cache_hits_are_not() {
        if skip_if_no_git() {
            return;
        }
        let dir = cache_with_object();
        let mut service = PackService::new(1);
        service.slot_wait = Duration::from_millis(100);

        // Every slot taken: generating a new pack is refused.
        let held = Arc::clone(&service.slots)
            .acquire_owned()
            .await
            .expect("slot");
        assert!(matches!(
            service.pack(dir.path(), OBJECT).await,
            Err(PackError::Busy)
        ));
        drop(held);

        // With the slot free it generates; with the cache warm a
        // request needs no slot at all.
        let first = service.pack(dir.path(), OBJECT).await.expect("generate");
        let _held = Arc::clone(&service.slots)
            .acquire_owned()
            .await
            .expect("slot");
        let again = service.pack(dir.path(), OBJECT).await.expect("cache hit");
        assert_eq!(again, first);
    }

    #[tokio::test]
    async fn concurrent_requests_share_one_generation() {
        if skip_if_no_git() {
            return;
        }
        let dir = cache_with_object();
        let service = Arc::new(PackService::new(4));
        let mut tasks = Vec::new();
        for _ in 0..4 {
            let service = Arc::clone(&service);
            let root = dir.path().to_path_buf();
            tasks.push(tokio::spawn(
                async move { service.pack(&root, OBJECT).await },
            ));
        }
        let mut packs = Vec::new();
        for task in tasks {
            packs.push(task.await.expect("join").expect("pack"));
        }
        assert!(packs.windows(2).all(|pair| pair[0] == pair[1]));
        let outgoing = std::fs::read_dir(packs[0].path.parent().expect("dir"))
            .expect("dir")
            .flatten()
            .count();
        assert_eq!(outgoing, 1, "one pack file, no leftover temporaries");
    }

    #[test]
    fn deadline_grows_with_the_pack() {
        assert_eq!(transfer_deadline(0), TRANSFER_BASE);
        assert_eq!(
            transfer_deadline(64 * 1024 * 60),
            TRANSFER_BASE + Duration::from_secs(60)
        );
    }
}
