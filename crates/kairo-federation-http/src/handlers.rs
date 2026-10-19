//! Request handlers for `specs/PHASE_2_FEDERATION.md` §2.1.
//!
//! Every per-record endpoint answers from the published view: an id
//! outside every published closure gets the same `404 fed_not_found`
//! as an id the store doesn't hold (§16 sub-decision 3). Store reads
//! are blocking filesystem I/O, so they run on the blocking pool.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use kairo_core::{ActorId, BlobId, ObjectId, StatementId};
use kairo_federation_core::{
    ActorListing, Announcement, AnnouncementAccepted, BatchedStatement, Cursor, FedErrorCode,
    FedErrorEnvelope, GitAvailability, ObjectListing, PageRequest, PageRequestError,
    PublishedObjects, PushAccepted, PushPlan, PushPlanRequest, RequestBody, StatementBatch,
    StatementBatchRequest, ANNOUNCE_PATH, API_PREFIX, MAX_PLAN_BYTES, PUSH_PATH, PUSH_PLAN_PATH,
    STATEMENT_BATCH_PATH, WELL_KNOWN_PATH,
};
use kairo_identity::json::ActorGenesisJson;
use kairo_statement::{TrustDecision, TrustScope};
use kairo_store::{
    ActorStore, BlobStore, InboxAnnouncement, InboxPut, InboxStore, InboxUpload, StoreError,
    TrustResolver,
};
use serde::Serialize;

use crate::node::Node;
use crate::view::PublishedView;

const JSON: &str = "application/json";
const OCTET_STREAM: &str = "application/octet-stream";

type PathId = Result<Path<String>, PathRejection>;
type RawQuery = Result<Query<HashMap<String, String>>, QueryRejection>;

pub(crate) fn router(node: Node) -> Router {
    Router::new()
        .route(WELL_KNOWN_PATH, get(node_document))
        .route(&format!("{API_PREFIX}/objects"), get(published_objects))
        .route(&format!("{API_PREFIX}/objects/:id"), get(object_listing))
        .route(
            &format!("{API_PREFIX}/objects/:id/genesis"),
            get(object_genesis),
        )
        .route(
            &format!("{API_PREFIX}/objects/:id/git/info/refs"),
            get(git_info_refs),
        )
        .route(
            &format!("{API_PREFIX}/objects/:id/git/git-upload-pack"),
            post(git_upload_pack),
        )
        .route(&format!("{API_PREFIX}/actors/:id"), get(actor_listing))
        .route(&format!("{API_PREFIX}/statements/:id"), get(statement))
        .route(
            &format!("{API_PREFIX}{STATEMENT_BATCH_PATH}"),
            post(statement_batch),
        )
        .route(&format!("{API_PREFIX}/blobs/:id"), get(blob))
        .route(&format!("{API_PREFIX}{ANNOUNCE_PATH}"), post(announce))
        .route(&format!("{API_PREFIX}{PUSH_PLAN_PATH}"), post(push_plan))
        .route(&format!("{API_PREFIX}{PUSH_PATH}"), post(push_upload))
        .fallback(not_found)
        .layer(middleware::from_fn(read_only))
        .with_state(node)
}

// --- responses ------------------------------------------------------

/// A request that ends in a federation error envelope.
#[derive(Debug)]
struct Fail {
    code: FedErrorCode,
    message: String,
}

impl Fail {
    fn new(code: FedErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn not_found() -> Self {
        Self::new(FedErrorCode::FedNotFound, "not found")
    }

    /// Log the store failure; tell the peer nothing about it.
    fn store(source: &StoreError) -> Self {
        tracing::error!(error = %source, "store read failed while serving a federation request");
        Self::new(FedErrorCode::FedStoreError, "store read failed")
    }

    /// `Missing` is the ordinary not-found; anything else is a store
    /// failure.
    fn from_read(source: &StoreError) -> Self {
        match source {
            StoreError::Missing => Self::not_found(),
            other => Self::store(other),
        }
    }
}

impl IntoResponse for Fail {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.code.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        match serde_json::to_vec(&FedErrorEnvelope::new(self.code, self.message)) {
            Ok(bytes) => bytes_response(status, JSON, bytes),
            Err(_) => status.into_response(),
        }
    }
}

type Reply = Result<Response, Fail>;

