//! Running `git upload-pack` for a peer (`specs/DECISIONS.md` §21 and
//! §22).
//!
//! Following Git hosts (Gitaly's concurrency limits), a semaphore caps
//! concurrent `upload-pack` fetches across all objects: each runs
//! `pack-objects`, which is CPU-heavy. Over the cap, a request waits
//! briefly for a slot, then gets `503 fed_busy` with `Retry-After`.
//!
//! Output streams from the process through a bounded channel, and no
//! lock is held: `upload-pack` reads immutable objects. A peer that
//! stops reading for [`STALL_TIMEOUT`] is disconnected and the process
//! killed, so a stalled peer costs one slot for that long at most.

use std::io;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes};
use http_body::Frame;
use kairo_git::GitCache;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};

/// Chunks in flight between the process and the socket.
const CHANNEL_DEPTH: usize = 8;
const CHUNK_BYTES: usize = 64 * 1024;

/// How long a request waits for a slot before `503`.
const SLOT_WAIT: Duration = Duration::from_secs(10);
/// `Retry-After` on a `503 fed_busy`, in seconds.
pub(crate) const RETRY_AFTER_SECS: u64 = 5;

/// Longest a peer may leave a chunk unread before it is disconnected.
pub(crate) const STALL_TIMEOUT: Duration = Duration::from_secs(30);

type Chunk = Result<Bytes, io::Error>;

/// Whether the managed cache under `git_root` has a per-object repo
/// for `object_id` with at least one ref. Read through gix without
/// opening the cache, so this process never writes to the store or
/// needs the `git` binary to answer.
pub(crate) fn has_git(git_root: &Path, object_id: &str) -> bool {
    let Ok(path) = kairo_git::object_repo_path(git_root, object_id) else {
        return false;
    };
    path.exists() && kairo_git::open(&path).is_ok_and(|repo| repo.has_refs().unwrap_or(false))
}

/// Why `upload-pack` couldn't be run.
#[derive(Debug)]
pub(crate) enum UploadError {
    /// No slot came free in time.
    Busy,
    Git(String),
}

/// Slots for concurrent `upload-pack` fetches, shared by every request
/// a node serves.
#[derive(Debug)]
pub(crate) struct UploadService {
    slots: Arc<Semaphore>,
    slot_wait: Duration,
}

