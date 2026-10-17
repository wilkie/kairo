//! Git transfer over Git's own smart HTTP (`specs/DECISIONS.md` §22):
//! the server runs `upload-pack` behind a filter that only serves
//! published commits, and a pull runs `git fetch`, negotiating so only
//! missing objects move. Every test needs the host's `git` binary and
//! returns early without one, like the other cache tests.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::process::Command;

use kairo_core::{ObjectId, StatementId, Timestamp};
use kairo_federation_core::{FedErrorCode, FedErrorEnvelope, ObjectListing};
use kairo_federation_http::client::{pull_object, GitOutcome, Limits, PullError, PullOptions};
use kairo_git::GitCache;
use kairo_statement::{GitObjectFormat, GitRevision};
use kairo_store::{FilesystemStore, PublishedObjectStore};
use kairo_test_support::git::{
    build_pack_from, init_git_repo_with_manifest, init_git_repo_with_manifest_in, skip_if_no_git,
};
use kairo_test_support::store::{CreatedActor, CreatedObject, StoreFixture};
use tempfile::TempDir;

use common::{node_actor, Served, BASE_URL, MANIFEST};

// Crates the lib or `common` depends on that this target doesn't name.
use axum as _;
use http_body as _;
use http_body_util as _;
use kairo_identity as _;
use kairo_object as _;
use serde as _;
use tokio as _;
use tower as _;
use tracing as _;
use tracing_subscriber as _;

/// An origin whose one revision names a real commit, with that commit
/// in the origin's managed cache when `cached`.
struct Origin {
    _dir: TempDir,
    repo: TempDir,
    fixture: StoreFixture,
    alice: CreatedActor,
    created: CreatedObject,
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
    let created = fixture.make_object(&alice, "kairo/object");
    let revision = fixture.make_revision(
        &alice,
        &created,
        commit.to_revision_id(),
        MANIFEST,
        Vec::new(),
    );
    fixture.set_branch(&alice, &created, &revision, "main");
    if cached {
        let cache = GitCache::open(fixture.store.root().join("git")).expect("cache");
        cache
            .ingest_pack(format, &build_pack_from(repo.path()))
            .expect("ingest");
        cache
            .set_ref(created.object_id.as_str(), "refs/heads/main", &commit)
            .expect("ref");
    }
    let store = fixture.store.clone();
    let node_actor = node_actor(&store, kairo_identity::ActorKind::node());
    store
        .publish_object(&created.object_id, Timestamp::from_seconds(1_700_001_000))
        .expect("publish");
    Origin {
        _dir: dir,
        repo,
        fixture,
        alice,
        object: created.object_id.clone(),
        created,
        store,
        node_actor,
        revision: revision.statement_id,
        commit,
    }
}

impl Origin {
    /// Commit `files` on top of the origin repo's head, sign it as a
    /// revision whose parent is the current one, put it on branch
    /// `next`, and refresh the origin's cache. Returns the new commit.
    fn advance(&self, files: &[(&str, &str)]) -> GitRevision {
        let path = self.repo.path();
        for (name, content) in files {
            std::fs::write(path.join(name), content).expect("write");
            git(path, &["add", name]);
        }
        git(path, &["commit", "-m", "next", "--quiet"]);
        let oid = git(path, &["rev-parse", "HEAD"]);
        let commit = GitRevision::new(self.commit.format(), &oid).expect("oid");
        let revision = self.fixture.make_revision(
            &self.alice,
            &self.created,
            commit.to_revision_id(),
            MANIFEST,
            vec![self.commit.to_revision_id()],
        );
        self.fixture
            .set_branch(&self.alice, &self.created, &revision, "next");
        let cache = GitCache::open(self.store.root().join("git")).expect("cache");
        cache
            .ingest_pack(self.commit.format(), &build_pack_from(path))
            .expect("ingest");
        cache
            .set_ref(self.object.as_str(), "refs/heads/next", &commit)
            .expect("ref");
        commit
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout)
        .expect("utf8")
        .trim()
        .to_owned()
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

fn git_url(served: &Served, object: &ObjectId) -> String {
    format!("{}/federation/v1/objects/{object}/git", served.base_url)
}

/// The ids in each pack the pool gained since `before`.
fn new_pack_objects(
    store: &FilesystemStore,
    before: &[std::path::PathBuf],
) -> Vec<std::path::PathBuf> {
    pack_indexes(store)
        .into_iter()
        .filter(|idx| !before.contains(idx))
        .collect()
}

fn pack_indexes(store: &FilesystemStore) -> Vec<std::path::PathBuf> {
    let dir = store.root().join("git/pool-sha1/objects/pack");
    let mut found: Vec<_> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "idx"))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

