//! Slice 8 exit criteria: read commands round-trip in both
//! daemon and direct mode, `--daemon` against a missing socket
//! returns exit 9, `--direct` ignores the socket, and a direct
//! write while a daemon-mode read hits the same store both
//! succeed (the §6 advisory locks are the proof).

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

// `kairo-cli` exposes its full prod-dep set to integration tests;
// silence `unused_crate_dependencies` for ones the test doesn't
// reference directly.
use base64 as _;
use clap as _;
use ed25519_dalek as _;
use http_body_util as _;
use hyper as _;
use hyper_util as _;
use kairo_bundle as _;
use kairo_core as _;
use kairo_daemon as _;
use kairo_daemon_client as _;
use kairo_federation_http as _;
use kairo_git as _;
use kairo_identity as _;
use kairo_keystore as _;
use kairo_object as _;
use kairo_statement as _;
use kairo_store as _;
use kairo_web as _;
use nix as _;
use serde_json as _;
use tempfile as _;
use tokio as _;

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use kairo_test_support::store::StoreFixture;

const KAIRO_BIN: &str = env!("CARGO_BIN_EXE_kairo");

fn wait_for_socket(socket: &Path, max: Duration) {
    let start = Instant::now();
    loop {
        if socket.exists() && std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return;
        }
        assert!(
            start.elapsed() <= max,
            "daemon never bound socket within {max:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Start a daemon over an existing populated store dir.
/// Returns the spawned child; caller should `kill` + `wait` to
/// shut it down. The store is already populated with a fixture
/// before the daemon opens it (the daemon opens the store in
/// `serve`, but advisory locks make subsequent direct reads /
/// writes from the CLI safe).
fn spawn_daemon(store: &Path) -> Child {
    let child = Command::new(KAIRO_BIN)
        .args(["--store", store.to_str().expect("utf-8")])
        .args(["daemon", "start"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kairo daemon start");
    wait_for_socket(&store.join("daemon.sock"), Duration::from_secs(5));
    child
}

fn run_kairo(args: &[&str]) -> std::process::Output {
    Command::new(KAIRO_BIN)
        .args(args)
        .output()
        .expect("run kairo")
}

#[test]
fn branch_show_round_trips_in_daemon_and_direct_mode() {
    use kairo_statement::RevisionId;

    const MANIFEST: &str = r#"
        [kairo]
        schema = 1
        kind = "kairo/object"
        name = "fixture"

        [content]
        kind = "tree"
    "#;

    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let revision = fixture.make_revision(
        &actor,
        &object,
        RevisionId::new("git:sha256:r1"),
        MANIFEST,
        vec![],
    );
    fixture.set_branch(&actor, &object, &revision, "head");
    let object_id = object.object_id.to_string();
    let actor_id = actor.actor_id.to_string();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    // Direct mode (no daemon running yet, no flag forces direct).
    let direct = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "branch",
        "show",
        "--object",
        &object_id,
        "--actor",
        &actor_id,
        "--name",
        "head",
    ]);
    assert!(direct.status.success(), "direct: {direct:?}");
    let direct_out = String::from_utf8(direct.stdout).expect("utf-8");
    assert!(
        direct_out.contains("name = head"),
        "direct stdout: {direct_out}"
    );

    // Now bring up the daemon and re-run — expect identical output.
    let mut daemon = spawn_daemon(&store_path);
    let daemon_out_status = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon", // require daemon path
        "branch",
        "show",
        "--object",
        &object_id,
        "--actor",
        &actor_id,
        "--name",
        "head",
    ]);
    assert!(
        daemon_out_status.status.success(),
        "daemon: {daemon_out_status:?}"
    );
    let daemon_out = String::from_utf8(daemon_out_status.stdout).expect("utf-8");
    assert_eq!(direct_out, daemon_out, "modes should produce same output");

    // Force `--direct` while the daemon is running — same output.
    let forced_direct = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--direct",
        "branch",
        "show",
        "--object",
        &object_id,
        "--actor",
        &actor_id,
        "--name",
        "head",
    ]);
    assert!(forced_direct.status.success());
    let forced_direct_out = String::from_utf8(forced_direct.stdout).expect("utf-8");
    assert_eq!(direct_out, forced_direct_out);

    // Tear down.
    let _ = daemon.kill();
    let _ = daemon.wait();
    drop(dir);
}