fn bytes_response(status: StatusCode, content_type: &'static str, bytes: Vec<u8>) -> Response {
    let mut response = (status, Body::from(bytes)).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

fn json_response<T: Serialize>(value: &T) -> Reply {
    let bytes = serde_json::to_vec(value).map_err(|serialize_error| {
        tracing::error!(%serialize_error, "failed to serialize a federation response");
        Fail::new(FedErrorCode::FedStoreError, "store read failed")
    })?;
    Ok(bytes_response(StatusCode::OK, JSON, bytes))
}

/// Run blocking store work off the async runtime.
async fn run_blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Fail> + Send + 'static,
) -> Result<T, Fail> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|join_error| {
            tracing::error!(%join_error, "federation request handler panicked");
            Err(Fail::new(FedErrorCode::FedStoreError, "store read failed"))
        })
}

/// [`run_blocking`] for work that produces the whole response.
async fn blocking(work: impl FnOnce() -> Reply + Send + 'static) -> Response {
    run_blocking(work)
        .await
        .unwrap_or_else(IntoResponse::into_response)
}

fn view(node: &Node) -> Result<Arc<PublishedView>, Fail> {
    node.views.get(&node.store).map_err(|e| Fail::store(&e))
}

fn parse_id<T>(
    raw: PathId,
    parse: impl FnOnce(String) -> Result<T, kairo_core::IdError>,
) -> Result<T, Fail> {
    let Path(raw) = raw.map_err(|_| Fail::new(FedErrorCode::FedBadId, "malformed path"))?;
    parse(raw).map_err(|e| Fail::new(FedErrorCode::FedBadId, format!("invalid id: {e}")))
}

fn page_request(query: RawQuery) -> Result<PageRequest, Fail> {
    let Query(query) =
        query.map_err(|_| Fail::new(FedErrorCode::FedBadCursor, "malformed query string"))?;
    PageRequest::from_query(
        query.get("limit").map(String::as_str),
        query.get("cursor").map(String::as_str),
    )
    .map_err(|e| match e {
        PageRequestError::BadLimit => Fail::new(FedErrorCode::FedBadLimit, e.to_string()),
        PageRequestError::BadCursor(_) => Fail::new(FedErrorCode::FedBadCursor, e.to_string()),
    })
}

// --- middleware and fallback ----------------------------------------

/// Only `GET`, `HEAD`, and five `POST`s exist; everything else is
/// `405` with the federation envelope, on every path including unknown
/// ones.
async fn read_only(request: Request, next: Next) -> Response {
    let method = request.method();
    // Two `POST`s only read: Git's upload-pack RPC (DECISIONS.md §22)
    // and the statement batch (§24), whose id list doesn't fit a URL.
    // The others are the authenticated announce, push plan, and upload
    // (§26), whose only write is an inbox entry.
    let path = request.uri().path();
    let allowed_post = method == Method::POST
        && (path.ends_with(UPLOAD_PACK_SUFFIX)
            || path == format!("{API_PREFIX}{STATEMENT_BATCH_PATH}")
            || path == format!("{API_PREFIX}{ANNOUNCE_PATH}")
            || path == format!("{API_PREFIX}{PUSH_PLAN_PATH}")
            || path == format!("{API_PREFIX}{PUSH_PATH}"));
    if method == Method::GET || method == Method::HEAD || allowed_post {
        return next.run(request).await;
    }
    let mut response = Fail::new(
        FedErrorCode::FedMethodNotAllowed,
        "federation endpoints are read-only",
    )
    .into_response();
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
    response
}

async fn not_found() -> Response {
    Fail::not_found().into_response()
}

// --- endpoints ------------------------------------------------------

async fn node_document(State(node): State<Node>) -> Response {
    json_response(node.document.as_ref()).unwrap_or_else(IntoResponse::into_response)
}

async fn published_objects(State(node): State<Node>, query: RawQuery) -> Response {
    let page = match page_request(query) {
        Ok(page) => page,
        Err(fail) => return fail.into_response(),
    };
    blocking(move || {
        let view = view(&node)?;
        // Object ids are base58, so an id is a valid cursor as is.
        let start = page.cursor().map_or(0, |cursor| {
            view.objects
                .partition_point(|object| object.as_str() <= cursor.as_str())
        });
        let end = start
            .saturating_add(page.limit() as usize)
            .min(view.objects.len());
        let objects = view.objects[start..end].to_vec();
        let next_cursor = if end < view.objects.len() {
            objects
                .last()
                .and_then(|last| Cursor::new(last.as_str()).ok())
        } else {
            None
        };
        json_response(&PublishedObjects {
            schema: Default::default(),
            objects,
            next_cursor,
        })
    })
    .await
}

/// The four listing sections, in paging order. Each cursor is
/// `<tag>.<last id>`, so a page boundary survives records being added
/// between requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Section {
    Statements,
    Authority,
    Actors,
    Blobs,
}

impl Section {
    const ALL: [Self; 4] = [Self::Statements, Self::Authority, Self::Actors, Self::Blobs];

