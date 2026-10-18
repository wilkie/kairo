use kairo_core::{ActorId, BlobId, ObjectId, StatementId, Timestamp};
use kairo_identity::json::ActorGenesisJson;
use kairo_identity::{
    ActorGenesisBody, ActorKind, KeyRevocationEntry, KeyRotationEntry, KeySurface,
    MemoryActorResolver, SecretSigningKey,
};
use serde_json::{json, Value};

use crate::{
    ActorListing, ActorListingError, Cursor, CursorError, FedErrorCode, FedErrorEnvelope,
    GitAvailability, NodeAuthHeaderError, NodeAuthorization, NodeDocument, NodeDocumentError,
    NodeRequest, ObjectListing, PageRequest, PageRequestError, PublishedObjects,
    UnsignedNodeDocument, DEFAULT_PAGE_LIMIT, MAX_CURSOR_LEN, MAX_PAGE_LIMIT, PROTOCOL_VERSION,
    SUPPORTS_OBJECT_PULL,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn genesis(kind: ActorKind, seed: u8) -> Result<ActorGenesisBody, Box<dyn std::error::Error>> {
    Ok(ActorGenesisBody::new(
        kind,
        SecretSigningKey::ed25519([seed; 32]).public_key(),
        vec![SecretSigningKey::ed25519([seed.wrapping_add(1); 32]).public_key()],
        1,
        Timestamp::from_seconds(1_700_000_000),
        [seed; 32],
    )?)
}

const SIGNED_AT: i64 = 1_700_000_100;

fn node_document() -> Result<NodeDocument, Box<dyn std::error::Error>> {
    let body = genesis(ActorKind::node(), 7)?;
    Ok(UnsignedNodeDocument {
        node_actor: body.actor_id(),
        actor_genesis: ActorGenesisJson::from_body(&body),
        base_url: "https://archive.example.org".to_owned(),
        protocol_versions: vec![PROTOCOL_VERSION.to_owned()],
        supports: vec![SUPPORTS_OBJECT_PULL.to_owned()],
    }
    .sign(
        &SecretSigningKey::ed25519([7; 32]),
        Timestamp::from_seconds(SIGNED_AT),
    ))
}

/// A resolver holding the node actor of [`node_document`].
fn node_resolver() -> Result<MemoryActorResolver, Box<dyn std::error::Error>> {
    let mut resolver = MemoryActorResolver::new();
    resolver.insert(genesis(ActorKind::node(), 7)?);
    Ok(resolver)
}

fn object_id(tag: &[u8]) -> ObjectId {
    ObjectId::from_bytes(b"kairo-federation-core/test/object", tag)
}

fn statement_id(tag: &[u8]) -> StatementId {
    StatementId::from_bytes(b"kairo-federation-core/test/statement", tag)
}

fn blob_id(tag: &[u8]) -> BlobId {
    BlobId::from_bytes(b"kairo-federation-core/test/blob", tag)
}

fn actor_id(tag: &[u8]) -> ActorId {
    ActorId::from_bytes(b"kairo-federation-core/test/actor", tag)
}

fn object_listing(git: Option<GitAvailability>) -> ObjectListing {
    ObjectListing {
        schema: Default::default(),
        object: object_id(b"o"),
        statements: vec![statement_id(b"genesis"), statement_id(b"revision")],
        authority: vec![statement_id(b"grant")],
        actors: vec![actor_id(b"alice")],
        blobs: vec![blob_id(b"manifest")],
        git,
        next_cursor: Some(Cursor::new("p2").expect("cursor")),
    }
}

fn round_trip<T>(value: &T) -> Result<(Value, T), Box<dyn std::error::Error>>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let json = serde_json::to_value(value)?;
    let back = serde_json::from_value(json.clone())?;
    Ok((json, back))
}

fn with_field(mut json: Value, key: &str, value: Value) -> Value {
    if let Value::Object(map) = &mut json {
        map.insert(key.to_owned(), value);
    }
    json
}

// --- node document --------------------------------------------------