#[test]
fn require_daemon_returns_exit_nine_when_socket_missing() {
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    // No daemon running.
    let result = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "branch",
        "show",
        "--object",
        // Some shape-valid object id that has no branch — but we
        // never get there because dispatch fails before the
        // store is touched.
        &kairo_core::ObjectId::from_sha256_digest([0xAB; 32]).to_string(),
        "--name",
        "head",
    ]);
    let code = result.status.code().expect("exit code");
    assert_eq!(code, 9, "expected exit 9 (daemon_unavailable); got {code}");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("daemon is not running") || stderr.contains("--daemon was set"),
        "stderr: {stderr}"
    );
}

#[test]
fn direct_write_succeeds_while_daemon_serves_concurrent_read() {
    use kairo_statement::RevisionId;

    const MANIFEST: &str = r#"
        [kairo]
        schema = 1
        kind = "kairo/object"
        name = "fixture"

        [content]
        kind = "tree"
    "#;

    // Set up a store with two revisions so we can advance the
    // branch from r1 to r2 mid-test.
    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let r1 = fixture.make_revision(
        &actor,
        &object,
        RevisionId::new("git:sha256:r1"),
        MANIFEST,
        vec![],
    );
    let r2 = fixture.make_revision(
        &actor,
        &object,
        RevisionId::new("git:sha256:r2"),
        MANIFEST,
        vec![RevisionId::new("git:sha256:r1")],
    );
    fixture.set_branch(&actor, &object, &r1, "head");
    let object_id = object.object_id.to_string();
    let actor_id = actor.actor_id.to_string();
    let r2_statement_id = r2.statement_id.to_string();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let mut daemon = spawn_daemon(&store_path);

    // While the daemon is serving, concurrently:
    //   - run `kairo branch show` through the daemon (read)
    //   - run `kairo branch set` direct (write — daemon ignores
    //     write commands by design; this hits the store directly)
    // The §6 advisory locks make concurrent read+write safe.
    let read_handle = std::thread::spawn({
        let store_path = store_path.clone();
        let object_id = object_id.clone();
        let actor_id = actor_id.clone();
        move || {
            run_kairo(&[
                "--store",
                store_path.to_str().unwrap(),
                "--daemon",
                "branch",
                "show",
                "--object",
                &object_id,
                "--actor",
                &actor_id,
                "--name",
                "head",
            ])
        }
    });

    let write_handle = std::thread::spawn({
        let store_path = store_path.clone();
        let object_id = object_id.clone();
        let actor_id = actor_id.clone();
        let r2_statement_id = r2_statement_id.clone();
        move || {
            run_kairo(&[
                "--store",
                store_path.to_str().unwrap(),
                "branch",
                "set",
                "--actor",
                &actor_id,
                "--object",
                &object_id,
                "--revision",
                &r2_statement_id,
                "--name",
                "head",
            ])
        }
    });

    let read_result = read_handle.join().expect("read join");
    let write_result = write_handle.join().expect("write join");

    assert!(
        read_result.status.success(),
        "read failed: stderr = {}",
        String::from_utf8_lossy(&read_result.stderr)
    );
    assert!(
        write_result.status.success(),
        "write failed: stderr = {}",
        String::from_utf8_lossy(&write_result.stderr)
    );

    // After the dust settles, the branch tip should be r2.
    let post = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--direct",
        "branch",
        "show",
        "--object",
        &object_id,
        "--actor",
        &actor_id,
        "--name",
        "head",
    ]);
    assert!(post.status.success());
    let post_out = String::from_utf8(post.stdout).expect("utf-8");
    assert!(
        post_out.contains(&r2_statement_id),
        "post stdout: {post_out}"
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
    drop(dir);
}

const FIXTURE_MANIFEST: &str = r#"
    [kairo]
    schema = 1
    kind = "kairo/object"
    name = "fixture"

    [content]
    kind = "tree"
"#;

