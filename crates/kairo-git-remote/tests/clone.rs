//! `git clone kairo://…` against an in-process Kairo node
//! (`specs/DECISIONS.md` §31), with the real `git` and the built
//! helper on `PATH`.

#![allow(clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use kairo_core::Timestamp;
use kairo_git::GitCache;
use kairo_identity::{ActorGenesisBody, ActorKind, SecretSigningKey};
use kairo_statement::{GitObjectFormat, GitRevision};
use kairo_store::{ActorStore, FilesystemStore, ObjectStore, PublishedObjectStore};
use kairo_test_support::git::{build_pack_from, init_git_repo_with_manifest_in, skip_if_no_git};
use kairo_test_support::store::StoreFixture;
use tempfile::TempDir;

// Crates the lib depends on that this target doesn't name.
use kairo_config as _;
use kairo_federation_service as _;
use kairo_git_remote as _;
use tokio as _;

const MANIFEST: &str = r#"
    [kairo]
    schema = 1
    kind = "kairo/object"
    name = "git-remote-fixture"

    [content]
    kind = "tree"
"#;

/// An origin node serving one published object whose `head` and `main`
/// branches name a real commit held in its Git cache.
struct Origin {
    _dir: TempDir,
    _repo: TempDir,
    object: String,
    commit: GitRevision,
    url: String,
    _stop: std::sync::mpsc::Sender<()>,
}

fn origin(format: GitObjectFormat) -> Origin {
    let (repo, oid) = init_git_repo_with_manifest_in(MANIFEST, format.as_str()).expect("repo");
    let commit = GitRevision::new(format, &oid).expect("oid");
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
    fixture.set_branch(&alice, &created, &revision, "head");
    fixture.set_branch(&alice, &created, &revision, "main");
    let cache = GitCache::open(fixture.store.root().join("git")).expect("cache");
    cache
        .ingest_pack(format, &build_pack_from(repo.path()))
        .expect("ingest");
    cache
        .set_ref(created.object_id.as_str(), "refs/heads/main", &commit)
        .expect("ref");
    let store = fixture.store.clone();
    store
        .publish_object(&created.object_id, Timestamp::from_seconds(1_700_001_000))
        .expect("publish");
    let key = SecretSigningKey::ed25519([200; 32]);
    let node = store
        .put_actor(
            &ActorGenesisBody::new(
                ActorKind::node(),
                key.public_key(),
                vec![SecretSigningKey::ed25519([201; 32]).public_key()],
                1,
                Timestamp::from_seconds(1_700_000_000),
                [202; 32],
            )
            .expect("genesis"),
        )
        .expect("node");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let addr = listener.local_addr().expect("addr");
    let base = format!("http://{addr}");
    let served =
        kairo_federation_http::Node::from_store(store.clone(), &node, &key, &base, Duration::ZERO)
            .expect("node");
    let (stop, stopped) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
            let stop = async move {
                let _ = tokio::task::spawn_blocking(move || stopped.recv()).await;
            };
            kairo_federation_http::serve_on(served, listener, stop)
                .await
                .expect("serve");
        });
    });
    let object = created.object_id.to_string();
    Origin {
        url: format!("kairo://{addr}/{object}"),
        _dir: dir,
        _repo: repo,
        object,
        commit,
        _stop: stop,
    }
}

/// A user with a Kairo store whose config lists `peer`, if given.
struct User {
    home: TempDir,
}

impl User {
    fn new(peer: Option<&str>) -> Self {
        let home = TempDir::new().expect("home");
        let store = home.path().join(".kairo");
        std::fs::create_dir_all(&store).expect("store");
        if let Some(peer) = peer {
            std::fs::write(
                store.join("config.toml"),
                format!("[[federation.peers]]\nurl = \"{peer}\"\n"),
            )
            .expect("config");
        }
        Self { home }
    }

    fn store(&self) -> PathBuf {
        self.home.path().join(".kairo")
    }

    fn git(&self, cwd: &Path, args: &[&str]) -> Output {
        let helper = PathBuf::from(env!("CARGO_BIN_EXE_git-remote-kairo"));
        let path = format!(
            "{}:{}",
            helper.parent().expect("bin dir").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new("git")
            .current_dir(cwd)
            .args(args)
            .env("PATH", path)
            .env("HOME", self.home.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("KAIRO_STORE")
            .output()
            .expect("git")
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn clones_and_verifies(format: GitObjectFormat) {
    if skip_if_no_git() {
        return;
    }
    let origin = origin(format);
    let peer = origin.url.replace("kairo://", "http://");
    let peer = peer.rsplit_once('/').expect("path").0.to_owned();
    let user = User::new(Some(&peer));
    let work = TempDir::new().expect("work");

    let cloned = user.git(work.path(), &["clone", &origin.url, "clone"]);
    assert!(cloned.status.success(), "{}", stderr(&cloned));
    let clone = work.path().join("clone");
    let head = user.git(&clone, &["rev-parse", "HEAD"]);
    assert_eq!(
        String::from_utf8_lossy(&head.stdout).trim(),
        origin.commit.oid(),
        "HEAD is the creator's head branch"
    );
    let branches = user.git(&clone, &["branch", "-r"]);
    let branches = String::from_utf8_lossy(&branches.stdout);
    assert!(branches.contains("origin/main"), "{branches}");
    let format_seen = user.git(&clone, &["rev-parse", "--show-object-format"]);
    assert_eq!(
        String::from_utf8_lossy(&format_seen.stdout).trim(),
        format.as_str()
    );

    // The pull went into the user's Kairo store, verified.
    let store = FilesystemStore::open(user.store()).expect("store");
    let object = kairo_core::ObjectId::new(origin.object.clone()).expect("id");
    assert!(store.get_object_genesis(&object).is_ok());

    // Read-only.
    let pushed = user.git(&clone, &["push", "origin", "HEAD:refs/heads/new"]);
    assert!(!pushed.status.success());
    assert!(
        stderr(&pushed).contains("kairo push"),
        "{}",
        stderr(&pushed)
    );
}

#[test]
fn a_kairo_url_clones_verified_history_sha1() {
    clones_and_verifies(GitObjectFormat::Sha1);
}

#[test]
fn a_kairo_url_clones_verified_history_sha256() {
    clones_and_verifies(GitObjectFormat::Sha256);
}

#[test]
fn an_unlisted_host_is_refused_unless_allowed() {
    if skip_if_no_git() {
        return;
    }
    let origin = origin(GitObjectFormat::Sha1);
    let user = User::new(None);
    let work = TempDir::new().expect("work");
    let refused = user.git(work.path(), &["clone", &origin.url, "refused"]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("kairo.allowPeer"),
        "{}",
        stderr(&refused)
    );

    let allowed = user.git(
        work.path(),
        &[
            "clone",
            "-c",
            "kairo.allowPeer=true",
            &origin.url,
            "allowed",
        ],
    );
    assert!(allowed.status.success(), "{}", stderr(&allowed));
}