#[test]
fn node_document_round_trips_and_validates() -> TestResult {
    let document = node_document()?;
    let (json, back) = round_trip(&document)?;
    assert_eq!(back, document);
    assert_eq!(json["schema"], "kairo.fed.node.v1");
    assert_eq!(json["node_actor"], document.node_actor.as_str());
    let genesis = back.validate()?;
    assert_eq!(genesis.actor_id(), document.node_actor);
    assert!(back.supports(SUPPORTS_OBJECT_PULL));
    assert!(!back.supports("token-indexes"));
    Ok(())
}

#[test]
fn a_signed_node_document_verifies_against_the_active_key() -> TestResult {
    let document = node_document()?;
    let json = serde_json::to_value(&document)?;
    assert_eq!(json["signed_at"], "2023-11-14T22:15:00Z");
    assert_eq!(json["signature"]["algorithm"], "ed25519");
    let signed_at = document.verify_signature(&node_resolver()?)?;
    assert_eq!(signed_at, Timestamp::from_seconds(SIGNED_AT));
    Ok(())
}

#[test]
fn every_signed_field_is_covered() -> TestResult {
    let resolver = node_resolver()?;
    let tampers: [fn(&mut NodeDocument); 4] = [
        |d| d.base_url = "https://elsewhere.example.org".to_owned(),
        |d| d.supports.push("statement-batch".to_owned()),
        |d| d.protocol_versions.push("kairo-fed/2".to_owned()),
        |d| d.signed_at = "2023-11-14T22:15:01Z".to_owned(),
    ];
    for tamper in tampers {
        let mut document = node_document()?;
        tamper(&mut document);
        assert!(matches!(
            document.verify_signature(&resolver),
            Err(NodeDocumentError::BadSignature)
        ));
    }
    let mut document = node_document()?;
    document.signed_at = "yesterday".to_owned();
    assert!(matches!(
        document.verify_signature(&resolver),
        Err(NodeDocumentError::InvalidSignedAt { .. })
    ));
    Ok(())
}

#[test]
fn a_node_document_signed_by_another_key_is_refused() -> TestResult {
    let mut document = node_document()?;
    document.sign(
        &SecretSigningKey::ed25519([9; 32]),
        Timestamp::from_seconds(SIGNED_AT),
    );
    assert!(matches!(
        document.verify_signature(&node_resolver()?),
        Err(NodeDocumentError::KeyMismatch { .. })
    ));
    Ok(())
}

#[test]
fn a_node_document_follows_the_node_actors_key_chain() -> TestResult {
    let document = node_document()?;
    let actor = document.node_actor.clone();
    let rotated = SecretSigningKey::ed25519([9; 32]);

    // Rotated after signing: the old key was active at signed_at.
    let mut resolver = node_resolver()?;
    resolver.insert_rotation(
        actor.clone(),
        KeyRotationEntry {
            statement_id: "rotation".to_owned(),
            next_key: rotated.public_key(),
            created_at: Timestamp::from_seconds(SIGNED_AT + 1),
            supersedes: None,
            surface: KeySurface::Operational,
        },
    );
    document.verify_signature(&resolver)?;

    // Re-signed after the rotation, it must be by the new key.
    let mut stale = document.clone();
    stale.sign(
        &SecretSigningKey::ed25519([7; 32]),
        Timestamp::from_seconds(SIGNED_AT + 2),
    );
    assert!(matches!(
        stale.verify_signature(&resolver),
        Err(NodeDocumentError::KeyMismatch { .. })
    ));
    let mut fresh = document.clone();
    fresh.sign(&rotated, Timestamp::from_seconds(SIGNED_AT + 2));
    fresh.verify_signature(&resolver)?;

    // A retroactive revocation of the old key refuses the original.
    let mut revoked = node_resolver()?;
    revoked.insert_revocation(
        actor,
        KeyRevocationEntry {
            statement_id: "revocation".to_owned(),
            revoked_key: SecretSigningKey::ed25519([7; 32]).public_key().key_id(),
            retroactive: true,
            created_at: Timestamp::from_seconds(SIGNED_AT + 5),
            surface: KeySurface::Attestation,
        },
    );
    assert!(matches!(
        document.verify_signature(&revoked),
        Err(NodeDocumentError::RevokedKey { .. })
    ));

    // An unknown node actor has no key to check against.
    assert!(matches!(
        document.verify_signature(&MemoryActorResolver::new()),
        Err(NodeDocumentError::NoActiveKey)
    ));
    Ok(())
}

