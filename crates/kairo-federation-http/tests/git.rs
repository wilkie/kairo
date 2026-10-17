//! Git pack transfer (`specs/PHASE_2_FEDERATION.md` slice 4): the
//! server streams an object's pack from the managed cache, and a pull
//! ingests it so every pulled revision's commit resolves locally.
//! Every test needs the host's `git` binary and returns early without
//! one, like the other cache tests.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use kairo_core::{ObjectId, StatementId, Timestamp};
use kairo_federation_core::ObjectListing;
use kairo_federation_http::client::{pull_object, GitOutcome, Limits, PullError, PullOptions};
use kairo_git::GitCache;
use kairo_statement::{GitObjectFormat, GitRevision};
use kairo_store::{FilesystemStore, PublishedObjectStore};
use kairo_test_support::git::{
    build_pack_from, init_git_repo_with_manifest, init_git_repo_with_manifest_in, skip_if_no_git,
};
use kairo_test_support::store::StoreFixture;
use tempfile::TempDir;

use common::{node_actor, Served, BASE_URL, MANIFEST};

// Crates the lib or `common` depends on that this target doesn't name.
use axum as _;
use http_body as _;
use http_body_util as _;
use kairo_federation_core as _;
use kairo_identity as _;
use kairo_object as _;
use serde as _;
use serde_json as _;
use tokio as _;
use tower as _;
use tracing as _;
use tracing_subscriber as _;
use ureq as _;

/// An origin whose one revision names a real commit, with that commit
/// in the origin's managed cache when `cached`.
struct Origin {
    _dir: TempDir,
    _repo: TempDir,
    store: FilesystemStore,
    node_actor: kairo_core::ActorId,
    object: ObjectId,
    revision: StatementId,
    commit: GitRevision,
}

fn origin(cached: bool) -> Origin {
    origin_in(cached, GitObjectFormat::Sha1)
}

fn origin_in(cached: bool, format: GitObjectFormat) -> Origin {
    let (repo, oid) = init_git_repo_with_manifest_in(MANIFEST, format.as_str()).expect("git repo");
    let commit = GitRevision::new(format, &oid).expect("oid in format");
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let revision = fixture.make_revision(
        &alice,
        &object,
        commit.to_revision_id(),
        MANIFEST,
        Vec::new(),
    );
    fixture.set_branch(&alice, &object, &revision, "main");
    if cached {
        let cache = GitCache::open(fixture.store.root().join("git")).expect("cache");
        cache
            .ingest_pack(format, &build_pack_from(repo.path()))
            .expect("ingest");
        cache
            .set_ref(object.object_id.as_str(), "refs/heads/main", &commit)
            .expect("ref");
    }
    let store = fixture.store.clone();
    let node_actor = node_actor(&store, kairo_identity::ActorKind::node());
    store
        .publish_object(&object.object_id, Timestamp::from_seconds(1_700_001_000))
        .expect("publish");
    Origin {
        _dir: dir,
        _repo: repo,
        store,
        node_actor,
        object: object.object_id,
        revision: revision.statement_id,
        commit,
    }
}

fn serve(origin: &Origin) -> Served {
    Served::spawn(
        kairo_federation_http::Node::from_store(
            origin.store.clone(),
            &origin.node_actor,
            BASE_URL,
            std::time::Duration::ZERO,
        )
        .expect("node")
        .router(),
    )
}

fn local_store() -> (TempDir, FilesystemStore) {
    let dir = TempDir::new().expect("tempdir");
    let store = FilesystemStore::open(dir.path()).expect("open");
    (dir, store)
}

fn local_has_commit(store: &FilesystemStore, object: &ObjectId, commit: &GitRevision) -> bool {
    GitCache::open(store.root().join("git"))
        .expect("cache")
        .has_commit(object.as_str(), commit)
        .expect("has_commit")
}

fn listing(served: &Served, object: &ObjectId) -> ObjectListing {
    let body = ureq::get(format!(
        "{}/federation/v1/objects/{object}",
        served.base_url
    ))
    .call()
    .expect("listing")
    .body_mut()
    .read_to_vec()
    .expect("body");
    serde_json::from_slice(&body).expect("listing json")
}