    fn tag(self) -> &'static str {
        match self {
            Self::Statements => "st",
            Self::Authority => "au",
            Self::Actors => "ac",
            Self::Blobs => "bl",
        }
    }

    fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|section| section.tag() == tag)
    }
}

fn parse_listing_cursor(cursor: &Cursor) -> Result<(Section, &str), Fail> {
    cursor
        .as_str()
        .split_once('.')
        .and_then(|(tag, id)| Some((Section::from_tag(tag)?, id)))
        .filter(|(_, id)| !id.is_empty())
        .ok_or_else(|| {
            Fail::new(
                FedErrorCode::FedBadCursor,
                "cursor does not name a position",
            )
        })
}

async fn object_listing(State(node): State<Node>, id: PathId, query: RawQuery) -> Response {
    let (object, page) = match (parse_id(id, ObjectId::new), page_request(query)) {
        (Ok(object), Ok(page)) => (object, page),
        (Err(fail), _) | (_, Err(fail)) => return fail.into_response(),
    };
    blocking(move || {
        let view = view(&node)?;
        let closure = view.closures.get(&object).ok_or_else(Fail::not_found)?;

        let mut items: Vec<(Section, &str)> = Vec::new();
        items.extend(
            closure
                .statements
                .iter()
                .map(|id| (Section::Statements, id.as_str())),
        );
        items.extend(
            closure
                .authority
                .iter()
                .map(|id| (Section::Authority, id.as_str())),
        );
        items.extend(
            closure
                .actors
                .iter()
                .map(|id| (Section::Actors, id.as_str())),
        );
        items.extend(closure.blobs.iter().map(|id| (Section::Blobs, id.as_str())));

        let start = match page.cursor() {
            None => 0,
            Some(cursor) => {
                let position = parse_listing_cursor(cursor)?;
                items.partition_point(|item| *item <= position)
            }
        };
        let end = start.saturating_add(page.limit() as usize).min(items.len());
        let slice = &items[start..end];

        let mut listing = ObjectListing {
            schema: Default::default(),
            object: object.clone(),
            statements: Vec::new(),
            authority: Vec::new(),
            actors: Vec::new(),
            blobs: Vec::new(),
            git: page.cursor().is_none().then(|| GitAvailability {
                available: crate::upload::has_git(&crate::git_root(&node.store), object.as_str()),
            }),
            next_cursor: None,
        };
        // The ids came from typed sets, so re-parsing can't fail; if
        // it did, that would be a bug, surfaced as a store error.
        let reparse = |_| {
            Fail::store(&StoreError::Rejected {
                reason: "closure id failed to re-parse".to_owned(),
            })
        };
        for (section, id) in slice {
            match section {
                Section::Statements => listing
                    .statements
                    .push(StatementId::new(*id).map_err(reparse)?),
                Section::Authority => listing
                    .authority
                    .push(StatementId::new(*id).map_err(reparse)?),
                Section::Actors => listing.actors.push(ActorId::new(*id).map_err(reparse)?),
                Section::Blobs => listing.blobs.push(BlobId::new(*id).map_err(reparse)?),
            }
        }
        if end < items.len() {
            listing.next_cursor = slice
                .last()
                .and_then(|(section, id)| Cursor::new(format!("{}.{id}", section.tag())).ok());
        }
        json_response(&listing)
    })
    .await
}

async fn object_genesis(State(node): State<Node>, id: PathId) -> Response {
    let object = match parse_id(id, ObjectId::new) {
        Ok(object) => object,
        Err(fail) => return fail.into_response(),
    };
    blocking(move || {
        if !view(&node)?.closures.contains_key(&object) {
            return Err(Fail::not_found());
        }
        let bytes = node
            .store
            .get_object_genesis_bytes(&object)
            .map_err(|e| Fail::from_read(&e))?;
        Ok(bytes_response(StatusCode::OK, JSON, bytes))
    })
    .await
}

// --- Git smart HTTP (DECISIONS.md §22) -----------------------------

const UPLOAD_PACK_SUFFIX: &str = "/git/git-upload-pack";
const UPLOAD_PACK_REQUEST: &str = "application/x-git-upload-pack-request";

/// Largest upload-pack request body, after decompression. A request is
/// wants and one negotiation round of haves; Git sends haves in
/// batches far below this.
const MAX_UPLOAD_REQUEST_BYTES: usize = 4 * 1024 * 1024;

fn bad_git_request(message: impl Into<String>) -> Fail {
    Fail::new(FedErrorCode::FedBadGitRequest, message)
}