#[test]
fn node_document_rejects_unknown_fields_and_wrong_schema() -> TestResult {
    let json = serde_json::to_value(node_document()?)?;
    let extra = with_field(json.clone(), "endpoints", json!({}));
    assert!(serde_json::from_value::<NodeDocument>(extra).is_err());
    let old = with_field(json, "schema", json!("kairo.fed.node.v0"));
    let error = serde_json::from_value::<NodeDocument>(old)
        .err()
        .ok_or("wrong schema accepted")?;
    assert!(error.to_string().contains("kairo.fed.node.v1"), "{error}");
    Ok(())
}

#[test]
fn node_document_rejects_malformed_actor_id_at_parse() -> TestResult {
    let json = with_field(
        serde_json::to_value(node_document()?)?,
        "node_actor",
        json!("kairo:actor:zQmNotBare"),
    );
    assert!(serde_json::from_value::<NodeDocument>(json).is_err());
    Ok(())
}

#[test]
fn node_document_validate_refuses_mismatched_actor() -> TestResult {
    let mut document = node_document()?;
    document.node_actor = genesis(ActorKind::node(), 9)?.actor_id();
    assert!(matches!(
        document.validate(),
        Err(NodeDocumentError::ActorMismatch { .. })
    ));
    Ok(())
}