#[test]
fn pull_ingests_the_pack_and_pins_each_commit() {
    if skip_if_no_git() {
        return;
    }
    let origin = origin(true);
    let served = serve(&origin);
    assert_eq!(
        listing(&served, &origin.object).git.map(|g| g.available),
        Some(true)
    );

    let (_dir, local) = local_store();
    let report = pull_object(
        &local,
        &served.base_url,
        &origin.object,
        &PullOptions::default(),
    )
    .expect("pull");
    assert_eq!(report.git, GitOutcome::Ingested { commits: 1 });
    assert!(local_has_commit(&local, &origin.object, &origin.commit));

    // The commit is pinned in the object's own repo, so this node can
    // serve the pack onward: the object's pack is non-empty and holds
    // the commit.
    let pack = GitCache::open(local.root().join("git"))
        .expect("cache")
        .pack_for_object(origin.object.as_str())
        .expect("pack");
    assert!(pack.starts_with(b"PACK"));
    let (_relay_dir, relay) = local_store();
    GitCache::open(relay.root().join("git"))
        .expect("cache")
        .ingest_pack(GitObjectFormat::Sha1, &pack)
        .expect("ingest relayed pack");
    assert!(GitCache::open(relay.root().join("git"))
        .expect("cache")
        .set_ref(origin.object.as_str(), "refs/heads/main", &origin.commit)
        .is_ok());

    // A second pull has nothing to fetch.
    let again = pull_object(
        &local,
        &served.base_url,
        &origin.object,
        &PullOptions::default(),
    )
    .expect("re-pull");
    assert_eq!(again.git, GitOutcome::AlreadyHeld { commits: 1 });
}

#[test]
fn a_peer_without_the_pack_reports_unavailable() {
    if skip_if_no_git() {
        return;
    }
    let origin = origin(false);
    let served = serve(&origin);
    assert_eq!(
        listing(&served, &origin.object).git.map(|g| g.available),
        Some(false)
    );

    let (_dir, local) = local_store();
    let report = pull_object(
        &local,
        &served.base_url,
        &origin.object,
        &PullOptions::default(),
    )
    .expect("pull still succeeds");
    assert_eq!(report.git, GitOutcome::Unavailable { missing: 1 });
    // The signed history arrived regardless.
    assert!(local.get_statement_bytes(&origin.revision).is_ok());
    assert!(!local.root().join("git").exists(), "cache not created");

    let status = ureq::get(format!(
        "{}/federation/v1/objects/{}/git",
        served.base_url, origin.object
    ))
    .config()
    .http_status_as_error(false)
    .build()
    .call()
    .expect("request")
    .status();
    assert_eq!(status.as_u16(), 404);
}

#[test]
fn the_pack_endpoint_streams_the_cached_pack() {
    if skip_if_no_git() {
        return;
    }
    let origin = origin(true);
    let served = serve(&origin);
    let mut response = ureq::get(format!(
        "{}/federation/v1/objects/{}/git",
        served.base_url, origin.object
    ))
    .call()
    .expect("pack");
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/x-git-packed-objects")
    );
    let pack = response
        .body_mut()
        .with_config()
        .limit(u64::MAX)
        .read_to_vec()
        .expect("pack body");
    assert_eq!(
        pack,
        GitCache::open(origin.store.root().join("git"))
            .expect("cache")
            .pack_for_object(origin.object.as_str())
            .expect("origin pack")
    );

    // Unpublishing hides the pack like every other record.
    origin
        .store
        .unpublish_object(&origin.object)
        .expect("unpublish");
    let status = ureq::get(format!(
        "{}/federation/v1/objects/{}/git",
        served.base_url, origin.object
    ))
    .config()
    .http_status_as_error(false)
    .build()
    .call()
    .expect("request")
    .status();
    assert_eq!(status.as_u16(), 404);
}

#[test]
fn a_pack_missing_the_revision_commit_fails_the_pull() {
    if skip_if_no_git() {
        return;
    }
    // The origin's cache holds an unrelated repo's history for the
    // object, so its pack is valid Git but lacks the signed commit.
    let origin = origin(false);
    let (other, other_oid) =
        init_git_repo_with_manifest("[kairo]\nschema = 1\nname = \"other\"\n").expect("repo");
    let other_commit = GitRevision::new(GitObjectFormat::Sha1, &other_oid).expect("sha1 oid");
    let cache = GitCache::open(origin.store.root().join("git")).expect("cache");
    cache
        .ingest_pack(GitObjectFormat::Sha1, &build_pack_from(other.path()))
        .expect("ingest");
    cache
        .set_ref(origin.object.as_str(), "refs/heads/main", &other_commit)
        .expect("ref");
    let served = serve(&origin);

    let (_dir, local) = local_store();
    match pull_object(
        &local,
        &served.base_url,
        &origin.object,
        &PullOptions::default(),
    ) {
        Err(PullError::GitIncomplete { missing }) => {
            assert_eq!(missing, vec![origin.commit.to_string()])
        }
        other => panic!("expected GitIncomplete, got {other:?}"),
    }
    // Statements were stored before the Git step and stay.
    assert!(local.get_statement_bytes(&origin.revision).is_ok());
}