/// The published commits of `object`, as hex ids, if the object is
/// published and this node's cache has a repo with refs for it.
/// `404` otherwise, like every other record.
async fn published_commits(node: &Node, object: &ObjectId) -> Result<BTreeSet<String>, Fail> {
    let node = node.clone();
    let object = object.clone();
    run_blocking(move || {
        let view = view(&node)?;
        let closure = view.closures.get(&object).ok_or_else(Fail::not_found)?;
        if !crate::upload::has_git(&crate::git_root(&node.store), object.as_str()) {
            return Err(Fail::not_found());
        }
        Ok(closure
            .commits
            .iter()
            .map(|commit| commit.oid().to_owned())
            .collect())
    })
    .await
}

fn require_v2(headers: &HeaderMap) -> Result<(), Fail> {
    let v2 = headers
        .get("git-protocol")
        .and_then(|value| value.to_str().ok())
        .is_some_and(crate::git_protocol::wants_v2);
    if v2 {
        Ok(())
    } else {
        Err(bad_git_request(
            "this node speaks Git protocol v2 only; use git >= 2.29 or set protocol.version=2",
        ))
    }
}

fn git_response(content_type: &'static str, body: Body) -> Response {
    let mut response = (StatusCode::OK, body).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

fn upload_failure(object: &ObjectId, error: crate::upload::UploadError) -> Response {
    match error {
        crate::upload::UploadError::Busy => {
            let mut response = Fail::new(
                FedErrorCode::FedBusy,
                "the node is serving as many Git fetches as it allows; retry shortly",
            )
            .into_response();
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from(crate::upload::RETRY_AFTER_SECS),
            );
            response
        }
        crate::upload::UploadError::Git(message) => {
            tracing::error!(object = %object, %message, "git upload-pack failed");
            Fail::new(FedErrorCode::FedStoreError, "store read failed").into_response()
        }
    }
}

/// `GET …/git/info/refs?service=git-upload-pack`: the protocol v2
/// capability advertisement, narrowed to what this server answers.
async fn git_info_refs(
    State(node): State<Node>,
    id: PathId,
    query: RawQuery,
    headers: HeaderMap,
) -> Response {
    let object = match parse_id(id, ObjectId::new) {
        Ok(object) => object,
        Err(fail) => return fail.into_response(),
    };
    let service = query
        .ok()
        .and_then(|Query(params)| params.get("service").cloned());
    if service.as_deref() != Some("git-upload-pack") {
        return bad_git_request("only service=git-upload-pack is served").into_response();
    }
    if let Err(fail) = published_commits(&node, &object).await {
        return fail.into_response();
    }
    if let Err(fail) = require_v2(&headers) {
        return fail.into_response();
    }
    let raw = match node
        .uploads
        .advertise(&crate::git_root(&node.store), object.as_str())
        .await
    {
        Ok(raw) => raw,
        Err(error) => return upload_failure(&object, error),
    };
    match crate::git_protocol::filter_advertisement(&raw) {
        Ok(advertisement) => git_response(
            "application/x-git-upload-pack-advertisement",
            Body::from(advertisement),
        ),
        Err(refusal) => {
            tracing::error!(object = %object, reason = %refusal.0, "unexpected upload-pack advertisement");
            Fail::new(FedErrorCode::FedStoreError, "store read failed").into_response()
        }
    }
}

/// `POST …/git/git-upload-pack`: one stateless protocol v2 command.
/// `ls-refs` is answered here; `fetch` is filtered to published
/// commits and handed to `git upload-pack` (`crate::git_protocol`).
async fn git_upload_pack(
    State(node): State<Node>,
    id: PathId,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let object = match parse_id(id, ObjectId::new) {
        Ok(object) => object,
        Err(fail) => return fail.into_response(),
    };
    let published = match published_commits(&node, &object).await {
        Ok(published) => published,
        Err(fail) => return fail.into_response(),
    };
    if let Err(fail) = require_v2(&headers) {
        return fail.into_response();
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    if content_type != Some(UPLOAD_PACK_REQUEST) {
        return bad_git_request(format!("expected Content-Type {UPLOAD_PACK_REQUEST}"))
            .into_response();
    }
    let gzip = match headers
        .get(header::CONTENT_ENCODING)
        .map(|value| value.to_str().unwrap_or_default())
    {
        None => false,
        Some("gzip" | "x-gzip") => true,
        Some(other) => {
            return bad_git_request(format!("unsupported Content-Encoding {other:?}"))
                .into_response()
        }
    };
    let raw = match axum::body::to_bytes(body, MAX_UPLOAD_REQUEST_BYTES).await {
        Ok(raw) => raw,
        Err(_) => return bad_git_request("request body too large").into_response(),
    };
    let request = if gzip {
        match gunzip(&raw) {
            Ok(request) => request,
            Err(fail) => return fail.into_response(),
        }
    } else {
        raw.to_vec()
    };
    let parsed = match crate::git_protocol::parse_request(&request) {
        Ok(parsed) => parsed,
        Err(refusal) => return bad_git_request(refusal.0).into_response(),
    };
    let result = "application/x-git-upload-pack-result";
    match parsed.command {
        "ls-refs" => git_response(
            result,
            Body::from(crate::git_protocol::ls_refs(&parsed, &published)),
        ),
        "fetch" => {
            let filtered = match crate::git_protocol::filter_fetch(&parsed, &published) {
                Ok(filtered) => filtered,
                Err(refusal) => return bad_git_request(refusal.0).into_response(),
            };
            match node
                .uploads
                .fetch(&crate::git_root(&node.store), object.as_str(), filtered)
                .await
            {
                Ok(body) => git_response(result, body),
                Err(error) => upload_failure(&object, error),
            }
        }
        other => bad_git_request(format!("command {other:?} is not served")).into_response(),
    }
}

fn gunzip(raw: &[u8]) -> Result<Vec<u8>, Fail> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(raw)
        .take(MAX_UPLOAD_REQUEST_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| bad_git_request("malformed gzip request body"))?;
    if out.len() > MAX_UPLOAD_REQUEST_BYTES {
        return Err(bad_git_request("request body too large"));
    }
    Ok(out)
}

