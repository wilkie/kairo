//! Streaming an object's Git pack as a response body
//! (`specs/PHASE_2_FEDERATION.md` slice 4).
//!
//! `git pack-objects` runs on the blocking pool and writes into a
//! bounded channel. The response body drains that channel, so a large
//! pack is never held in memory and a slow peer applies backpressure
//! to `pack-objects` instead of growing a buffer. If `pack-objects`
//! fails partway through, the body ends in an error and the transfer
//! aborts. The receiver's `git index-pack` then rejects the truncated
//! pack, because its trailing checksum never arrives.

use std::io::{self, Write};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes};
use http_body::Frame;
use kairo_git::GitCache;
use tokio::sync::mpsc;

/// Chunks in flight between `pack-objects` and the socket.
const CHANNEL_DEPTH: usize = 8;
const CHUNK_BYTES: usize = 64 * 1024;

type Chunk = Result<Bytes, io::Error>;

/// Whether the managed cache under `git_root` has a per-object repo
/// for `object_id`. Checked without opening the cache, which would
/// initialize the pool, and this process never writes to the store.
pub(crate) fn has_pack(git_root: &Path, object_id: &str) -> bool {
    kairo_git::object_repo_path(git_root, object_id).is_ok_and(|path| path.exists())
}

/// A response body streaming `object_id`'s pack from the cache at
/// `git_root`. The caller has already checked [`has_pack`].
pub(crate) fn pack_body(git_root: &Path, object_id: &str) -> Body {
    let (sender, receiver) = mpsc::channel::<Chunk>(CHANNEL_DEPTH);
    let git_root = git_root.to_path_buf();
    let object_id = object_id.to_owned();
    tokio::task::spawn_blocking(move || {
        let result = GitCache::open(&git_root).and_then(|cache| {
            let mut sink = io::BufWriter::with_capacity(
                CHUNK_BYTES,
                ChannelWriter {
                    sender: sender.clone(),
                },
            );
            cache.pack_for_object_to(&object_id, &mut sink)?;
            sink.flush().map_err(|source| kairo_git::GitError::CacheIo {
                path: git_root.clone(),
                source,
            })
        });
        if let Err(error) = result {
            tracing::error!(object = %object_id, %error, "streaming a Git pack failed");
            let _ = sender.blocking_send(Err(io::Error::other("pack stream failed")));
        }
    });
    Body::new(ChannelBody { receiver })
}

/// `Write` end: each write becomes one chunk on the channel, blocking
/// while the channel is full. A dropped receiver (the peer hung up)
/// surfaces as `BrokenPipe`, which stops `pack-objects`.
struct ChannelWriter {
    sender: mpsc::Sender<Chunk>,
}

impl Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.sender
            .blocking_send(Ok(Bytes::copy_from_slice(buf)))
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Body end: yields chunks until the writer side drops.
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
