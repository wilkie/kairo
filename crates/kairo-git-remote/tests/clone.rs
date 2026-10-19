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
    repo: TempDir,
    fixture: StoreFixture,
    alice: kairo_test_support::store::CreatedActor,
    created: kairo_test_support::store::CreatedObject,
    revision: kairo_test_support::store::CreatedRevision,
    /// The `main` branch statement, which a later move supersedes.
    main: kairo_core::StatementId,
    object: String,
    commit: GitRevision,
    url: String,
    _stop: std::sync::mpsc::Sender<()>,
}

impl Origin {
    /// Commit a file on top of the repo's head, sign it as a revision
    /// whose parent is `parent`, move `main` to it, and refresh the
    /// origin's cache.
    fn advance(&self, parent: &GitRevision) -> GitRevision {
        let path = self.repo.path();
        std::fs::write(path.join("notes.txt"), "more\n").expect("write");
        run(path, &["add", "notes.txt"]);
        run(path, &["commit", "-m", "next", "--quiet"]);
        let oid = run(path, &["rev-parse", "HEAD"]);
        let commit = GitRevision::new(parent.format(), &oid).expect("oid");
        let revision = self.fixture.make_revision(
            &self.alice,
            &self.created,
            commit.to_revision_id(),
            MANIFEST,
            vec![parent.to_revision_id()],
        );
        // The fixture's `set_branch` doesn't chain moves; supersede `main`
        // explicitly so the new tip wins.
        let moved = self.sign(
            &self.alice,
            kairo_statement::ObjectBranchBody::new(
                self.created.object_id.clone(),
                "main",
                revision.statement_id.clone(),
                Some(self.main.clone()),
            ),
        );
        kairo_store::StatementStore::put_object_branch(&self.fixture.store, &moved)
            .expect("move main");
        let cache = GitCache::open(self.fixture.store.root().join("git")).expect("cache");
        cache
            .ingest_pack(parent.format(), &build_pack_from(path))
            .expect("ingest");
        cache
            .set_ref(self.created.object_id.as_str(), "refs/heads/main", &commit)
            .expect("ref");
        commit
    }

    /// Sign `body` as `signer` about the object.
    fn sign<B: kairo_statement::StatementBody>(
        &self,
        signer: &kairo_test_support::store::CreatedActor,
        body: B,
    ) -> kairo_statement::SignedStatement<B> {
        use kairo_core::canonical::CanonicalEncode;
        let unsigned = kairo_statement::UnsignedStatement::new(
            signer.actor_id.clone(),
            format!("object:{}", self.created.object_id)
                .parse()
                .expect("subject"),
            Timestamp::from_seconds(1_700_000_900),
            body,
        );
        let bytes = signer.signing.sign(&unsigned.canonical_bytes());
        kairo_statement::SignedStatement::new(
            unsigned,
            kairo_statement::Signature::new(
                signer.actor_id.clone(),
                signer.signing.public_key().key_id().to_string(),
                "ed25519",
                bytes.bytes().to_vec(),
            ),
        )
    }
}

fn run(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
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
    let main = fixture
        .set_branch(&alice, &created, &revision, "main")
        .statement_id;
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
        repo,
        fixture,
        alice,
        created,
        revision,
        main,
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

/// The peer URL a `kairo://` URL names.
fn peer_of(origin: &Origin) -> String {
    let peer = origin.url.replace("kairo://", "http://");
    peer.rsplit_once('/').expect("path").0.to_owned()
}

#[test]
fn a_later_git_fetch_brings_new_signed_revisions() {
    if skip_if_no_git() {
        return;
    }
    let origin = origin(GitObjectFormat::Sha1);
    let user = User::new(Some(&peer_of(&origin)));
    let work = TempDir::new().expect("work");
    let cloned = user.git(work.path(), &["clone", &origin.url, "clone"]);
    assert!(cloned.status.success(), "{}", stderr(&cloned));
    let clone = work.path().join("clone");

    let next = origin.advance(&origin.commit);
    let fetched = user.git(&clone, &["fetch", "origin"]);
    assert!(fetched.status.success(), "{}", stderr(&fetched));
    let main = user.git(&clone, &["rev-parse", "origin/main"]);
    assert_eq!(String::from_utf8_lossy(&main.stdout).trim(), next.oid());
    // The new commit's parent is the first: the whole chain arrived.
    let parent = user.git(&clone, &["rev-parse", "origin/main^"]);
    assert_eq!(
        String::from_utf8_lossy(&parent.stdout).trim(),
        origin.commit.oid()
    );
}

#[test]
fn others_branches_are_namespaced_tags_listed_and_unheld_refs_left_out() {
    if skip_if_no_git() {
        return;
    }
    let origin = origin(GitObjectFormat::Sha1);
    let store = &origin.fixture.store;
    // Bob moves a branch of his own to alice's revision.
    let bob = origin.fixture.make_actor();
    let bob_branch = origin.sign(
        &bob,
        kairo_statement::ObjectBranchBody::new(
            origin.created.object_id.clone(),
            "experiment",
            origin.revision.statement_id.clone(),
            None,
        ),
    );
    kairo_store::StatementStore::put_object_branch(store, &bob_branch).expect("bob's branch");
    // Alice tags 1.0.0.
    let tag = origin.sign(
        &origin.alice,
        kairo_statement::ObjectVersionTagBody::new(
            origin.created.object_id.clone(),
            kairo_statement::SemverVersion::parse("1.0.0").expect("semver"),
            Some(origin.revision.statement_id.clone()),
            None,
        )
        .expect("tag"),
    );
    kairo_store::StatementStore::put_object_version_tag(store, &tag).expect("tag");
    // A branch whose revision names no Git commit.
    let elsewhere = origin.fixture.make_revision(
        &origin.alice,
        &origin.created,
        kairo_statement::RevisionId::new("tar:sha256:00"),
        MANIFEST,
        Vec::new(),
    );
    origin
        .fixture
        .set_branch(&origin.alice, &origin.created, &elsewhere, "docs");

    let user = User::new(Some(&peer_of(&origin)));
    let work = TempDir::new().expect("work");
    let cloned = user.git(
        work.path(),
        &["clone", "--mirror", &origin.url, "mirror.git"],
    );
    assert!(cloned.status.success(), "{}", stderr(&cloned));
    assert!(
        stderr(&cloned).contains("left out refs/heads/docs: its revision names no Git commit"),
        "{}",
        stderr(&cloned)
    );
    let mirror = work.path().join("mirror.git");
    let refs = user.git(
        &mirror,
        &["for-each-ref", "--format=%(refname) %(objectname)"],
    );
    let refs = String::from_utf8_lossy(&refs.stdout).into_owned();
    let oid = origin.commit.oid();
    for expected in [
        format!("refs/heads/main {oid}"),
        format!("refs/heads/head {oid}"),
        format!("refs/tags/1.0.0 {oid}"),
        format!("refs/kairo/actors/{}/experiment {oid}", bob.actor_id),
    ] {
        assert!(refs.contains(&expected), "{expected} missing from:\n{refs}");
    }
    assert!(!refs.contains("refs/heads/docs"), "{refs}");
    assert!(!refs.contains("refs/heads/experiment"), "{refs}");
}