async fn actor_listing(State(node): State<Node>, id: PathId) -> Response {
    let actor = match parse_id(id, ActorId::new) {
        Ok(actor) => actor,
        Err(fail) => return fail.into_response(),
    };
    blocking(move || {
        let view = view(&node)?;
        let key_events = view.actors.get(&actor).ok_or_else(Fail::not_found)?;
        let genesis = node
            .store
            .get_actor(&actor)
            .map_err(|e| Fail::from_read(&e))?;
        json_response(&ActorListing {
            schema: Default::default(),
            actor: actor.clone(),
            genesis: ActorGenesisJson::from_body(&genesis),
            key_events: key_events.clone(),
        })
    })
    .await
}

async fn statement(State(node): State<Node>, id: PathId) -> Response {
    let statement = match parse_id(id, StatementId::new) {
        Ok(statement) => statement,
        Err(fail) => return fail.into_response(),
    };
    blocking(move || {
        if !view(&node)?.statements.contains(&statement) {
            return Err(Fail::not_found());
        }
        let bytes = node
            .store
            .get_statement_bytes(&statement)
            .map_err(|e| Fail::from_read(&e))?;
        Ok(bytes_response(StatusCode::OK, JSON, bytes))
    })
    .await
}

/// Largest statement batch request body (`DECISIONS.md` §24): 256 ids
/// fit with room to spare.
const MAX_BATCH_REQUEST_BYTES: usize = 64 * 1024;

/// `POST /federation/v1/statement-batch`: the requested statements the
/// node serves, in request order, each the stored JSON verbatim. An id
/// outside every published closure is left out exactly like one the
/// store doesn't hold (`DECISIONS.md` §24 sub-decision 2).
async fn statement_batch(State(node): State<Node>, body: Body) -> Response {
    let bad = |message: String| Fail::new(FedErrorCode::FedBadBatch, message).into_response();
    let Ok(raw) = axum::body::to_bytes(body, MAX_BATCH_REQUEST_BYTES).await else {
        return bad(format!(
            "batch request body over {MAX_BATCH_REQUEST_BYTES} bytes"
        ));
    };
    let request: StatementBatchRequest = match serde_json::from_slice(&raw) {
        Ok(request) => request,
        Err(error) => return bad(format!("malformed batch request: {error}")),
    };
    if let Err(error) = request.validate() {
        return bad(error.to_string());
    }
    blocking(move || {
        let view = view(&node)?;
        let mut seen = std::collections::HashSet::new();
        let mut statements = Vec::new();
        for id in request.ids {
            if !seen.insert(id.clone()) || !view.statements.contains(&id) {
                continue;
            }
            let bytes = match node.store.get_statement_bytes(&id) {
                Ok(bytes) => bytes,
                Err(StoreError::Missing) => continue,
                Err(error) => return Err(Fail::store(&error)),
            };
            // Stored statements are JSON; one that isn't is corrupt.
            let text = String::from_utf8(bytes).map_err(|_| {
                Fail::store(&StoreError::Rejected {
                    reason: format!("stored statement {id} is not UTF-8"),
                })
            })?;
            let statement = serde_json::value::RawValue::from_string(text).map_err(|error| {
                Fail::store(&StoreError::Rejected {
                    reason: format!("stored statement {id} is not JSON: {error}"),
                })
            })?;
            statements.push(BatchedStatement { id, statement });
        }
        json_response(&StatementBatch {
            schema: Default::default(),
            statements,
        })
    })
    .await
}