#[test]
fn an_oversize_pack_fails_the_pull() {
    if skip_if_no_git() {
        return;
    }
    let origin = origin(true);
    let served = serve(&origin);
    let (_dir, local) = local_store();
    let limits = Limits {
        max_git_bytes: 16,
        ..Limits::default()
    };
    let result = pull_object(
        &local,
        &served.base_url,
        &origin.object,
        &PullOptions {
            limits,
            ..PullOptions::default()
        },
    );
    assert!(matches!(result, Err(PullError::Git(_))), "{result:?}");
    assert!(!local_has_commit(&local, &origin.object, &origin.commit));
    // The signed history before the Git step stays.
    assert!(local.get_statement_bytes(&origin.revision).is_ok());
}

#[test]
fn sha256_history_federates_into_its_own_pool() {
    if skip_if_no_git() {
        return;
    }
    let origin = origin_in(true, GitObjectFormat::Sha256);
    assert_eq!(origin.commit.oid().len(), 64);
    let served = serve(&origin);
    let (_dir, local) = local_store();
    let report = pull_object(
        &local,
        &served.base_url,
        &origin.object,
        &PullOptions::default(),
    )
    .expect("pull");
    assert_eq!(report.git, GitOutcome::Ingested { commits: 1 });
    let cache = GitCache::open(local.root().join("git")).expect("cache");
    assert_eq!(
        cache.repo_format(origin.object.as_str()).expect("format"),
        Some(GitObjectFormat::Sha256)
    );
    assert!(cache
        .has_commit(origin.object.as_str(), &origin.commit)
        .expect("has_commit"));
    assert!(local.root().join("git").join("pool-sha256").exists());
    assert!(!local.root().join("git").join("pool-sha1").exists());
}

#[test]
fn a_failed_git_step_leaves_no_repo_to_advertise() {
    if skip_if_no_git() {
        return;
    }
    // Same setup as the missing-commit case: the pull fails after
    // ingesting the peer's pack into the pool.
    let origin = origin(false);
    let (other, other_oid) =
        init_git_repo_with_manifest("[kairo]\nschema = 1\nname = \"other\"\n").expect("repo");
    let other_commit = GitRevision::new(GitObjectFormat::Sha1, &other_oid).expect("sha1 oid");
    let origin_cache = GitCache::open(origin.store.root().join("git")).expect("cache");
    origin_cache
        .ingest_pack(GitObjectFormat::Sha1, &build_pack_from(other.path()))
        .expect("ingest");
    origin_cache
        .set_ref(origin.object.as_str(), "refs/heads/main", &other_commit)
        .expect("ref");
    let served = serve(&origin);

    let (_dir, local) = local_store();
    assert!(matches!(
        pull_object(
            &local,
            &served.base_url,
            &origin.object,
            &PullOptions::default()
        ),
        Err(PullError::GitIncomplete { .. })
    ));
    let cache = GitCache::open(local.root().join("git")).expect("cache");
    assert_eq!(
        cache.repo_format(origin.object.as_str()).expect("format"),
        None
    );
    assert!(!cache.has_refs(origin.object.as_str()).expect("has_refs"));
}

#[test]
fn packs_are_served_from_the_cache_with_length_and_etag() {
    if skip_if_no_git() {
        return;
    }
    let origin = origin(true);
    let served = serve(&origin);
    let url = format!(
        "{}/federation/v1/objects/{}/git",
        served.base_url, origin.object
    );
    let fetch = || {
        let mut response = ureq::get(&url).call().expect("pack");
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let (length, etag) = (header("content-length"), header("etag"));
        let body = response
            .body_mut()
            .with_config()
            .limit(u64::MAX)
            .read_to_vec()
            .expect("body");
        (length, etag, body)
    };
    let (length, etag, body) = fetch();
    assert_eq!(length, Some(body.len().to_string()));
    let etag = etag.expect("etag");
    assert!(etag.starts_with('"') && etag.ends_with('"'), "{etag}");

    // The second request is the same cached file.
    let (_, etag_again, body_again) = fetch();
    assert_eq!(etag_again.as_deref(), Some(etag.as_str()));
    assert_eq!(body_again, body);
    let outgoing = origin.store.root().join("git").join("outgoing");
    assert!(outgoing.exists(), "cache directory written");
}