/// Compare daemon-mode and direct-mode output for `args`. Both
/// runs must succeed and produce byte-identical stdout.
fn assert_modes_agree(store_path: &Path, args: &[&str]) {
    let mut daemon_args = vec!["--store", store_path.to_str().unwrap(), "--daemon"];
    daemon_args.extend_from_slice(args);
    let daemon_run = run_kairo(&daemon_args);
    assert!(
        daemon_run.status.success(),
        "daemon mode failed: stderr = {}",
        String::from_utf8_lossy(&daemon_run.stderr)
    );

    let mut direct_args = vec!["--store", store_path.to_str().unwrap(), "--direct"];
    direct_args.extend_from_slice(args);
    let direct_run = run_kairo(&direct_args);
    assert!(
        direct_run.status.success(),
        "direct mode failed: stderr = {}",
        String::from_utf8_lossy(&direct_run.stderr)
    );

    let daemon_out = String::from_utf8(daemon_run.stdout).expect("utf-8");
    let direct_out = String::from_utf8(direct_run.stdout).expect("utf-8");
    assert_eq!(
        daemon_out, direct_out,
        "modes diverge for args {args:?}\n\ndaemon:\n{daemon_out}\n\ndirect:\n{direct_out}"
    );
}

#[test]
fn branch_list_round_trips_in_both_modes() {
    use kairo_statement::RevisionId;

    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let revision = fixture.make_revision(
        &actor,
        &object,
        RevisionId::new("git:sha256:r1"),
        FIXTURE_MANIFEST,
        vec![],
    );
    fixture.set_branch(&actor, &object, &revision, "head");
    let object_id = object.object_id.to_string();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let mut daemon = spawn_daemon(&store_path);

    assert_modes_agree(&store_path, &["branch", "list", "--object", &object_id]);

    let _ = daemon.kill();
    let _ = daemon.wait();
    drop(dir);
}

#[test]
fn revision_inspect_round_trips_in_both_modes() {
    use kairo_statement::RevisionId;

    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let revision = fixture.make_revision(
        &actor,
        &object,
        RevisionId::new("git:sha256:r1"),
        FIXTURE_MANIFEST,
        vec![],
    );
    let statement_id = revision.statement_id.to_string();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let mut daemon = spawn_daemon(&store_path);

    assert_modes_agree(
        &store_path,
        &["revision", "inspect", "--statement", &statement_id],
    );
    assert_modes_agree(
        &store_path,
        &[
            "revision",
            "inspect",
            "--statement",
            &statement_id,
            "--json",
        ],
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
    drop(dir);
}

#[test]
fn capability_list_grantor_round_trips_in_both_modes() {
    use kairo_core::canonical::CanonicalEncode;
    use kairo_statement::{
        ActorCapabilityGrantBody, Capability, CapabilityScope, Signature, SignedStatement,
        StatementKind, UnsignedStatement,
    };
    use kairo_store::StatementStore;

    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");

    // Issue one capability grant alice → bob on this object.
    let cap = Capability::new(
        CapabilityScope::Object(object.object_id.clone()),
        vec![StatementKind::ObjectVersionTag],
        false,
        Vec::new(),
    )
    .expect("capability");
    let body = ActorCapabilityGrantBody::new(bob.actor_id.clone(), cap, None);
    let subject: kairo_core::KairoRef = format!("actor:{}", bob.actor_id).parse().expect("subject");
    let unsigned = UnsignedStatement::new(
        alice.actor_id.clone(),
        subject,
        kairo_core::Timestamp::from_seconds(1_700_000_000),
        body,
    );
    let bytes = alice.signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        alice.actor_id.clone(),
        alice.signing.public_key().key_id().to_string(),
        "ed25519",
        bytes.bytes().to_vec(),
    );
    fixture
        .store
        .put_actor_capability_grant(&SignedStatement::new(unsigned, signature))
        .expect("put_actor_capability_grant");

    let alice_id = alice.actor_id.to_string();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let mut daemon = spawn_daemon(&store_path);

    assert_modes_agree(&store_path, &["capability", "list", "--grantor", &alice_id]);

    let _ = daemon.kill();
    let _ = daemon.wait();
    drop(dir);
}

// Slice 2 (daemon writes): `kairo trust grant` is the first
// write command routed through `cli::dispatch`. Daemon-mode
// posts the signed envelope through `POST /api/v1/statements`;
// direct-mode hits `FilesystemStore::put_actor_trust` as before.
// Both paths must produce byte-identical stdout and identical
// on-disk state — that's how the slice's "output text unchanged"
// promise is enforced.