async fn blob(State(node): State<Node>, id: PathId) -> Response {
    let blob = match parse_id(id, BlobId::new) {
        Ok(blob) => blob,
        Err(fail) => return fail.into_response(),
    };
    blocking(move || {
        if !view(&node)?.blobs.contains(&blob) {
            return Err(Fail::not_found());
        }
        let bytes = node
            .store
            .get_blob(&blob)
            .map_err(|e| Fail::from_read(&e))?;
        Ok(bytes_response(StatusCode::OK, OCTET_STREAM, bytes))
    })
    .await
}

// --- announce (DECISIONS.md §26) ---------------------------------------

/// Largest announcement body: 256 object ids fit with room to spare.
const MAX_ANNOUNCE_BYTES: usize = 64 * 1024;

/// `POST /federation/v1/announce`: put an authenticated, trusted,
/// accepted announcement in the inbox for `kairo federation receive`.
/// Checks run in order, so an unauthenticated request learns nothing
/// about trust or the accept list.
async fn announce(
    State(node): State<Node>,
    headers: HeaderMap,
    uri: axum::http::Uri,
    body: Body,
) -> Response {
    let Ok(raw) = axum::body::to_bytes(body, MAX_ANNOUNCE_BYTES).await else {
        return Fail::new(
            FedErrorCode::FedBadAnnouncement,
            format!("announcement body over {MAX_ANNOUNCE_BYTES} bytes"),
        )
        .into_response();
    };
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let target = uri
        .path_and_query()
        .map_or_else(|| uri.path().to_owned(), |pq| pq.as_str().to_owned());
    let result = run_blocking(move || {
        let who = node
            .authenticate_request("POST", &target, authorization.as_deref(), &raw)
            .map_err(|error| auth_failure(&error))?;
        let bad = |message: String| Fail::new(FedErrorCode::FedBadAnnouncement, message);
        let announcement: Announcement = serde_json::from_slice(&raw)
            .map_err(|error| bad(format!("malformed announcement: {error}")))?;
        announcement
            .validate()
            .map_err(|error| bad(error.to_string()))?;

        authorize_push(&node, &who.node, &announcement.objects)?;
        let receiver = &node.document.node_actor;

        let entry = InboxAnnouncement {
            from: who.node.clone(),
            to: receiver.clone(),
            base_url: announcement.base_url,
            objects: announcement.objects,
            received_at: kairo_core::Timestamp::now(),
        };
        let token: String = who.nonce.iter().map(|byte| format!("{byte:02x}")).collect();
        match node
            .store
            .put_inbox_announcement(&entry, &token, node.federation.limits.max_inbox_entries())
            .map_err(|e| Fail::store(&e))?
        {
            InboxPut::Stored { name } => Ok(Ok(name)),
            InboxPut::Full { held } => Ok(Err(held)),
        }
    })
    .await;
    match result {
        Ok(Ok(entry)) => {
            let body = AnnouncementAccepted {
                schema: Default::default(),
                entry,
            };
            match serde_json::to_vec(&body) {
                Ok(bytes) => bytes_response(StatusCode::ACCEPTED, JSON, bytes),
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        }
        Ok(Err(held)) => {
            let mut response = Fail::new(
                FedErrorCode::FedBusy,
                format!("{held} announcements from this node are waiting to be received"),
            )
            .into_response();
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from(INBOX_RETRY_AFTER_SECS),
            );
            response
        }
        Err(fail) => fail.into_response(),
    }
}

/// `Retry-After` when a node's inbox share is full: receiving runs on
/// an operator's schedule, so this is a slow retry.
const INBOX_RETRY_AFTER_SECS: u64 = 60;

/// A node's `KairoNode1` failure as a response; store failures stay
/// generic.
fn auth_failure(error: &crate::NodeAuthError) -> Fail {
    if matches!(error, crate::NodeAuthError::Store(_)) {
        tracing::error!(%error, "store read failed while authenticating a request");
        return Fail::new(FedErrorCode::FedStoreError, "store read failed");
    }
    Fail::new(error.code(), error.to_string())
}

