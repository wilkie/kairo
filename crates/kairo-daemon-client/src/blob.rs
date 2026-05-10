//! `Client::blob` and the [`BlobReader`] adapter.
//!
//! The daemon serves `GET /api/v1/blobs/{id}` as a chunked
//! `application/octet-stream` body. On the client side we
//! present that as an `AsyncRead` so callers can `tokio::io::
//! copy` it to a file or stdout without buffering. Errors
//! (404, 400, 500) come back as the JSON envelope, decoded
//! into a [`ClientError`] before any read happens.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::stream::{Stream, TryStreamExt};
use http_body_util::{BodyExt, BodyStream, Empty};
use hyper::body::{Bytes, Frame};
use hyper::Request;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, ReadBuf};
use tokio::net::UnixStream;
use tokio::time::timeout;
use tokio_util::io::StreamReader;

use crate::client::{Client, REQUEST_CONNECT_TIMEOUT};
use crate::envelope::decode_error;
use crate::error::{ClientError, ClientResult};

impl Client {
    /// `GET /api/v1/blobs/{blob_id}` — open the blob for
    /// streaming reads. The returned [`BlobReader`] implements
    /// [`AsyncRead`]; callers either `read_to_end` it (small
    /// blobs) or `tokio::io::copy` it into a sink (large blobs,
    /// the case streaming exists for).
    ///
    /// Errors (`Connect`, `Http { 404, ... }`, etc.) surface
    /// before the reader is constructed — once you have a
    /// [`BlobReader`], the response is 2xx and bytes are
    /// flowing.
    pub async fn blob(&self, blob_id: &str) -> ClientResult<BlobReader> {
        let stream = match timeout(
            REQUEST_CONNECT_TIMEOUT,
            UnixStream::connect(self.socket_path()),
        )
        .await
        {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => return Err(ClientError::Connect(error)),
            Err(_) => return Err(ClientError::Timeout(REQUEST_CONNECT_TIMEOUT)),
        };
        let io = TokioIo::new(stream);

        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|error| ClientError::Transport(Box::new(error)))?;

        // Drive the connection state machine in the background.
        // We deliberately don't await this — the body stream
        // below holds the only reference that keeps it alive,
        // and when the BlobReader drops the connection winds
        // down and the task exits.
        let conn_task = tokio::spawn(async move {
            let _ = conn.await;
        });

        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1/blobs/{blob_id}"))
            .header("host", "daemon")
            .body(Empty::<Bytes>::new())
            .map_err(|error| ClientError::Transport(Box::new(error)))?;
        let response = sender
            .send_request(req)
            .await
            .map_err(|error| ClientError::Transport(Box::new(error)))?;
        let status = response.status().as_u16();

        if !(200..300).contains(&status) {
            let bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|error| ClientError::Transport(Box::new(error)))?
                .to_bytes();
            drop(sender);
            let _ = conn_task.await;
            return Err(decode_error(status, &bytes));
        }

        let body = response.into_body();
        let data_stream = BodyStream::new(body)
            .try_filter_map(|frame: Frame<Bytes>| async move { Ok(frame.into_data().ok()) })
            .map_err(|error: hyper::Error| std::io::Error::other(error));

        // Hold the ownership chain in the reader so the
        // connection task and sender drop together with the
        // last byte read.
        Ok(BlobReader::new(data_stream, sender, conn_task))
    }
}

/// Streaming reader over a blob response body.
///
/// Implements [`AsyncRead`]; standard tokio I/O combinators
/// (`read_to_end`, `copy`, `BufReader`, etc.) just work.
///
/// Dropping the reader before EOF is safe: it drops the request
/// `sender` and the spawned connection task, which signals
/// hyper to close the underlying socket.
pub struct BlobReader {
    inner: Pin<Box<dyn AsyncRead + Send>>,
    // Held only for their `Drop` side effect: dropping `_sender`
    // closes the request half; dropping `_conn_task` aborts the
    // background driver. Together they tear down the connection
    // when the reader is dropped early.
    _sender: hyper::client::conn::http1::SendRequest<Empty<Bytes>>,
    _conn_task: tokio::task::JoinHandle<()>,
}

impl BlobReader {
    fn new<S>(
        stream: S,
        sender: hyper::client::conn::http1::SendRequest<Empty<Bytes>>,
        conn_task: tokio::task::JoinHandle<()>,
    ) -> Self
    where
        S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    {
        let reader = StreamReader::new(stream);
        Self {
            inner: Box::pin(reader),
            _sender: sender,
            _conn_task: conn_task,
        }
    }
}

impl std::fmt::Debug for BlobReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlobReader").finish_non_exhaustive()
    }
}

impl AsyncRead for BlobReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.inner.as_mut().poll_read(cx, buf)
    }
}