fn index_oids(idx: &Path) -> Vec<String> {
    let output = Command::new("git")
        .args(["show-index"])
        .stdin(std::fs::File::open(idx).expect("idx"))
        .output()
        .expect("show-index");
    String::from_utf8(output.stdout)
        .expect("utf8")
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(str::to_owned))
        .collect()
}

/// A raw protocol v2 request to the upload-pack endpoint.
fn upload_pack_raw(served: &Served, object: &ObjectId, lines: &[&str], v2: bool) -> (u16, Vec<u8>) {
    // Each line a pkt-line; "0001" is the delimiter.
    let mut framed = Vec::new();
    for line in lines {
        if *line == "0001" {
            framed.extend_from_slice(b"0001");
        } else {
            framed.extend_from_slice(format!("{:04x}{line}\n", line.len() + 5).as_bytes());
        }
    }
    framed.extend_from_slice(b"0000");
    let mut request = ureq::post(format!("{}/git-upload-pack", git_url(served, object)))
        .header("content-type", "application/x-git-upload-pack-request");
    if v2 {
        request = request.header("git-protocol", "version=2");
    }
    let mut response = request
        .config()
        .http_status_as_error(false)
        .build()
        .send(&framed[..])
        .expect("request");
    let status = response.status().as_u16();
    let bytes = response
        .body_mut()
        .with_config()
        .limit(u64::MAX)
        .read_to_vec()
        .expect("body");
    (status, bytes)
}

fn error_code(bytes: &[u8]) -> FedErrorCode {
    serde_json::from_slice::<FedErrorEnvelope>(bytes)
        .expect("envelope")
        .error
        .code
}

#[test]
fn pull_fetches_the_commits_and_pins_each() {
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
    assert!(
        !local.root().join("git/incoming").exists()
            || std::fs::read_dir(local.root().join("git/incoming"))
                .expect("incoming")
                .next()
                .is_none(),
        "no quarantine left behind"
    );

    // The commit is pinned in the object's own repo, so this node can
    // serve it onward.
    let pack = GitCache::open(local.root().join("git"))
        .expect("cache")
        .pack_for_object(origin.object.as_str())
        .expect("pack");
    assert!(pack.starts_with(b"PACK"));

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
fn a_second_pull_fetches_only_what_is_new() {
    if skip_if_no_git() {
        return;
    }
    let origin = origin(true);
    let served = serve(&origin);
    let (_dir, local) = local_store();
    pull_object(
        &local,
        &served.base_url,
        &origin.object,
        &PullOptions::default(),
    )
    .expect("first pull");

    let next = origin.advance(&[("data.txt", "new content\n")]);
    let before = pack_indexes(&local);
    let report = pull_object(
        &local,
        &served.base_url,
        &origin.object,
        &PullOptions::default(),
    )
    .expect("second pull");
    assert_eq!(report.git, GitOutcome::Ingested { commits: 1 });
    assert!(local_has_commit(&local, &origin.object, &next));

    // Negotiation sent the first commit as a `have`, so the new pack
    // carries the new commit but not the old one.
    let added = new_pack_objects(&local, &before);
    assert_eq!(added.len(), 1, "one new pack");
    let oids = index_oids(&added[0]);
    assert!(oids.contains(&next.oid().to_owned()));
    assert!(
        !oids.contains(&origin.commit.oid().to_owned()),
        "the commit the puller already had was not resent"
    );
}

#[test]
fn a_peer_without_git_reports_unavailable() {
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
        "{}/info/refs?service=git-upload-pack",
        git_url(&served, &origin.object)
    ))
    .header("git-protocol", "version=2")
    .config()
    .http_status_as_error(false)
    .build()
    .call()
    .expect("request")
    .status();
    assert_eq!(status.as_u16(), 404);
}