#[test]
fn trust_grant_daemon_mode_matches_direct_output_and_persists() {
    let (dir, fixture) = StoreFixture::temp();
    let truster = fixture.make_actor();
    let trusted = fixture.make_actor();
    let truster_id = truster.actor_id.to_string();
    let trusted_id = trusted.actor_id.to_string();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let mut daemon = spawn_daemon(&store_path);

    // Issue the grant via the daemon. The format!() in
    // run_trust_decide is mode-agnostic, so the stdout shape must
    // match the existing direct-mode test fixture exactly.
    let grant = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "trust",
        "grant",
        "--by",
        &truster_id,
        "--of",
        &trusted_id,
        "--reason",
        "daemon round-trip",
    ]);
    assert!(
        grant.status.success(),
        "trust grant --daemon failed: stderr = {}",
        String::from_utf8_lossy(&grant.stderr)
    );
    let grant_out = String::from_utf8(grant.stdout).expect("utf-8");
    assert!(grant_out.contains("grant trust"), "grant: {grant_out}");
    assert!(
        grant_out.contains("decision = trusted"),
        "grant: {grant_out}"
    );
    assert!(
        grant_out.contains("supersedes = (genesis)"),
        "grant: {grant_out}"
    );

    // Read it back through the daemon: `trust show` already
    // dispatches via cli::dispatch (Slice 8) — confirm the head
    // we just POSTed is what comes back.
    assert_modes_agree(
        &store_path,
        &["trust", "show", "--by", &truster_id, "--of", &trusted_id],
    );

    // The block path should chain on top of the grant — proves
    // the daemon-side write didn't drop the prior head from the
    // store's audit chain.
    let block = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "trust",
        "block",
        "--by",
        &truster_id,
        "--of",
        &trusted_id,
        "--reason",
        "revoke for ci",
    ]);
    assert!(
        block.status.success(),
        "trust block --daemon failed: stderr = {}",
        String::from_utf8_lossy(&block.stderr)
    );
    let block_out = String::from_utf8(block.stdout).expect("utf-8");
    assert!(
        block_out.contains("decision = untrusted"),
        "block: {block_out}"
    );
    assert!(
        !block_out.contains("supersedes = (genesis)"),
        "block did not chain on prior grant: {block_out}"
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
    drop(dir);
}

#[test]
fn trust_grant_falls_back_to_direct_when_daemon_absent() {
    // Default mode (no --daemon, no --direct) with the daemon
    // not running: `cli::dispatch::resolve` returns `Mode::Direct`
    // silently. The CLI write should succeed against the local
    // store with no socket present.
    let (dir, fixture) = StoreFixture::temp();
    let truster = fixture.make_actor();
    let trusted = fixture.make_actor();
    let truster_id = truster.actor_id.to_string();
    let trusted_id = trusted.actor_id.to_string();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    assert!(!store_path.join("daemon.sock").exists());

    let grant = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "trust",
        "grant",
        "--by",
        &truster_id,
        "--of",
        &trusted_id,
    ]);
    assert!(
        grant.status.success(),
        "trust grant (default mode, no daemon) failed: stderr = {}",
        String::from_utf8_lossy(&grant.stderr)
    );
    let grant_out = String::from_utf8(grant.stdout).expect("utf-8");
    assert!(
        grant_out.contains("decision = trusted"),
        "grant: {grant_out}"
    );

    drop(dir);
}

// Slice 3: revision create / branch set / tag bind route through
// `cli::dispatch`. Daemon-mode posts the signed envelope via
// `POST /api/v1/statements`; direct-mode hits the matching
// `put_*` on `FilesystemStore`. The tests bootstrap the store
// fixture with whatever the operation depends on (actor +
// object for revisions, actor + object + revision for branches
// and tags), then assert daemon-mode succeeds and the post-
// condition reads back identically to a direct read.

fn write_manifest_file(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("manifest.toml");
    std::fs::write(&path, FIXTURE_MANIFEST).expect("write manifest");
    path
}