#[test]
fn node_document_validate_refuses_non_node_kind() -> TestResult {
    let body = genesis(ActorKind::person(), 7)?;
    let mut document = node_document()?;
    document.node_actor = body.actor_id();
    document.actor_genesis = ActorGenesisJson::from_body(&body);
    match document.validate() {
        Err(NodeDocumentError::NotANodeActor { kind }) => assert_eq!(kind, "person"),
        other => return Err(format!("expected NotANodeActor, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn node_document_validate_refuses_unsupported_protocol() -> TestResult {
    let mut document = node_document()?;
    document.protocol_versions = vec!["kairo-fed/2".to_owned()];
    assert!(matches!(
        document.validate(),
        Err(NodeDocumentError::UnsupportedProtocol { .. })
    ));
    Ok(())
}

#[test]
fn node_document_validate_checks_base_url() -> TestResult {
    for good in [
        "https://archive.example.org",
        "https://archive.example.org:8443",
        "http://127.0.0.1:7880",
        "http://[::1]:7880",
    ] {
        let mut document = node_document()?;
        document.base_url = good.to_owned();
        assert!(document.validate().is_ok(), "{good} refused");
    }
    for bad in [
        "",
        "archive.example.org",
        "ftp://archive.example.org",
        "https://",
        "https://archive.example.org/",
        "https://archive.example.org/kairo",
        "https://archive.example.org?x=1",
        "https://user@archive.example.org",
        "https://archive .example.org",
    ] {
        let mut document = node_document()?;
        document.base_url = bad.to_owned();
        assert!(
            matches!(
                document.validate(),
                Err(NodeDocumentError::InvalidBaseUrl { .. })
            ),
            "{bad:?} accepted"
        );
    }
    Ok(())
}

// --- listings -------------------------------------------------------

#[test]
fn published_objects_round_trips() -> TestResult {
    let page = PublishedObjects {
        schema: Default::default(),
        objects: vec![object_id(b"a"), object_id(b"b")],
        next_cursor: None,
    };
    let (json, back) = round_trip(&page)?;
    assert_eq!(back, page);
    assert_eq!(json["schema"], "kairo.fed.objects.v1");
    assert_eq!(json["next_cursor"], Value::Null);
    Ok(())
}

#[test]
fn object_listing_round_trips_with_and_without_git() -> TestResult {
    let first = object_listing(Some(GitAvailability { available: true }));
    let (json, back) = round_trip(&first)?;
    assert_eq!(back, first);
    assert_eq!(json["schema"], "kairo.fed.object-listing.v1");
    assert_eq!(json["git"], json!({ "available": true }));
    assert_eq!(json["authority"][0], statement_id(b"grant").as_str());
    assert_eq!(back.len(), 5);

    let later = object_listing(None);
    let (json, back) = round_trip(&later)?;
    assert_eq!(back, later);
    assert!(json.get("git").is_none(), "git serialized on a later page");
    Ok(())
}

#[test]
fn object_listing_rejects_bad_ids_and_unknown_fields() -> TestResult {
    let json = serde_json::to_value(object_listing(None))?;
    let bad_id = with_field(json.clone(), "blobs", json!(["not-an-id"]));
    assert!(serde_json::from_value::<ObjectListing>(bad_id).is_err());
    let extra = with_field(json, "trust", json!([]));
    assert!(serde_json::from_value::<ObjectListing>(extra).is_err());
    Ok(())
}

#[test]
fn actor_listing_round_trips_and_validates() -> TestResult {
    let body = genesis(ActorKind::person(), 3)?;
    let listing = ActorListing {
        schema: Default::default(),
        actor: body.actor_id(),
        genesis: ActorGenesisJson::from_body(&body),
        key_events: vec![statement_id(b"rotation")],
    };
    let (json, back) = round_trip(&listing)?;
    assert_eq!(back, listing);
    assert_eq!(json["schema"], "kairo.fed.actor-listing.v1");
    assert_eq!(back.validate()?.actor_id(), body.actor_id());

    let forged = ActorListing {
        actor: actor_id(b"someone-else"),
        ..listing
    };
    assert!(matches!(
        forged.validate(),
        Err(ActorListingError::ActorMismatch { .. })
    ));
    Ok(())
}

// --- paging ---------------------------------------------------------

#[test]
fn cursor_accepts_url_safe_tokens_only() {
    assert!(Cursor::new("AbC-09_.~").is_ok());
    assert_eq!(Cursor::new(""), Err(CursorError::Empty));
    assert_eq!(
        Cursor::new("a".repeat(MAX_CURSOR_LEN + 1)),
        Err(CursorError::TooLong)
    );
    for bad in ["a b", "a/b", "a&b", "a=b", "é"] {
        assert_eq!(
            Cursor::new(bad),
            Err(CursorError::InvalidCharacter),
            "{bad}"
        );
    }
    assert!(serde_json::from_value::<Cursor>(json!("a/b")).is_err());
}

#[test]
fn page_request_parses_limit_and_cursor() -> TestResult {
    let first = PageRequest::from_query(None, None)?;
    assert_eq!(first, PageRequest::first());
    assert_eq!(first.limit(), DEFAULT_PAGE_LIMIT);
    assert_eq!(first.cursor(), None);

    let max = PageRequest::from_query(Some(&MAX_PAGE_LIMIT.to_string()), Some("c1"))?;
    assert_eq!(max.limit(), MAX_PAGE_LIMIT);
    assert_eq!(max.cursor().map(Cursor::as_str), Some("c1"));

    let next = first.next(Cursor::new("c2")?);
    assert_eq!(next.limit(), DEFAULT_PAGE_LIMIT);
    assert_eq!(next.cursor().map(Cursor::as_str), Some("c2"));
    Ok(())
}

#[test]
fn page_request_refuses_out_of_range_limits_and_bad_cursors() {
    let too_big = (MAX_PAGE_LIMIT + 1).to_string();
    for bad in ["0", "-1", "ten", "", too_big.as_str()] {
        assert_eq!(
            PageRequest::from_query(Some(bad), None),
            Err(PageRequestError::BadLimit),
            "{bad:?}"
        );
    }
    assert_eq!(
        PageRequest::from_query(None, Some("")),
        Err(PageRequestError::BadCursor(CursorError::Empty))
    );
}

// --- errors ---------------------------------------------------------

#[test]
fn error_envelope_matches_the_daemon_api_shape() -> TestResult {
    let envelope = FedErrorEnvelope::not_found();
    let json = serde_json::to_value(&envelope)?;
    assert_eq!(
        json,
        json!({
            "ok": false,
            "schema": "kairo.api.error.v1",
            "error": { "code": "fed_not_found", "message": "not found", "details": {} }
        })
    );
    let back: FedErrorEnvelope = serde_json::from_value(json)?;
    assert_eq!(back, envelope);
    Ok(())
}

#[test]
fn error_codes_serialize_and_map_to_statuses() -> TestResult {
    let cases = [
        (FedErrorCode::FedNotFound, 404),
        (FedErrorCode::FedBadId, 400),
        (FedErrorCode::FedBadCursor, 400),
        (FedErrorCode::FedBadLimit, 400),
        (FedErrorCode::FedMethodNotAllowed, 405),
        (FedErrorCode::FedStoreError, 500),
        (FedErrorCode::FedBusy, 503),
        (FedErrorCode::FedUnauthenticated, 401),
    ];
    for (code, status) in cases {
        assert_eq!(serde_json::to_value(code)?, json!(code.as_str()));
        assert_eq!(code.status(), status, "{code}");
    }
    Ok(())
}

#[test]
fn unknown_error_codes_from_newer_nodes_still_parse() -> TestResult {
    let json = json!({
        "ok": false,
        "schema": "kairo.api.error.v1",
        "error": { "code": "fed_rate_limited", "message": "slow down" }
    });
    let envelope: FedErrorEnvelope = serde_json::from_value(json)?;
    assert_eq!(envelope.error.code, FedErrorCode::Unknown);
    assert_eq!(envelope.error.details, json!({}));
    Ok(())
}

// --- KairoNode1 (DECISIONS.md §25 sub-decision 3) ---------------------

const REQUEST: NodeRequest<'static> = NodeRequest {
    method: "POST",
    receiver_base_url: "https://archive.example.org",
    path_and_query: "/federation/v1/announce",
    body: b"{}",
};

#[test]
fn the_node_request_signing_string_is_pinned() {
    let nonce = [0xab; 16];
    assert_eq!(
        REQUEST.signing_string(1_700_000_000, &nonce),
        "kairo-federation-request-v1\n\
         POST\n\
         https://archive.example.org\n\
         /federation/v1/announce\n\
         44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a\n\
         1700000000\n\
         abababababababababababababababab\n"
    );
}

#[test]
fn a_node_authorization_round_trips_and_verifies() -> TestResult {
    let key = SecretSigningKey::ed25519([7; 32]);
    let node = genesis(ActorKind::node(), 7)?.actor_id();
    let auth = NodeAuthorization::sign(&REQUEST, node.clone(), &key, 1_700_000_000, [1; 16]);
    let header = auth.format();
    assert!(header.starts_with(&format!("KairoNode1 node={node}, key_id=")));
    let parsed = NodeAuthorization::parse(&header)?;
    assert_eq!(parsed, auth);
    let signing = REQUEST.signing_string(parsed.timestamp, &parsed.nonce);
    kairo_identity::verify_signature(
        &key.public_key(),
        signing.as_bytes(),
        &kairo_identity::SignatureBytes::ed25519(parsed.signature),
    )?;
    Ok(())
}

#[test]
fn malformed_node_authorizations_are_refused() -> TestResult {
    let key = SecretSigningKey::ed25519([7; 32]);
    let node = genesis(ActorKind::node(), 7)?.actor_id();
    let good = NodeAuthorization::sign(&REQUEST, node, &key, 1_700_000_000, [0xab; 16]).format();
    assert!(matches!(
        NodeAuthorization::parse(&good.replacen("KairoNode1", "KairoActor1", 1)),
        Err(NodeAuthHeaderError::UnsupportedScheme(_))
    ));
    let nonce = "ab".repeat(16);
    for bad in [
        "KairoNode1".to_owned(),
        good.replacen("node=", "actor=", 1),
        format!("{good}, node=x"),
        good.replacen(", timestamp=1700000000", "", 1),
        good.replacen("timestamp=1700000000", "timestamp=soon", 1),
        good.replacen(&nonce, &nonce.to_uppercase(), 1),
        good.replacen(&nonce, "abab", 1),
        good.replacen("signature=", "signature=AA", 1),
    ] {
        assert!(
            matches!(
                NodeAuthorization::parse(&bad),
                Err(NodeAuthHeaderError::Malformed(_))
            ),
            "{bad}"
        );
    }
    Ok(())
}