#[test]
fn a_plain_git_client_can_mirror_the_published_commits() {
    if skip_if_no_git() {
        return;
    }
    let origin = origin(true);
    let served = serve(&origin);
    let mirror = TempDir::new().expect("tempdir");
    let output = Command::new("git")
        .args(["-c", "protocol.version=2", "clone", "--mirror", "--quiet"])
        .arg(git_url(&served, &origin.object))
        .arg(mirror.path().join("mirror.git"))
        .output()
        .expect("clone");
    assert!(output.status.success(), "{output:?}");
    let refs = git(&mirror.path().join("mirror.git"), &["for-each-ref"]);
    assert!(
        refs.contains(&format!(
            "{} commit\trefs/kairo/revisions/{}",
            origin.commit.oid(),
            origin.commit.oid()
        )),
        "{refs}"
    );

    // Unpublishing hides the endpoint like every other record.
    origin
        .store
        .unpublish_object(&origin.object)
        .expect("unpublish");
    let output = Command::new("git")
        .args(["-c", "protocol.version=2", "ls-remote"])
        .arg(git_url(&served, &origin.object))
        .output()
        .expect("ls-remote");
    assert!(!output.status.success());
}

#[test]
fn unpublished_objects_in_the_shared_pool_are_never_served() {
    if skip_if_no_git() {
        return;
    }
    // The origin's pool also holds a commit of an object it doesn't
    // publish. Protocol v2 `upload-pack` alone would serve it to
    // anyone who names it; the filter must not.
    let origin = origin(true);
    let (secret_repo, secret_oid) =
        init_git_repo_with_manifest("[kairo]\nschema = 1\nname = \"secret\"\n").expect("repo");
    let secret = GitRevision::new(GitObjectFormat::Sha1, &secret_oid).expect("oid");
    let private = origin.fixture.make_object(&origin.alice, "kairo/object");
    let cache = GitCache::open(origin.store.root().join("git")).expect("cache");
    cache
        .ingest_pack(GitObjectFormat::Sha1, &build_pack_from(secret_repo.path()))
        .expect("ingest");
    cache
        .set_ref(private.object_id.as_str(), "refs/heads/main", &secret)
        .expect("ref");
    let served = serve(&origin);

    // Wanting it through the published object is refused.
    let (status, body) = upload_pack_raw(
        &served,
        &origin.object,
        &[
            "command=fetch",
            "0001",
            &format!("want {secret_oid}"),
            "done",
        ],
        true,
    );
    assert_eq!(status, 400);
    assert_eq!(error_code(&body), FedErrorCode::FedBadGitRequest);

    // So is a real `git fetch` of it.
    let target = TempDir::new().expect("tempdir");
    git(target.path(), &["init", "--bare", "--quiet"]);
    let output = Command::new("git")
        .arg("-C")
        .arg(target.path())
        .args(["-c", "protocol.version=2", "fetch", "--quiet"])
        .arg(git_url(&served, &origin.object))
        .arg(format!("{secret_oid}:refs/stolen"))
        .output()
        .expect("fetch");
    assert!(!output.status.success(), "secret commit fetched");

    // The private object has no endpoint of its own.
    let status = ureq::get(format!(
        "{}/info/refs?service=git-upload-pack",
        git_url(&served, &private.object_id)
    ))
    .header("git-protocol", "version=2")
    .config()
    .http_status_as_error(false)
    .build()
    .call()
    .expect("request")
    .status();
    assert_eq!(status.as_u16(), 404);

    // `ls-refs` names only the published commit.
    let (status, body) = upload_pack_raw(&served, &origin.object, &["command=ls-refs"], true);
    assert_eq!(status, 200);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains(origin.commit.oid()), "{text}");
    assert!(!text.contains(&secret_oid), "{text}");

    // A `have` of the secret commit is dropped, not acknowledged.
    let (status, body) = upload_pack_raw(
        &served,
        &origin.object,
        &[
            "command=fetch",
            "0001",
            &format!("want {}", origin.commit.oid()),
            &format!("have {secret_oid}"),
        ],
        true,
    );
    assert_eq!(status, 200);
    let text = String::from_utf8_lossy(&body);
    assert!(!text.contains(&format!("ACK {secret_oid}")), "{text}");
}