impl UploadService {
    pub(crate) fn new(max_concurrent: usize) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(max_concurrent.max(1))),
            slot_wait: SLOT_WAIT,
        }
    }

    /// Default cap: half the available CPUs, at least one.
    pub(crate) fn default_concurrency() -> usize {
        std::thread::available_parallelism()
            .map(|n| (n.get() / 2).max(1))
            .unwrap_or(1)
    }

    /// The capability advertisement for the object's repo, as
    /// `upload-pack` prints it. Cheap; takes no slot.
    pub(crate) async fn advertise(
        &self,
        git_root: &Path,
        object_id: &str,
    ) -> Result<Vec<u8>, UploadError> {
        let command = command(git_root, object_id, true)?;
        let output = tokio::process::Command::from(command)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| UploadError::Git(e.to_string()))?;
        if !output.status.success() {
            return Err(UploadError::Git(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        Ok(output.stdout)
    }

    /// Run one stateless `upload-pack` request (already filtered) and
    /// stream its response. Holds a slot until the response ends.
    pub(crate) async fn fetch(
        &self,
        git_root: &Path,
        object_id: &str,
        request: Vec<u8>,
    ) -> Result<Body, UploadError> {
        let permit = tokio::time::timeout(self.slot_wait, Arc::clone(&self.slots).acquire_owned())
            .await
            .map_err(|_| UploadError::Busy)?
            .map_err(|e| UploadError::Git(e.to_string()))?;
        let command = command(git_root, object_id, false)?;
        let mut child = tokio::process::Command::from(command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| UploadError::Git(e.to_string()))?;
        let (Some(mut stdin), Some(stdout), Some(mut stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(UploadError::Git(
                "upload-pack pipes not captured".to_owned(),
            ));
        };
        // `upload-pack --stateless-rpc` reads the whole request before
        // answering, so writing it first can't deadlock.
        tokio::spawn(async move {
            let _ = stdin.write_all(&request).await;
        });
        let object = object_id.to_owned();
        tokio::spawn(async move {
            let mut text = Vec::new();
            let _ = stderr.read_to_end(&mut text).await;
            if let Ok(status) = child.wait().await {
                if !status.success() {
                    tracing::warn!(
                        %object,
                        stderr = %String::from_utf8_lossy(&text),
                        "upload-pack failed"
                    );
                }
            }
        });
        Ok(stream_body(stdout, permit))
    }
}

fn command(
    git_root: &Path,
    object_id: &str,
    advertise: bool,
) -> Result<std::process::Command, UploadError> {
    GitCache::open(git_root)
        .and_then(|cache| cache.upload_pack(object_id, advertise))
        .map_err(|e| UploadError::Git(e.to_string()))
}

/// A response body fed from `source` by a task. If the peer leaves a
/// chunk unread for [`STALL_TIMEOUT`], the body ends in an error, the
/// connection drops, and dropping `source` ends the process.
fn stream_body(
    mut source: impl tokio::io::AsyncRead + Unpin + Send + 'static,
    permit: OwnedSemaphorePermit,
) -> Body {
    let (sender, receiver) = mpsc::channel::<Chunk>(CHANNEL_DEPTH);
    tokio::spawn(async move {
        let _permit = permit;
        let mut buf = vec![0u8; CHUNK_BYTES];
        loop {
            let read = match source.read(&mut buf).await {
                Ok(0) => return,
                Ok(read) => read,
                Err(error) => {
                    let _ = sender.send(Err(error)).await;
                    return;
                }
            };
            let chunk = Ok(Bytes::copy_from_slice(&buf[..read]));
            match tokio::time::timeout(STALL_TIMEOUT, sender.send(chunk)).await {
                Ok(Ok(())) => {}
                // The peer hung up.
                Ok(Err(_)) => return,
                Err(_) => {
                    tracing::warn!("a peer stopped reading a Git response; dropping it");
                    let _ = sender
                        .try_send(Err(io::Error::new(io::ErrorKind::TimedOut, "peer stalled")));
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
    use crate::git_protocol::{parse_request, write_pkt};
    use http_body_util::BodyExt;
    use kairo_git::{GitObjectFormat, GitRevision};
    use kairo_test_support::git::{build_pack_from, init_git_repo_with_manifest, skip_if_no_git};

    const OBJECT: &str = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";

    /// A cache whose object repo has one ref, and that ref's commit.
    fn cache_with_object() -> (tempfile::TempDir, String) {
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
        (dir, oid)
    }

    fn fetch_request(oid: &str) -> Vec<u8> {
        let mut out = Vec::new();
        write_pkt(&mut out, b"command=fetch\n");
        out.extend_from_slice(b"0001");
        write_pkt(&mut out, b"no-progress\n");
        write_pkt(&mut out, format!("want {oid}\n").as_bytes());
        write_pkt(&mut out, b"done\n");
        out.extend_from_slice(b"0000");
        assert!(parse_request(&out).is_ok());
        out
    }

    #[tokio::test]
    async fn fetch_streams_a_packfile_and_over_the_cap_is_busy() {
        if skip_if_no_git() {
            return;
        }
        let (dir, oid) = cache_with_object();
        let mut service = UploadService::new(1);
        service.slot_wait = Duration::from_millis(100);

        let held = Arc::clone(&service.slots)
            .acquire_owned()
            .await
            .expect("slot");
        assert!(matches!(
            service.fetch(dir.path(), OBJECT, fetch_request(&oid)).await,
            Err(UploadError::Busy)
        ));
        drop(held);

        let body = service
            .fetch(dir.path(), OBJECT, fetch_request(&oid))
            .await
            .expect("fetch");
        let bytes = body.collect().await.expect("body").to_bytes();
        assert!(
            bytes.windows(8).any(|w| w == b"packfile"),
            "a packfile section"
        );
        assert!(bytes.windows(4).any(|w| w == b"PACK"), "pack data");
        // The slot came back when the response ended.
        assert_eq!(service.slots.available_permits(), 1);
    }

    #[tokio::test]
    async fn the_advertisement_is_protocol_v2() {
        if skip_if_no_git() {
            return;
        }
        let (dir, _) = cache_with_object();
        let service = UploadService::new(1);
        let advertisement = service.advertise(dir.path(), OBJECT).await.expect("ad");
        assert!(advertisement.starts_with(b"000eversion 2\n"));
    }
}