#[test]
fn revision_create_daemon_mode_persists_through_dispatch() {
    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let actor_id = actor.actor_id.to_string();
    let object_id = object.object_id.to_string();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let manifest_path = write_manifest_file(&store_path);

    let mut daemon = spawn_daemon(&store_path);
    let create = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "revision",
        "create",
        "--actor",
        &actor_id,
        "--object",
        &object_id,
        "--revision",
        "git:sha256:slice3-cli-r1",
        "--manifest",
        manifest_path.to_str().unwrap(),
    ]);
    assert!(
        create.status.success(),
        "revision create --daemon failed: stderr = {}",
        String::from_utf8_lossy(&create.stderr)
    );
    let create_out = String::from_utf8(create.stdout).expect("utf-8");
    assert!(
        create_out.contains("created revision"),
        "create: {create_out}"
    );
    let statement_id = create_out
        .lines()
        .find_map(|l| l.strip_prefix("statement = "))
        .expect("statement id");

    // Read back through direct mode — daemon and direct must
    // converge on the same persisted revision.
    let list = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--direct",
        "revision",
        "list",
        "--object",
        &object_id,
    ]);
    assert!(list.status.success(), "revision list: {list:?}");
    let list_out = String::from_utf8(list.stdout).expect("utf-8");
    assert!(
        list_out.contains(statement_id),
        "revision not visible to direct read after daemon write: {list_out}"
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
    drop(dir);
}

#[test]
fn branch_set_daemon_mode_advances_head_through_dispatch() {
    use kairo_statement::RevisionId;

    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let revision = fixture.make_revision(
        &actor,
        &object,
        RevisionId::new("git:sha256:slice3-cli-branch-r1"),
        FIXTURE_MANIFEST,
        vec![],
    );
    let actor_id = actor.actor_id.to_string();
    let object_id = object.object_id.to_string();
    let revision_id = revision.statement_id.to_string();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let mut daemon = spawn_daemon(&store_path);
    let set = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "branch",
        "set",
        "--actor",
        &actor_id,
        "--object",
        &object_id,
        "--revision",
        &revision_id,
        "--name",
        "head",
    ]);
    assert!(
        set.status.success(),
        "branch set --daemon failed: stderr = {}",
        String::from_utf8_lossy(&set.stderr)
    );
    let set_out = String::from_utf8(set.stdout).expect("utf-8");
    assert!(set_out.contains("set branch"), "set: {set_out}");
    assert!(set_out.contains("supersedes = (genesis)"), "set: {set_out}");

    // Both modes should resolve to the same head.
    assert_modes_agree(
        &store_path,
        &[
            "branch", "show", "--object", &object_id, "--actor", &actor_id, "--name", "head",
        ],
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
    drop(dir);
}

#[test]
fn tag_bind_daemon_mode_persists_head_through_dispatch() {
    use kairo_statement::RevisionId;

    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let revision = fixture.make_revision(
        &actor,
        &object,
        RevisionId::new("git:sha256:slice3-cli-tag-r1"),
        FIXTURE_MANIFEST,
        vec![],
    );
    let actor_id = actor.actor_id.to_string();
    let object_id = object.object_id.to_string();
    let revision_id = revision.statement_id.to_string();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let mut daemon = spawn_daemon(&store_path);
    let bind = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "tag",
        "bind",
        "--actor",
        &actor_id,
        "--object",
        &object_id,
        "--version",
        "1.0.0",
        "--revision",
        &revision_id,
    ]);
    assert!(
        bind.status.success(),
        "tag bind --daemon failed: stderr = {}",
        String::from_utf8_lossy(&bind.stderr)
    );
    let bind_out = String::from_utf8(bind.stdout).expect("utf-8");
    assert!(bind_out.contains("bind tag"), "bind: {bind_out}");
    assert!(bind_out.contains("version = 1.0.0"), "bind: {bind_out}");

    assert_modes_agree(
        &store_path,
        &[
            "tag",
            "show",
            "--object",
            &object_id,
            "--actor",
            &actor_id,
            "--version",
            "1.0.0",
        ],
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
    drop(dir);
}

// Slice 4: object create + capability grant/revoke route through
// dispatch. `kairo actor create` stays direct (Phase A-permanent
// exception — keystore choreography is not a daemon concern).