#[test]
fn only_protocol_v2_ls_refs_and_fetch_are_served() {
    if skip_if_no_git() {
        return;
    }
    let origin = origin(true);
    let served = serve(&origin);

    let (status, body) = upload_pack_raw(&served, &origin.object, &["command=ls-refs"], false);
    assert_eq!(status, 400, "protocol v0 refused");
    assert_eq!(error_code(&body), FedErrorCode::FedBadGitRequest);

    let (status, _) = upload_pack_raw(
        &served,
        &origin.object,
        &["command=object-info", "0001", "size"],
        true,
    );
    assert_eq!(status, 400, "object-info refused");

    // Git gzips request bodies over 1 KiB.
    let mut gzipped = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut gzipped, b"0014command=ls-refs\n0000").expect("gzip");
    let mut response = ureq::post(format!(
        "{}/git-upload-pack",
        git_url(&served, &origin.object)
    ))
    .header("content-type", "application/x-git-upload-pack-request")
    .header("content-encoding", "gzip")
    .header("git-protocol", "version=2")
    .send(&gzipped.finish().expect("gzip")[..])
    .expect("gzipped ls-refs");
    let refs = response.body_mut().read_to_string().expect("body");
    assert!(refs.contains(origin.commit.oid()), "{refs}");

    let mut response = ureq::get(format!(
        "{}/info/refs?service=git-upload-pack",
        git_url(&served, &origin.object)
    ))
    .header("git-protocol", "version=2")
    .call()
    .expect("advertisement");
    let advertisement = response.body_mut().read_to_string().expect("body");
    assert!(advertisement.contains("version 2"));
    assert!(advertisement.contains("fetch="));
    assert!(!advertisement.contains("object-info"), "{advertisement}");

    let status = ureq::get(format!(
        "{}/info/refs?service=git-receive-pack",
        git_url(&served, &origin.object)
    ))
    .header("git-protocol", "version=2")
    .config()
    .http_status_as_error(false)
    .build()
    .call()
    .expect("request")
    .status();
    assert_eq!(status.as_u16(), 400, "receive-pack is never served");

    let status = ureq::post(format!(
        "{}/git-receive-pack",
        git_url(&served, &origin.object)
    ))
    .config()
    .http_status_as_error(false)
    .build()
    .send(&b""[..])
    .expect("request")
    .status();
    assert_eq!(status.as_u16(), 405, "no other POST");
}

#[test]
fn a_peer_missing_the_revision_commit_fails_the_pull() {
    if skip_if_no_git() {
        return;
    }
    // The origin's cache holds an unrelated repo's history for the
    // object, so the signed commit can't be fetched from it.
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
    let result = pull_object(
        &local,
        &served.base_url,
        &origin.object,
        &PullOptions::default(),
    );
    assert!(
        matches!(
            result,
            Err(PullError::Git(_) | PullError::GitIncomplete { .. })
        ),
        "{result:?}"
    );
    // Statements were stored before the Git step and stay.
    assert!(local.get_statement_bytes(&origin.revision).is_ok());
    // And nothing was left to advertise.
    let cache = GitCache::open(local.root().join("git")).expect("cache");
    assert_eq!(
        cache.repo_format(origin.object.as_str()).expect("format"),
        None
    );
    assert!(!cache.has_refs(origin.object.as_str()).expect("has_refs"));
}

#[test]
fn an_oversize_fetch_fails_the_pull() {
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
    assert!(
        matches!(result, Err(PullError::TooLarge { limit: 16, .. })),
        "{result:?}"
    );
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