/// §26 sub-decisions 1–2: this node's actor trusts `from` with
/// `federation:push`, and an accept entry covers every object for it.
fn authorize_push(node: &Node, from: &ActorId, objects: &[ObjectId]) -> Result<(), Fail> {
    let receiver = &node.document.node_actor;
    let trusted = node
        .store
        .latest_trust(receiver, from, &TrustScope::federation_push())
        .map_err(|e| Fail::store(&e))?
        .is_some_and(|signed| signed.unsigned().body().decision() == Some(TrustDecision::Trusted));
    if !trusted {
        return Err(Fail::new(
            FedErrorCode::FedNotTrusted,
            format!(
                "node actor {receiver} does not trust {from} with {}",
                TrustScope::federation_push()
            ),
        ));
    }
    let refused: Vec<String> = objects
        .iter()
        .filter(|object| node.federation.accepts(object, from).is_none())
        .map(ToString::to_string)
        .collect();
    if !refused.is_empty() {
        return Err(Fail::new(
            FedErrorCode::FedNotAccepted,
            format!(
                "this node does not accept pushes from {from} for {}",
                refused.join(", ")
            ),
        ));
    }
    Ok(())
}

fn request_target(uri: &axum::http::Uri) -> String {
    uri.path_and_query()
        .map_or_else(|| uri.path().to_owned(), |pq| pq.as_str().to_owned())
}

fn authorization(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn inbox_full(held: u64, what: &str) -> Response {
    let mut response = Fail::new(
        FedErrorCode::FedBusy,
        format!("{held} {what} from this node are waiting to be received"),
    )
    .into_response();
    response.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from(INBOX_RETRY_AFTER_SECS),
    );
    response
}

// --- push (DECISIONS.md §26 sub-decisions 6 and 8) --------------------

/// `POST /federation/v1/push/plan`: what of the pusher's inventory this
/// node lacks, and which of its commits it has.
async fn push_plan(
    State(node): State<Node>,
    headers: HeaderMap,
    uri: axum::http::Uri,
    body: Body,
) -> Response {
    let bad = |message: String| Fail::new(FedErrorCode::FedBadPush, message);
    let Ok(raw) = axum::body::to_bytes(body, MAX_PLAN_BYTES).await else {
        return bad(format!("plan request over {MAX_PLAN_BYTES} bytes")).into_response();
    };
    let authorization = authorization(&headers);
    let target = request_target(&uri);
    blocking(move || {
        let who = node
            .authenticate_request("POST", &target, authorization.as_deref(), &raw)
            .map_err(|error| auth_failure(&error))?;
        let request: PushPlanRequest = serde_json::from_slice(&raw)
            .map_err(|error| bad(format!("malformed plan request: {error}")))?;
        request
            .inventory
            .validate()
            .map_err(|error| bad(error.to_string()))?;
        authorize_push(&node, &who.node, std::slice::from_ref(&request.object))?;
        let plan = plan_for(&node, &request).map_err(|fail| {
            fail.unwrap_or_else(|| {
                bad("the inventory names a commit that isn't a git: revision".to_owned())
            })
        })?;
        json_response(&plan)
    })
    .await
}

/// `Err(None)` is a malformed commit in the inventory.
fn plan_for(node: &Node, request: &PushPlanRequest) -> Result<PushPlan, Option<Fail>> {
    let store = &node.store;
    let lacks = |result: Result<(), StoreError>| match result {
        Ok(()) => Ok(false),
        Err(StoreError::Missing) => Ok(true),
        Err(error) => Err(Some(Fail::store(&error))),
    };
    let inventory = &request.inventory;
    let mut plan = PushPlan {
        genesis: lacks(store.get_object_genesis_bytes(&request.object).map(drop))?,
        ..PushPlan::default()
    };
    for actor in &inventory.actors {
        if lacks(store.get_actor(&actor.actor).map(drop))? {
            plan.actors.push(actor.actor.clone());
        }
    }
    for id in inventory.statement_ids() {
        if lacks(store.get_statement_bytes(id).map(drop))? {
            plan.statements.push(id.clone());
        }
    }
    for id in &inventory.blobs {
        if lacks(store.open_blob(id).map(drop))? {
            plan.blobs.push(id.clone());
        }
    }
    let mut commits = Vec::new();
    for commit in &inventory.commits {
        match kairo_statement::GitRevision::parse(commit) {
            Ok(Some(revision)) => commits.push(revision),
            _ => return Err(None),
        }
    }
    if !commits.is_empty() {
        let cache = kairo_git::GitCache::open(crate::git_root(store)).map_err(|error| {
            tracing::error!(%error, "could not open the Git cache for a push plan");
            Some(Fail::new(FedErrorCode::FedStoreError, "store read failed"))
        })?;
        let missing: BTreeSet<String> = cache
            .missing_objects(&commits)
            .map_err(|error| {
                tracing::error!(%error, "could not check commits for a push plan");
                Some(Fail::new(FedErrorCode::FedStoreError, "store read failed"))
            })?
            .into_iter()
            .map(ToString::to_string)
            .collect();
        plan.have_commits = commits
            .iter()
            .map(ToString::to_string)
            .filter(|commit| !missing.contains(commit))
            .collect();
    }
    Ok(plan)
}