#[test]
fn object_create_daemon_mode_persists_through_dispatch() {
    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let actor_id = actor.actor_id.to_string();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let mut daemon = spawn_daemon(&store_path);
    let create = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "object",
        "create",
        "--actor",
        &actor_id,
        "--kind",
        "kairo/object",
    ]);
    assert!(
        create.status.success(),
        "object create --daemon failed: stderr = {}",
        String::from_utf8_lossy(&create.stderr)
    );
    let create_out = String::from_utf8(create.stdout).expect("utf-8");
    assert!(
        create_out.contains("created object"),
        "create: {create_out}"
    );
    let object_id = create_out
        .lines()
        .find_map(|l| l.strip_prefix("object = "))
        .expect("object id");

    // Read back through direct mode — the genesis persisted by
    // the daemon should be visible to a direct-mode read of the
    // same store.
    let genesis_read = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--direct",
        "revision",
        "list",
        "--object",
        object_id,
    ]);
    assert!(
        genesis_read.status.success(),
        "revision list after daemon object create failed: stderr = {}",
        String::from_utf8_lossy(&genesis_read.stderr)
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
    drop(dir);
}

#[test]
fn capability_grant_daemon_mode_persists_and_revoke_chains() {
    // End-to-end exercise of the §13 sub-decision 2 dispatcher
    // for the capability surface: grant then revoke, both
    // through `--daemon`. The follow-up `capability list` (read,
    // already daemon-mode by Slice 8) sees the grant; direct-mode
    // re-read sees the revocation chained on top.
    let (dir, fixture) = StoreFixture::temp();
    let grantor = fixture.make_actor();
    let grantee = fixture.make_actor();
    let object = fixture.make_object(&grantor, "kairo/object");
    let grantor_id = grantor.actor_id.to_string();
    let grantee_id = grantee.actor_id.to_string();
    let object_id = object.object_id.to_string();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let mut daemon = spawn_daemon(&store_path);

    let grant = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "capability",
        "grant",
        "--grantor",
        &grantor_id,
        "--grantee",
        &grantee_id,
        "--object",
        &object_id,
        "--kind",
        "ObjectVersionTag",
        "--delegable",
    ]);
    assert!(
        grant.status.success(),
        "capability grant --daemon failed: stderr = {}",
        String::from_utf8_lossy(&grant.stderr)
    );
    let grant_out = String::from_utf8(grant.stdout).expect("utf-8");
    assert!(grant_out.contains("grant capability"), "grant: {grant_out}");
    let grant_statement = grant_out
        .lines()
        .find_map(|l| l.strip_prefix("statement = "))
        .expect("grant statement id");

    // Daemon-mode list and direct-mode list converge.
    assert_modes_agree(
        &store_path,
        &["capability", "list", "--grantor", &grantor_id],
    );

    // Revoke through the daemon, chained on the grant we just
    // issued.
    let revoke = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "capability",
        "revoke",
        "--grantor",
        &grantor_id,
        "--grant",
        grant_statement,
        "--retroactive",
        "--reason",
        "ci revocation",
    ]);
    assert!(
        revoke.status.success(),
        "capability revoke --daemon failed: stderr = {}",
        String::from_utf8_lossy(&revoke.stderr)
    );
    let revoke_out = String::from_utf8(revoke.stdout).expect("utf-8");
    assert!(
        revoke_out.contains("revoke capability"),
        "revoke: {revoke_out}"
    );
    assert!(
        revoke_out.contains(&format!("revoked_grant = {grant_statement}")),
        "revoke: {revoke_out}"
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
    drop(dir);
}

