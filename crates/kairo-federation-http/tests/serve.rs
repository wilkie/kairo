//! Router-level tests for the federation serving side
//! (`specs/PHASE_2_FEDERATION.md` slice 2). They drive `Node::router`
//! directly against a seeded tempdir store.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use kairo_core::{ObjectId, StatementId, Timestamp};
use kairo_federation_core::{
    ActorListing, FedErrorCode, FedErrorEnvelope, NodeDocument, ObjectListing, PublishedObjects,
};
use kairo_federation_http::{Error, Node};
use kairo_identity::ActorKind;
use kairo_store::PublishedObjectStore;
use kairo_test_support::store::StoreFixture;
use tower::ServiceExt;

use common::{node_actor, seed, Seeded, BASE_URL};

// Crates the lib or `common` depends on that this target doesn't name.
use http_body as _;
use kairo_federation_core as _;
use kairo_git as _;
use kairo_object as _;
use serde as _;
use serde_json as _;
use tempfile as _;
use tracing as _;
use tracing_subscriber as _;
use ureq as _;

fn router(seeded: &Seeded) -> Router {
    common::node(seeded).router()
}

async fn call(router: &Router, method: Method, uri: &str) -> (StatusCode, String, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, content_type, body.to_vec())
}

async fn get_json<T: serde::de::DeserializeOwned>(router: &Router, uri: &str) -> T {
    let (status, content_type, body) = call(router, Method::GET, uri).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{uri}: {}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(content_type, "application/json", "{uri}");
    serde_json::from_slice(&body).unwrap_or_else(|e| panic!("{uri}: {e}"))
}

async fn error_code(
    router: &Router,
    method: Method,
    uri: &str,
) -> (StatusCode, FedErrorCode, Vec<u8>) {
    let (status, _, body) = call(router, method, uri).await;
    let envelope: FedErrorEnvelope =
        serde_json::from_slice(&body).unwrap_or_else(|e| panic!("{uri}: not an envelope: {e}"));
    assert!(!envelope.ok);
    (status, envelope.error.code, body)
}

/// Every page of an object listing, merged.
async fn full_listing(router: &Router, object: &ObjectId, limit: u32) -> (ObjectListing, usize) {
    let mut uri = format!("/federation/v1/objects/{object}?limit={limit}");
    let mut merged: Option<ObjectListing> = None;
    let mut pages = 0;
    loop {
        let page: ObjectListing = get_json(router, &uri).await;
        pages += 1;
        assert!(page.len() <= limit as usize);
        if pages > 1 {
            assert!(page.git.is_none(), "git repeated on page {pages}");
        }
        let next = page.next_cursor.clone();
        match merged.as_mut() {
            None => merged = Some(page),
            Some(all) => {
                all.statements.extend(page.statements);
                all.authority.extend(page.authority);
                all.actors.extend(page.actors);
                all.blobs.extend(page.blobs);
            }
        }
        match next {
            Some(cursor) => {
                uri = format!("/federation/v1/objects/{object}?limit={limit}&cursor={cursor}")
            }
            None => break,
        }
    }
    (merged.expect("one page"), pages)
}

// --- node document ----------------------------------------------------

#[tokio::test]
async fn serves_a_valid_node_document() {
    let seeded = seed();
    let document: NodeDocument = get_json(&router(&seeded), "/.well-known/kairo-node").await;
    assert_eq!(document.node_actor, seeded.node_actor);
    assert_eq!(document.base_url, BASE_URL);
    assert!(document.supports("object-pull"));
    document.validate().expect("document validates");
}

#[test]
fn refuses_a_node_actor_that_is_not_kind_node() {
    let (_dir, fixture) = StoreFixture::temp();
    let person = node_actor(&fixture.store, ActorKind::person());
    let result = Node::from_store(fixture.store.clone(), &person, BASE_URL, Duration::ZERO);
    assert!(matches!(result, Err(Error::NotANodeActor { ref kind, .. }) if kind == "person"));
}