/// `POST /federation/v1/push?object=<id>`: stream a closure bundle into
/// the inbox. Every check but the signature runs before the body is
/// read; the signature over the body's SHA-256 runs after, and a body
/// that fails it is deleted.
async fn push_upload(
    State(node): State<Node>,
    headers: HeaderMap,
    uri: axum::http::Uri,
    query: RawQuery,
    body: Body,
) -> Response {
    let object = match query {
        Ok(Query(query)) => query
            .get("object")
            .and_then(|raw| ObjectId::new(raw.clone()).ok()),
        Err(_) => None,
    };
    let Some(object) = object else {
        return Fail::new(
            FedErrorCode::FedBadPush,
            "an upload names ?object=<object id>",
        )
        .into_response();
    };
    let authorization = authorization(&headers);
    let target = request_target(&uri);
    let max_bytes = node.federation.limits.max_push_bytes();
    let max_entries = node.federation.limits.max_inbox_entries();

    let checking = node.clone();
    let checked_object = object.clone();
    let prechecked = run_blocking(move || {
        let pending = checking
            .precheck_request(authorization.as_deref())
            .map_err(|error| auth_failure(&error))?;
        authorize_push(
            &checking,
            pending.claimed_node(),
            std::slice::from_ref(&checked_object),
        )?;
        let held = checking
            .store
            .inbox_held(pending.claimed_node())
            .map_err(|e| Fail::store(&e))?;
        let token: String = pending.nonce().iter().map(|b| format!("{b:02x}")).collect();
        let incoming = checking
            .store
            .inbox_incoming_path(&token)
            .map_err(|e| Fail::store(&e))?;
        Ok((pending, held, token, incoming))
    })
    .await;
    let (pending, held, token, incoming) = match prechecked {
        Ok(checked) => checked,
        Err(fail) => return fail.into_response(),
    };
    if held >= max_entries {
        return inbox_full(held, "pushes and announcements");
    }

    let digest = match receive_body(body, &incoming, max_bytes).await {
        Ok(digest) => digest,
        Err(fail) => {
            let _ = tokio::fs::remove_file(&incoming).await;
            return fail.into_response();
        }
    };

    let result = run_blocking(move || {
        let who = match node.finish_request(pending, "POST", &target, RequestBody::Sha256(digest)) {
            Ok(who) => who,
            Err(error) => {
                let _ = std::fs::remove_file(&incoming);
                return Err(auth_failure(&error));
            }
        };
        let upload = InboxUpload {
            from: who.node,
            to: node.document.node_actor.clone(),
            object,
            received_at: kairo_core::Timestamp::now(),
        };
        node.store
            .put_inbox_upload(&upload, &incoming, &token, max_entries)
            .map_err(|e| Fail::store(&e))
    })
    .await;
    match result {
        Ok(InboxPut::Stored { name }) => {
            let body = PushAccepted {
                schema: Default::default(),
                entry: name,
            };
            match serde_json::to_vec(&body) {
                Ok(bytes) => bytes_response(StatusCode::ACCEPTED, JSON, bytes),
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        }
        Ok(InboxPut::Full { held }) => inbox_full(held, "pushes and announcements"),
        Err(fail) => fail.into_response(),
    }
}

/// Stream `body` to `path`, at most `max_bytes`, returning its SHA-256.
async fn receive_body(
    mut body: Body,
    path: &std::path::Path,
    max_bytes: u64,
) -> Result<[u8; 32], Fail> {
    use http_body::Body as _;
    use sha2::{Digest, Sha256};
    use tokio::io::AsyncWriteExt;

    let write_failed = |error: std::io::Error| {
        tracing::error!(%error, path = %path.display(), "could not write an upload to the inbox");
        Fail::new(FedErrorCode::FedStoreError, "store write failed")
    };
    let mut file = tokio::fs::File::create(path).await.map_err(write_failed)?;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    while let Some(frame) =
        std::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx)).await
    {
        let frame = frame.map_err(|error| {
            Fail::new(
                FedErrorCode::FedBadPush,
                format!("upload body failed: {error}"),
            )
        })?;
        let Ok(data) = frame.into_data() else {
            continue;
        };
        total += data.len() as u64;
        if total > max_bytes {
            return Err(Fail::new(
                FedErrorCode::FedTooLarge,
                format!("upload over this node's max_push_bytes ({max_bytes})"),
            ));
        }
        hasher.update(&data);
        file.write_all(&data).await.map_err(write_failed)?;
    }
    file.flush().await.map_err(write_failed)?;
    Ok(hasher.finalize().into())
}