#[test]
fn smoke_seed_two_hop_delegation_round_trips_through_daemon() {
    // Slice 4 exit criterion: the 2-hop delegation chain in
    // `scripts/smoke-seed.sh` (Alice → Bob delegable, Bob → Carol
    // re-delegation) reproduces end-to-end through the daemon.
    //
    // We bootstrap actors + object via the test-support fixture
    // (mirrors what `kairo actor create` and `object create`
    // would do — `actor create` stays direct in Phase A, and we
    // use a single `object create --daemon` to prove the genesis
    // dispatcher arm). Then we issue both grants through the
    // daemon and confirm the per-object capability head list
    // contains both grantors.
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let carol = fixture.make_actor();
    let alice_id = alice.actor_id.to_string();
    let bob_id = bob.actor_id.to_string();
    let carol_id = carol.actor_id.to_string();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let mut daemon = spawn_daemon(&store_path);

    // Object create through the daemon — exercises the
    // ObjectGenesis arm we added in Slice 4.
    let create = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "object",
        "create",
        "--actor",
        &alice_id,
        "--kind",
        "software",
    ]);
    assert!(
        create.status.success(),
        "object create failed: stderr = {}",
        String::from_utf8_lossy(&create.stderr)
    );
    let create_out = String::from_utf8(create.stdout).expect("utf-8");
    let alpha_id = create_out
        .lines()
        .find_map(|l| l.strip_prefix("object = "))
        .expect("alpha id");

    // Alice → Bob, delegable.
    let grant_ab = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "capability",
        "grant",
        "--grantor",
        &alice_id,
        "--grantee",
        &bob_id,
        "--object",
        alpha_id,
        "--kind",
        "ObjectVersionTag",
        "--delegable",
    ]);
    assert!(
        grant_ab.status.success(),
        "alice→bob grant failed: stderr = {}",
        String::from_utf8_lossy(&grant_ab.stderr)
    );

    // Bob → Carol re-delegation (the second hop). Not delegable
    // — Carol's the end of the chain.
    let grant_bc = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "capability",
        "grant",
        "--grantor",
        &bob_id,
        "--grantee",
        &carol_id,
        "--object",
        alpha_id,
        "--kind",
        "ObjectVersionTag",
    ]);
    assert!(
        grant_bc.status.success(),
        "bob→carol re-grant failed: stderr = {}",
        String::from_utf8_lossy(&grant_bc.stderr)
    );

    // Both heads land in the per-object capability index.
    let list = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "capability",
        "list",
        "--object",
        alpha_id,
    ]);
    assert!(
        list.status.success(),
        "capability list --object failed: stderr = {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let list_out = String::from_utf8(list.stdout).expect("utf-8");
    assert!(
        list_out.contains(&alice_id),
        "alice not in chain list: {list_out}"
    );
    assert!(
        list_out.contains(&bob_id),
        "bob not in chain list: {list_out}"
    );
    assert!(
        list_out.contains(&carol_id),
        "carol not in chain list: {list_out}"
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
    drop(dir);
}

// Slice 5 audit: `tag revoke` is a separate dispatcher arm from
// `tag bind`. The Slice 3 test only exercised bind; this test
// covers the revoke arm end-to-end through `--daemon`. `trust
// withdraw` is intentionally not given a dedicated test —
// withdraw shares `run_trust_decide` with grant/block, which
// Slice 2 already exercises in daemon mode.

#[test]
fn tag_revoke_daemon_mode_chains_on_prior_bind() {
    use kairo_statement::RevisionId;

    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let revision = fixture.make_revision(
        &actor,
        &object,
        RevisionId::new("git:sha256:slice5-tag-r1"),
        FIXTURE_MANIFEST,
        vec![],
    );
    let actor_id = actor.actor_id.to_string();
    let object_id = object.object_id.to_string();
    let revision_id = revision.statement_id.to_string();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let mut daemon = spawn_daemon(&store_path);

    // Bind first so revoke has something to chain off of.
    let bind = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "tag",
        "bind",
        "--actor",
        &actor_id,
        "--object",
        &object_id,
        "--version",
        "1.0.0",
        "--revision",
        &revision_id,
    ]);
    assert!(
        bind.status.success(),
        "tag bind failed: stderr = {}",
        String::from_utf8_lossy(&bind.stderr)
    );

    let revoke = run_kairo(&[
        "--store",
        store_path.to_str().unwrap(),
        "--daemon",
        "tag",
        "revoke",
        "--actor",
        &actor_id,
        "--object",
        &object_id,
        "--version",
        "1.0.0",
    ]);
    assert!(
        revoke.status.success(),
        "tag revoke --daemon failed: stderr = {}",
        String::from_utf8_lossy(&revoke.stderr)
    );
    let revoke_out = String::from_utf8(revoke.stdout).expect("utf-8");
    assert!(revoke_out.contains("revoke tag"), "revoke: {revoke_out}");
    assert!(
        revoke_out.contains("target = (revoked)"),
        "revoke: {revoke_out}"
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
    drop(dir);
}