#[test]
fn refuses_a_missing_node_actor_and_a_bad_base_url() {
    let (_dir, fixture) = StoreFixture::temp();
    let absent = fixture.make_actor().actor_id;
    let other = StoreFixture::temp().1;
    let result = Node::from_store(other.store.clone(), &absent, BASE_URL, Duration::ZERO);
    assert!(matches!(result, Err(Error::NodeActorMissing { .. })));

    let node = node_actor(&fixture.store, ActorKind::node());
    for bad in ["https://archive.example.org/", "archive.example.org"] {
        let result = Node::from_store(fixture.store.clone(), &node, bad, Duration::ZERO);
        assert!(matches!(result, Err(Error::NodeDocument(_))), "{bad}");
    }
}

// --- listings ---------------------------------------------------------

#[tokio::test]
async fn lists_only_published_objects() {
    let seeded = seed();
    let page: PublishedObjects = get_json(&router(&seeded), "/federation/v1/objects").await;
    assert_eq!(page.objects, vec![seeded.published.clone()]);
    assert!(page.next_cursor.is_none());
}

#[tokio::test]
async fn published_objects_page_by_id() {
    let seeded = seed();
    seeded
        .store
        .publish_object(&seeded.unpublished, Timestamp::from_seconds(1_700_002_000))
        .expect("publish second");
    let router = router(&seeded);

    let first: PublishedObjects = get_json(&router, "/federation/v1/objects?limit=1").await;
    assert_eq!(first.objects.len(), 1);
    let cursor = first.next_cursor.clone().expect("a second page");
    let second: PublishedObjects = get_json(
        &router,
        &format!("/federation/v1/objects?limit=1&cursor={cursor}"),
    )
    .await;
    assert_eq!(second.objects.len(), 1);
    assert!(second.next_cursor.is_none());

    let mut all = vec![first.objects[0].clone(), second.objects[0].clone()];
    let mut expected = vec![seeded.published.clone(), seeded.unpublished.clone()];
    expected.sort();
    assert_eq!(all, expected, "pages are in id order");
    all.dedup();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn object_listing_is_the_published_closure() {
    let seeded = seed();
    let listing: ObjectListing = get_json(
        &router(&seeded),
        &format!("/federation/v1/objects/{}", seeded.published),
    )
    .await;

    assert_eq!(listing.object, seeded.published);
    assert_eq!(
        listing.statements.iter().cloned().collect::<BTreeSet<_>>(),
        seeded.statements
    );
    assert_eq!(
        listing.authority.iter().cloned().collect::<BTreeSet<_>>(),
        seeded.authority
    );
    assert!(!listing.statements.contains(&seeded.trust));
    assert!(!listing.authority.contains(&seeded.trust));
    for other in &seeded.unpublished_statements {
        assert!(!listing.statements.contains(other) && !listing.authority.contains(other));
    }

    // bob signed a branch, so he's listed; carol signed nothing and
    // isn't. Key events are per actor, not in the listing.
    let actors: BTreeSet<_> = listing.actors.iter().cloned().collect();
    assert_eq!(actors, [seeded.alice.clone(), seeded.bob.clone()].into());
    assert!(!listing.statements.contains(&seeded.alice_rotation));

    assert_eq!(listing.blobs, vec![seeded.manifest.clone()]);
    assert_eq!(listing.git.map(|git| git.available), Some(false));
    assert!(listing.next_cursor.is_none());
    for list in [&listing.statements, &listing.authority] {
        assert!(list.windows(2).all(|pair| pair[0] < pair[1]), "sorted");
    }
}

#[tokio::test]
async fn object_listing_pages_reassemble_to_the_whole() {
    let seeded = seed();
    let router = router(&seeded);
    let (whole, pages) = full_listing(&router, &seeded.published, 1000).await;
    assert_eq!(pages, 1);
    for limit in [1, 2, 3] {
        let (paged, pages) = full_listing(&router, &seeded.published, limit).await;
        assert_eq!(paged.statements, whole.statements, "limit {limit}");
        assert_eq!(paged.authority, whole.authority, "limit {limit}");
        assert_eq!(paged.actors, whole.actors, "limit {limit}");
        assert_eq!(paged.blobs, whole.blobs, "limit {limit}");
        assert_eq!(pages, whole.len().div_ceil(limit as usize), "limit {limit}");
    }
}

#[tokio::test]
async fn actor_listing_serves_key_events() {
    let seeded = seed();
    let router = router(&seeded);
    let listing: ActorListing =
        get_json(&router, &format!("/federation/v1/actors/{}", seeded.alice)).await;
    listing.validate().expect("genesis derives the id");
    assert_eq!(listing.key_events, vec![seeded.alice_rotation.clone()]);

    let bob: ActorListing =
        get_json(&router, &format!("/federation/v1/actors/{}", seeded.bob)).await;
    assert!(bob.key_events.is_empty());

    // The rotation is servable as a statement because alice is in a
    // published closure.
    let (status, _, _) = call(
        &router,
        Method::GET,
        &format!("/federation/v1/statements/{}", seeded.alice_rotation),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // carol isn't in any published closure.
    let (status, code, _) = error_code(
        &router,
        Method::GET,
        &format!("/federation/v1/actors/{}", seeded.carol),
    )
    .await;
    assert_eq!(
        (status, code),
        (StatusCode::NOT_FOUND, FedErrorCode::FedNotFound)
    );
}

// --- records ----------------------------------------------------------

#[tokio::test]
async fn statements_are_served_byte_for_byte() {
    let seeded = seed();
    let router = router(&seeded);
    for id in seeded.statements.iter().chain(&seeded.authority) {
        let (status, content_type, body) = call(
            &router,
            Method::GET,
            &format!("/federation/v1/statements/{id}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{id}");
        assert_eq!(content_type, "application/json");
        assert_eq!(
            body,
            seeded.store.get_statement_bytes(id).expect("stored"),
            "{id}"
        );
    }
}

#[tokio::test]
async fn unpublished_and_missing_records_are_indistinguishable() {
    let seeded = seed();
    let router = router(&seeded);
    let missing = StatementId::from_bytes(b"test", b"never stored");
    let (status, code, missing_body) = error_code(
        &router,
        Method::GET,
        &format!("/federation/v1/statements/{missing}"),
    )
    .await;
    assert_eq!(
        (status, code),
        (StatusCode::NOT_FOUND, FedErrorCode::FedNotFound)
    );

    let held_but_private = seeded
        .unpublished_statements
        .iter()
        .chain(std::iter::once(&seeded.trust));
    for id in held_but_private {
        assert!(
            seeded.store.get_statement_bytes(id).is_ok(),
            "fixture stores {id}"
        );
        let (status, _, body) = call(
            &router,
            Method::GET,
            &format!("/federation/v1/statements/{id}"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{id}");
        assert_eq!(
            body, missing_body,
            "{id} distinguishable from a missing record"
        );
    }

    for uri in [
        format!("/federation/v1/objects/{}", seeded.unpublished),
        format!("/federation/v1/objects/{}/genesis", seeded.unpublished),
    ] {
        let (status, _, body) = call(&router, Method::GET, &uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(body, missing_body, "{uri}");
    }
}

#[tokio::test]
async fn genesis_and_blobs_are_served_raw() {
    let seeded = seed();
    let router = router(&seeded);
    let (status, content_type, body) = call(
        &router,
        Method::GET,
        &format!("/federation/v1/objects/{}/genesis", seeded.published),
    )
    .await;
    assert_eq!(
        (status, content_type.as_str()),
        (StatusCode::OK, "application/json")
    );
    assert_eq!(
        body,
        seeded
            .store
            .get_object_genesis_bytes(&seeded.published)
            .expect("stored")
    );

    let (status, content_type, body) = call(
        &router,
        Method::GET,
        &format!("/federation/v1/blobs/{}", seeded.manifest),
    )
    .await;
    assert_eq!(
        (status, content_type.as_str()),
        (StatusCode::OK, "application/octet-stream")
    );
    assert_eq!(
        body,
        kairo_store::BlobStore::get_blob(&seeded.store, &seeded.manifest).expect("stored")
    );
}

#[tokio::test]
async fn unpublishing_stops_serving() {
    let seeded = seed();
    let router = router(&seeded);
    let revision = seeded
        .statements
        .iter()
        .next()
        .expect("a statement")
        .clone();
    let uri = format!("/federation/v1/statements/{revision}");
    assert_eq!(call(&router, Method::GET, &uri).await.0, StatusCode::OK);

    seeded
        .store
        .unpublish_object(&seeded.published)
        .expect("unpublish");
    // The router was built with a zero view TTL, so the next request
    // sees the change.
    assert_eq!(
        call(&router, Method::GET, &uri).await.0,
        StatusCode::NOT_FOUND
    );
    let page: PublishedObjects = get_json(&router, "/federation/v1/objects").await;
    assert!(page.objects.is_empty());
}

// --- request errors ---------------------------------------------------

#[tokio::test]
async fn malformed_requests_get_federation_errors() {
    let seeded = seed();
    let router = router(&seeded);
    let object = &seeded.published;
    let cases = [
        (
            "/federation/v1/statements/not-an-id".to_owned(),
            StatusCode::BAD_REQUEST,
            FedErrorCode::FedBadId,
        ),
        (
            "/federation/v1/objects/kairo:object:x".to_owned(),
            StatusCode::BAD_REQUEST,
            FedErrorCode::FedBadId,
        ),
        (
            format!("/federation/v1/objects/{object}?limit=0"),
            StatusCode::BAD_REQUEST,
            FedErrorCode::FedBadLimit,
        ),
        (
            format!("/federation/v1/objects/{object}?limit=1001"),
            StatusCode::BAD_REQUEST,
            FedErrorCode::FedBadLimit,
        ),
        (
            format!("/federation/v1/objects/{object}?cursor=zz.abc"),
            StatusCode::BAD_REQUEST,
            FedErrorCode::FedBadCursor,
        ),
        (
            format!("/federation/v1/objects/{object}?cursor=st."),
            StatusCode::BAD_REQUEST,
            FedErrorCode::FedBadCursor,
        ),
        (
            "/federation/v1/objects?cursor=a%2Fb".to_owned(),
            StatusCode::BAD_REQUEST,
            FedErrorCode::FedBadCursor,
        ),
        (
            "/federation/v1/nope".to_owned(),
            StatusCode::NOT_FOUND,
            FedErrorCode::FedNotFound,
        ),
        (
            "/api/v1/status".to_owned(),
            StatusCode::NOT_FOUND,
            FedErrorCode::FedNotFound,
        ),
    ];
    for (uri, status, code) in cases {
        let (got_status, got_code, _) = error_code(&router, Method::GET, &uri).await;
        assert_eq!((got_status, got_code), (status, code), "{uri}");
    }
}

#[tokio::test]
async fn writes_are_method_not_allowed_everywhere() {
    let seeded = seed();
    let router = router(&seeded);
    for (method, uri) in [
        (Method::POST, "/federation/v1/objects".to_owned()),
        (
            Method::PUT,
            format!("/federation/v1/statements/{}", seeded.alice_rotation),
        ),
        (
            Method::DELETE,
            format!("/federation/v1/objects/{}", seeded.published),
        ),
        (Method::POST, "/federation/v1/announce".to_owned()),
        (Method::OPTIONS, "/.well-known/kairo-node".to_owned()),
    ] {
        let request = Request::builder()
            .method(method.clone())
            .uri(&uri)
            .body(Body::empty())
            .expect("request");
        let response = router.clone().oneshot(request).await.expect("response");
        assert_eq!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} {uri}"
        );
        assert_eq!(
            response
                .headers()
                .get(header::ALLOW)
                .and_then(|v| v.to_str().ok()),
            Some("GET, HEAD"),
            "{method} {uri}"
        );
    }
}

#[tokio::test]
async fn head_is_allowed() {
    let seeded = seed();
    let (status, _, body) = call(&router(&seeded), Method::HEAD, "/.well-known/kairo-node").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_empty());
}

// --- process ----------------------------------------------------------

#[tokio::test]
async fn serve_binds_and_shuts_down() {
    let seeded = seed();
    let config = kairo_federation_http::Config {
        store: seeded.store.root().to_path_buf(),
        bind_addr: "127.0.0.1:0".parse().expect("addr"),
        base_url: BASE_URL.to_owned(),
        node_actor: seeded.node_actor.clone(),
        view_ttl: Duration::ZERO,
        max_concurrent_packs: Some(1),
    };
    kairo_federation_http::serve_with_shutdown(config, async {})
        .await
        .expect("serve exits cleanly");
}
