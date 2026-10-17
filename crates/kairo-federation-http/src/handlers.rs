//! Request handlers for `specs/PHASE_2_FEDERATION.md` §2.1.
//!
//! Every per-record endpoint answers from the published view: an id
//! outside every published closure gets the same `404 fed_not_found`
//! as an id the store doesn't hold (§16 sub-decision 3). Store reads
//! are blocking filesystem I/O, so they run on the blocking pool.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use kairo_core::{ActorId, BlobId, ObjectId, StatementId};
use kairo_federation_core::{
    ActorListing, Cursor, FedErrorCode, FedErrorEnvelope, GitAvailability, ObjectListing,
    PageRequest, PageRequestError, PublishedObjects, API_PREFIX, WELL_KNOWN_PATH,
};
use kairo_identity::json::ActorGenesisJson;
use kairo_store::{ActorStore, BlobStore, StoreError};
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
        .route(&format!("{API_PREFIX}/objects/:id/git"), get(object_git))
        .route(&format!("{API_PREFIX}/actors/:id"), get(actor_listing))
        .route(&format!("{API_PREFIX}/statements/:id"), get(statement))
        .route(&format!("{API_PREFIX}/blobs/:id"), get(blob))
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

/// Only `GET` and `HEAD` exist; everything else is `405` with the
/// federation envelope, on every path including unknown ones.
async fn read_only(request: Request, next: Next) -> Response {
    if request.method() == Method::GET || request.method() == Method::HEAD {
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
                available: crate::pack::has_pack(&crate::git_root(&node.store), object.as_str()),
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

/// The object's Git pack, streamed from the managed cache. `404` when
/// the object isn't published or the cache holds nothing for it.
async fn object_git(State(node): State<Node>, id: PathId) -> Response {
    let object = match parse_id(id, ObjectId::new) {
        Ok(object) => object,
        Err(fail) => return fail.into_response(),
    };
    let check_node = node.clone();
    let check_object = object.clone();
    let available = run_blocking(move || {
        let published = view(&check_node)?.closures.contains_key(&check_object);
        let cached =
            crate::pack::has_pack(&crate::git_root(&check_node.store), check_object.as_str());
        if published && cached {
            Ok(())
        } else {
            Err(Fail::not_found())
        }
    })
    .await;
    if let Err(fail) = available {
        return fail.into_response();
    }
    let body = crate::pack::pack_body(&crate::git_root(&node.store), object.as_str());
    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-git-packed-objects"),
    );
    response
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
