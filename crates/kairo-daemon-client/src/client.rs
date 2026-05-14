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

use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use kairo_core::Timestamp;
use kairo_identity::SecretSigningKey;
use kairo_statement::json::{
    ActorCapabilityGrantStatementJson as KsActorCapabilityGrantStatementJson,
    ActorCapabilityRevocationStatementJson as KsActorCapabilityRevocationStatementJson,
    ActorTrustStatementJson as KsActorTrustStatementJson,
    ObjectBranchStatementJson as KsObjectBranchStatementJson,
    ObjectGenesisStatementJson as KsObjectGenesisStatementJson,
    ObjectRevisionStatementJson as KsObjectRevisionStatementJson,
    ObjectVersionTagStatementJson as KsObjectVersionTagStatementJson,
};
use kairo_statement::{
    ActorCapabilityGrantBody, ActorCapabilityRevocationBody, ActorTrustBody, ObjectBranchBody,
    ObjectGenesisStatement, ObjectRevisionBody, ObjectVersionTagBody, SignedStatement,
};

use crate::auth::{random_nonce, sign_request, AuthEnvelope};
use serde::de::DeserializeOwned;
use tokio::net::UnixStream;
use tokio::time::timeout;

use crate::dto::{
    ActorGenesisJson, ActorTrustStatementJson, BranchTipDto, CapabilityHeadDto,
    ObjectBranchStatementJson, ObjectByActorDto, ObjectGenesisStatementJson,
    ObjectVersionTagStatementJson, PutStatementResult, StatementByActorDto, StatementValue,
    StatusInfo, ValidationResult, VersionInfo,
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

    /// `POST /api/v1/statements` — persist an `ActorTrust`
    /// statement through the daemon's write surface
    /// (`DECISIONS.md §13` / §14).
    ///
    /// The caller signs the statement client-side, and the same
    /// signing key signs the request envelope (Phase B,
    /// `DECISIONS.md §14`). The daemon authenticates the request
    /// against the actor's active key, re-verifies the statement
    /// signature, then writes through
    /// `FilesystemStore::put_actor_trust`. Returns the persisted
    /// `statement_id` and the echoed body so callers don't need a
    /// follow-up read.
    pub async fn put_actor_trust(
        &self,
        signed: &SignedStatement<ActorTrustBody>,
        secret: &SecretSigningKey,
    ) -> ClientResult<PutStatementResult> {
        let dto = KsActorTrustStatementJson::from_statement(signed);
        let body = serde_json::to_vec(&dto)
            .map_err(|error| ClientError::Decode(format!("serialize ActorTrust body: {error}")))?;
        self.post_signed_statement(body, signed.signature().actor().clone(), secret)
            .await
    }

    /// `POST /api/v1/statements` for an `ObjectRevision`. The
    /// signed envelope is generated client-side from the local
    /// keystore; the daemon re-verifies and persists.
    pub async fn put_object_revision(
        &self,
        signed: &SignedStatement<ObjectRevisionBody>,
        secret: &SecretSigningKey,
    ) -> ClientResult<PutStatementResult> {
        let dto = KsObjectRevisionStatementJson::from_statement(signed);
        let body = serde_json::to_vec(&dto).map_err(|error| {
            ClientError::Decode(format!("serialize ObjectRevision body: {error}"))
        })?;
        self.post_signed_statement(body, signed.signature().actor().clone(), secret)
            .await
    }

    /// `POST /api/v1/statements` for an `ObjectBranch`. Used by
    /// `kairo branch set` when dispatch routes through the daemon.
    pub async fn put_object_branch(
        &self,
        signed: &SignedStatement<ObjectBranchBody>,
        secret: &SecretSigningKey,
    ) -> ClientResult<PutStatementResult> {
        let dto = KsObjectBranchStatementJson::from_statement(signed);
        let body = serde_json::to_vec(&dto)
            .map_err(|error| ClientError::Decode(format!("serialize ObjectBranch body: {error}")))?;
        self.post_signed_statement(body, signed.signature().actor().clone(), secret)
            .await
    }

    /// `POST /api/v1/statements` for an `ObjectVersionTag`. Used
    /// by `kairo tag bind` when dispatch routes through the
    /// daemon.
    pub async fn put_object_version_tag(
        &self,
        signed: &SignedStatement<ObjectVersionTagBody>,
        secret: &SecretSigningKey,
    ) -> ClientResult<PutStatementResult> {
        let dto = KsObjectVersionTagStatementJson::from_statement(signed);
        let body = serde_json::to_vec(&dto).map_err(|error| {
            ClientError::Decode(format!("serialize ObjectVersionTag body: {error}"))
        })?;
        self.post_signed_statement(body, signed.signature().actor().clone(), secret)
            .await
    }

    /// `POST /api/v1/statements` for an `ObjectGenesisStatement`.
    /// The result's `statement_id` field echoes the persisted
    /// `object_id` — ObjectGenesis has no separate statement-id
    /// concept; the object_id is the identity.
    pub async fn put_object_genesis(
        &self,
        statement: &ObjectGenesisStatement,
        secret: &SecretSigningKey,
    ) -> ClientResult<PutStatementResult> {
        let dto = KsObjectGenesisStatementJson::from_statement(statement);
        let body = serde_json::to_vec(&dto)
            .map_err(|error| ClientError::Decode(format!("serialize ObjectGenesis body: {error}")))?;
        self.post_signed_statement(body, statement.signature().actor().clone(), secret)
            .await
    }

    /// `POST /api/v1/statements` for an `ActorCapabilityGrant`.
    /// Used by `kairo capability grant` when dispatch routes
    /// through the daemon.
    pub async fn put_actor_capability_grant(
        &self,
        signed: &SignedStatement<ActorCapabilityGrantBody>,
        secret: &SecretSigningKey,
    ) -> ClientResult<PutStatementResult> {
        let dto = KsActorCapabilityGrantStatementJson::from_statement(signed);
        let body = serde_json::to_vec(&dto).map_err(|error| {
            ClientError::Decode(format!("serialize ActorCapabilityGrant body: {error}"))
        })?;
        self.post_signed_statement(body, signed.signature().actor().clone(), secret)
            .await
    }

    /// `POST /api/v1/statements` for an `ActorCapabilityRevocation`.
    /// Used by `kairo capability revoke` when dispatch routes
    /// through the daemon.
    pub async fn put_actor_capability_revocation(
        &self,
        signed: &SignedStatement<ActorCapabilityRevocationBody>,
        secret: &SecretSigningKey,
    ) -> ClientResult<PutStatementResult> {
        let dto = KsActorCapabilityRevocationStatementJson::from_statement(signed);
        let body = serde_json::to_vec(&dto).map_err(|error| {
            ClientError::Decode(format!("serialize ActorCapabilityRevocation body: {error}"))
        })?;
        self.post_signed_statement(body, signed.signature().actor().clone(), secret)
            .await
    }

    /// Sign and POST a serialized statement envelope at
    /// `/api/v1/statements`. Common helper behind every
    /// `put_*` method: builds the Phase B request envelope
    /// (`DECISIONS.md §14`), formats the `Authorization`
    /// header, ships the bytes, decodes the response.
    async fn post_signed_statement(
        &self,
        body: Vec<u8>,
        actor: kairo_core::ActorId,
        secret: &SecretSigningKey,
    ) -> ClientResult<PutStatementResult> {
        let nonce = random_nonce()
            .map_err(|error| ClientError::Transport(Box::new(error)))?;
        let timestamp = Timestamp::now().seconds();
        let envelope = sign_request(
            "POST",
            "/api/v1/statements",
            &body,
            actor,
            secret,
            timestamp,
            nonce,
        );
        self.post_json_with_auth("/api/v1/statements", body, envelope)
            .await
    }

    async fn post_json_with_auth<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Vec<u8>,
        auth: AuthEnvelope,
    ) -> ClientResult<T> {
        let path = path.to_owned();
        let auth_header = auth.format();
        let work = async move {
            let (status, bytes) = self.send_post(&path, body, &auth_header).await?;
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

    async fn send_post(
        &self,
        path: &str,
        body: Vec<u8>,
        auth_header: &str,
    ) -> ClientResult<(u16, Bytes)> {
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
            let _ = conn.await;
        });

        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header("host", "daemon")
            .header("content-type", "application/json")
            .header("authorization", auth_header)
            .body(Full::new(Bytes::from(body)))
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

        drop(sender);
        let _ = conn_task.await;

        Ok((status, body))
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
