//! HTTP-over-Unix-socket client for the Kairo daemon.
//!
//! Each request opens a fresh `connect(2)` and runs an HTTP/1
//! handshake; there is no connection pool yet. That's a fine
//! default for the read-only daemon under expected load — a CLI
//! invocation makes one or two requests per dispatch — and keeps
//! the lifetime story trivially simple. If pooling becomes
//! load-bearing (e.g., a long-lived web server doing many
//! requests), it lands as a follow-up; the public API on
//! [`Client`] is shaped so pooling can be added without changing
//! call sites.

use std::path::{Path, PathBuf};
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use serde::de::DeserializeOwned;
use tokio::net::UnixStream;
use tokio::time::timeout;

use crate::dto::{
    ActorGenesisJson, ActorTrustStatementJson, BranchTipDto, CapabilityHeadDto, ObjectByActorDto,
    ObjectBranchStatementJson, ObjectGenesisStatementJson, ObjectVersionTagStatementJson,
    StatementByActorDto, StatementValue, StatusInfo, ValidationResult, VersionInfo,
};
use crate::envelope::{decode_error, decode_success};
use crate::error::{ClientError, ClientResult};

/// Default timeout used by [`Client::probe`] when the caller
/// passes `None`.
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Connect-phase timeout used by request methods other than
/// `probe`. Bounded so a hung accept queue can't wedge the CLI.
pub(crate) const REQUEST_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Whole-request timeout (connect + handshake + response). Long
/// enough for modest blob streaming; future streaming methods
/// will use a different deadline (or none).
const REQUEST_DEADLINE: Duration = Duration::from_secs(30);

/// A handle bound to a daemon's listening Unix socket.
///
/// Cheap to clone (just a `PathBuf`). Connections are lazy — the
/// client does not contact the daemon until a method is called.
#[derive(Debug, Clone)]
pub struct Client {
    socket_path: PathBuf,
}

impl Client {
    /// Construct a client targeting the daemon listening on
    /// `socket_path` (typically `<store>/daemon.sock`). Does not
    /// connect.
    pub fn new<P: AsRef<Path>>(socket_path: P) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    /// Filesystem path of the daemon's listening socket.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Quick liveness probe: try `GET /api/v1/status`, returning
    /// `true` iff the daemon answers with a 2xx within
    /// `timeout_dur` (or `DEFAULT_PROBE_TIMEOUT` when `None`).
    ///
    /// Used by the CLI's probe-and-fall-back dispatch (see
    /// `specs/CLI.md` §3.3): missing socket → fall back to
    /// direct mode silently. Any error — connect refused, hang,
    /// non-2xx, malformed body — counts as "no daemon".
    pub async fn probe(&self, timeout_dur: Option<Duration>) -> bool {
        let deadline = timeout_dur.unwrap_or(DEFAULT_PROBE_TIMEOUT);
        matches!(timeout(deadline, self.do_probe()).await, Ok(Ok(())))
    }

    async fn do_probe(&self) -> ClientResult<()> {
        // Discard the response body — probe only cares about 2xx.
        let _: StatusInfo = self.get_json("/api/v1/status").await?;
        Ok(())
    }

    /// `GET /api/v1/version`.
    pub async fn version(&self) -> ClientResult<VersionInfo> {
        self.get_json("/api/v1/version").await
    }

    /// `GET /api/v1/status`.
    pub async fn status(&self) -> ClientResult<StatusInfo> {
        self.get_json("/api/v1/status").await
    }

    /// `GET /api/v1/actors/{actor_id}` — returns the actor's
    /// genesis JSON. The `id` is appended verbatim; the daemon
    /// returns 400 (`bad_request`) for shape-invalid IDs and 404
    /// (`not_found`) when the actor is absent from the store.
    pub async fn actor(&self, actor_id: &str) -> ClientResult<ActorGenesisJson> {
        self.get_json(&format!("/api/v1/actors/{actor_id}")).await
    }

    /// `GET /api/v1/actors/{actor_id}/statements` — every signed
    /// statement authored by the actor, sorted by `(created_at,
    /// statement_id)` ascending. `ObjectGenesis` is excluded
    /// server-side (it carries `created_by`, not the envelope
    /// `actor` field every other statement type uses).
    pub async fn list_statements_by_actor(
        &self,
        actor_id: &str,
    ) -> ClientResult<Vec<StatementByActorDto>> {
        self.get_json(&format!("/api/v1/actors/{actor_id}/statements"))
            .await
    }

    /// `GET /api/v1/actors/{actor_id}/objects` — every object
    /// whose `ObjectGenesis.created_by` is `actor_id`, sorted by
    /// `(created_at, object_id)` ascending. The complement to
    /// [`Self::list_statements_by_actor`] — together they answer
    /// "what is this actor responsible for in the store?".
    pub async fn list_objects_by_actor(
        &self,
        actor_id: &str,
    ) -> ClientResult<Vec<ObjectByActorDto>> {
        self.get_json(&format!("/api/v1/actors/{actor_id}/objects"))
            .await
    }

    /// `GET /api/v1/objects/{object_id}` — returns the object's
    /// genesis statement JSON.
    pub async fn object(&self, object_id: &str) -> ClientResult<ObjectGenesisStatementJson> {
        self.get_json(&format!("/api/v1/objects/{object_id}")).await
    }

    /// `GET /api/v1/statements/{statement_id}` — returns the
    /// statement's JSON envelope by id, polymorphic across
    /// statement kinds. The kind discriminator is inside the
    /// returned value.
    pub async fn statement(&self, statement_id: &str) -> ClientResult<StatementValue> {
        self.get_json(&format!("/api/v1/statements/{statement_id}"))
            .await
    }

    /// `GET /api/v1/branches/{object}` — list of `(actor, name)`
    /// branch heads on the object. Each entry is a lightweight
    /// summary; fetch the full envelope with [`Self::statement`]
    /// or [`Self::latest_branch`].
    pub async fn list_branches(&self, object_id: &str) -> ClientResult<Vec<BranchTipDto>> {
        self.get_json(&format!("/api/v1/branches/{object_id}"))
            .await
    }

    /// `GET /api/v1/branches/{object}/{name}/latest` — the
    /// chain-leaf `ObjectBranch` statement for `(actor, object,
    /// name)`. `actor` defaults to the object's `created_by`
    /// when omitted.
    pub async fn latest_branch(
        &self,
        object_id: &str,
        name: &str,
        actor: Option<&str>,
    ) -> ClientResult<ObjectBranchStatementJson> {
        let path = match actor {
            Some(actor) => format!("/api/v1/branches/{object_id}/{name}/latest?actor={actor}"),
            None => format!("/api/v1/branches/{object_id}/{name}/latest"),
        };
        self.get_json(&path).await
    }

    /// `GET /api/v1/version-tags/{object}/{version}` — the
    /// chain-leaf `ObjectVersionTag` statement for `(actor,
    /// object, version)`. Honors cross-actor `supersedes` when
    /// the successor's signer holds an `ObjectVersionTag`
    /// capability on the object. `actor` defaults to the
    /// object's `created_by` when omitted.
    pub async fn latest_version_tag(
        &self,
        object_id: &str,
        version: &str,
        actor: Option<&str>,
    ) -> ClientResult<ObjectVersionTagStatementJson> {
        let path = match actor {
            Some(actor) => {
                format!("/api/v1/version-tags/{object_id}/{version}?actor={actor}")
            }
            None => format!("/api/v1/version-tags/{object_id}/{version}"),
        };
        self.get_json(&path).await
    }

    /// `GET /api/v1/trust/{by_actor}/{of_actor}` — the chain-
    /// leaf `ActorTrust` statement (grant, block, or
    /// withdrawal). Returns 404 when `by_actor` has never
    /// expressed an opinion about `of_actor`.
    pub async fn trust(
        &self,
        by_actor: &str,
        of_actor: &str,
    ) -> ClientResult<ActorTrustStatementJson> {
        self.get_json(&format!("/api/v1/trust/{by_actor}/{of_actor}"))
            .await
    }

    /// `GET /api/v1/capabilities/{grantor}` — list of
    /// `(grantee, scope)` capability heads issued by the
    /// grantor. Mirrors `kairo capability list --grantor`.
    pub async fn list_capabilities_from(
        &self,
        grantor: &str,
    ) -> ClientResult<Vec<CapabilityHeadDto>> {
        self.get_json(&format!("/api/v1/capabilities/{grantor}"))
            .await
    }

    /// `GET /api/v1/verify-object/{object_id}` — verify the
    /// object's statement-layer state and return a structured
    /// [`ValidationResult`]. The daemon does not consult
    /// manifests or Git; the resulting `status` is
    /// `Indeterminate` for objects with revisions because the
    /// content layer is unprovable server-side.
    pub async fn verify_object(&self, object_id: &str) -> ClientResult<ValidationResult> {
        self.get_json(&format!("/api/v1/verify-object/{object_id}"))
            .await
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> ClientResult<T> {
        let path = path.to_owned();
        let work = async move {
            let (status, bytes) = self.send_get(&path).await?;
            if (200..300).contains(&status) {
                decode_success(&bytes)
            } else {
                Err(decode_error(status, &bytes))
            }
        };

        match timeout(REQUEST_DEADLINE, work).await {
            Ok(result) => result,
            Err(_) => Err(ClientError::Timeout(REQUEST_DEADLINE)),
        }
    }

    async fn send_get(&self, path: &str) -> ClientResult<(u16, Bytes)> {
        let stream = match timeout(
            REQUEST_CONNECT_TIMEOUT,
            UnixStream::connect(&self.socket_path),
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

        let conn_task = tokio::spawn(async move {
            // Connection task drains the underlying stream while
            // the request is in flight. Its return value is the
            // hyper connection error (if any); we surface those
            // through the request path, not here.
            let _ = conn.await;
        });

        let req = Request::builder()
            .method("GET")
            .uri(path)
            .header("host", "daemon")
            .body(Empty::<Bytes>::new())
            .map_err(|error| ClientError::Transport(Box::new(error)))?;

        let response = sender
            .send_request(req)
            .await
            .map_err(|error| ClientError::Transport(Box::new(error)))?;
        let status = response.status().as_u16();
        let body = response
            .into_body()
            .collect()
            .await
            .map_err(|error| ClientError::Transport(Box::new(error)))?
            .to_bytes();

        // Drop sender so the connection task notices the half-close
        // and finishes; await it briefly so any tail error on the
        // wire is observed before the function returns.
        drop(sender);
        let _ = conn_task.await;

        Ok((status, body))
    }
}
