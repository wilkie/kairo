//! live-bootstrap through its first musl toolchain for x86, end to end
//! (`specs/DECISIONS.md` §38): the stage0-posix wrapper is imported and
//! its x86 target built; `bootstrap/live-bootstrap/import.sh` archives
//! live-bootstrap and its distfiles and creates the wrapper; `kairo
//! build` runs live-bootstrap's own scripts under
//! `environment:linux-root/x86`, each step checking what it built
//! against upstream's checksums; the installed bash, and every package
//! the bash-run steps made, match the checksums upstream publishes,
//! read from the archived source; and `kairo reproduce` matches.
//!
//! Ignored by default: it needs the network (GitHub, Savannah, and the
//! distfile mirrors), bubblewrap on an x86-64 kernel that runs i386, and
//! several minutes.
//!
//! ```text
//! cargo test -p kairo-cli --test bootstrap_live -- --ignored
//! ```

#![allow(clippy::expect_used, clippy::panic)]

// Integration tests inherit kairo-cli's full prod-dep set;
// silence the lint for ones the test doesn't reference directly.
use base64 as _;
use clap as _;
use ed25519_dalek as _;
use http_body_util as _;
use hyper as _;
use hyper_util as _;
use kairo_build as _;
use kairo_bundle as _;
use kairo_config as _;
use kairo_daemon as _;
use kairo_daemon_client as _;
use kairo_federation_core as _;
use kairo_federation_http as _;
use kairo_federation_service as _;
use kairo_identity as _;
use kairo_keystore as _;
use kairo_object as _;
use kairo_resolver as _;
use kairo_statement as _;
use kairo_test_support as _;
use kairo_web as _;
use nix as _;
use serde_json as _;
use sha2 as _;
use tokio as _;
use ureq as _;

use std::path::Path;
use std::process::Command;

const KAIRO: &str = env!("CARGO_BIN_EXE_kairo");
const LIVE_BOOTSTRAP_URL: &str = "https://github.com/fosslinux/live-bootstrap";
const LIVE_BOOTSTRAP_COMMIT: &str = "b1ceced7ea8a819a26f23796f6dd7ae8496a33a4";

fn kairo(store: &Path, args: &[&str]) -> String {
    let output = Command::new(KAIRO)
        .arg("--store")
        .arg(store)
        .args(args)
        .output()
        .expect("run kairo");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "kairo {args:?} failed:\n{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

fn field<'a>(text: &'a str, name: &str) -> &'a str {
    let prefix = format!("{name} = ");
    text.lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("no {name} in:\n{text}"))
}

/// Run a wrapper's `import.sh`; its stdout.
fn import(store: &Path, actor: &str, wrapper: &str) -> String {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../bootstrap")
        .join(wrapper)
        .join("import.sh");
    let output = Command::new(&script)
        .arg(actor)
        .arg("--store")
        .arg(store)
        .env("KAIRO", KAIRO)
        .output()
        .expect("run import.sh");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "{wrapper}/import.sh failed:\n{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
#[ignore = "needs the network, bubblewrap on a kernel that runs i386, and several minutes"]
fn live_bootstrap_builds_a_musl_toolchain_from_the_stage0_artifact_under_the_engine() {
    let store_dir = tempfile::TempDir::new().expect("tempdir");
    let store = store_dir.path();
    let actor = field(
        &kairo(
            store,
            &[
                "actor",
                "create",
                "--kind",
                "person",
                "--generate-attestation-key",
            ],
        ),
        "actor",
    )
    .to_owned();

    // stage0-posix's x86 tools, this build's input.
    let stage0 = import(store, &actor, "stage0-posix");
    let config = &stage0[stage0.find("[[runtime.seeds]]").expect("config lines")..];
    std::fs::write(store.join("config.toml"), config).expect("config");
    let stage0_wrapper = format!("object:{}", field(&stage0, "wrapper"));
    let stage0_built = kairo(
        store,
        &[
            "build",
            &stage0_wrapper,
            "--target",
            "x86",
            "--actor",
            &actor,
        ],
    );
    assert_eq!(field(&stage0_built, "status"), "success", "{stage0_built}");

    let imported = import(store, &actor, "live-bootstrap");
    let wrapper = format!("object:{}", field(&imported, "wrapper"));
    let built = kairo(
        store,
        &["build", &wrapper, "--target", "x86", "--actor", &actor],
    );
    assert_eq!(field(&built, "status"), "success", "{built}");
    assert_eq!(field(&built, "environment"), "environment:linux-root/x86");
    assert_eq!(field(&built, "host_dependent"), "no");
    let artifact = field(&built, "artifact").to_owned();

    // The installed tcc is the one upstream's checksum names, as the
    // archived live-bootstrap source holds it.
    let source = field(
        &kairo(store, &["source", "id", LIVE_BOOTSTRAP_URL]),
        "source",
    )
    .to_owned();
    let cache = kairo_git::GitCache::open(store.join("git")).expect("cache");
    let repo = kairo_git::open(&cache.path_for(&source).expect("path")).expect("repo");
    let commit =
        kairo_git::GitRevision::new(kairo_git::GitObjectFormat::Sha1, LIVE_BOOTSTRAP_COMMIT)
            .expect("revision");
    let blob_store = kairo_store::FilesystemStore::open(store).expect("store");
    // The kaem-built bash, which nothing later replaces; the kaem-built
    // tcc is replaced by the musl one, whose packages are checked below
    // (each step checked its own outputs as it ran).
    for checksums in ["steps/bash-2.05b/bash-2.05b.checksums"] {
        let listed = String::from_utf8(
            repo.read_blob_at_path(&commit, checksums)
                .expect("read")
                .expect("the checksums in the commit"),
        )
        .expect("utf8");
        for line in listed.lines() {
            let (digest, path) = line.split_once("  ").expect("a checksum line");
            let path = path.strip_prefix('/').expect("an absolute path");
            let blob = built
                .lines()
                .find_map(|line| {
                    let mut parts = line.split_whitespace();
                    (parts.next()?.trim_end_matches('*') == path).then(|| parts.next())?
                })
                .unwrap_or_else(|| panic!("{path} is not in the artifact"));
            let bytes = kairo_store::BlobStore::get_blob(
                &blob_store,
                &kairo_core::BlobId::new(blob).expect("blob id"),
            )
            .expect("blob");
            assert_eq!(sha256(&bytes), digest, "{path}");
        }
    }

    // Every package the bash-run steps made is the one upstream's
    // SHA256SUMS.pkgs names.
    let sums = String::from_utf8(
        repo.read_blob_at_path(&commit, "steps/SHA256SUMS.pkgs")
            .expect("read")
            .expect("SHA256SUMS.pkgs in the commit"),
    )
    .expect("utf8");
    let packages: Vec<(&str, &str)> = built
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let path = parts.next()?.strip_prefix("repo/")?;
            Some((path, parts.next()?))
        })
        .collect();
    assert_eq!(packages.len(), 9, "{built}");
    for (package, blob) in packages {
        let digest = sums
            .lines()
            .find_map(|line| {
                let (digest, name) = line.split_once("  ")?;
                (name == package).then_some(digest)
            })
            .unwrap_or_else(|| panic!("{package} isn't in SHA256SUMS.pkgs"));
        let bytes = kairo_store::BlobStore::get_blob(
            &blob_store,
            &kairo_core::BlobId::new(blob).expect("blob id"),
        )
        .expect("blob");
        assert_eq!(sha256(&bytes), digest, "{package}");
    }

    // Another run of the same plan gives the same artifact.
    let result = field(&built, "result").to_owned();
    let reproduced = kairo(
        store,
        &[
            "reproduce",
            &format!("statement:{result}"),
            "--actor",
            &actor,
        ],
    );
    assert_eq!(
        field(&reproduced, "reproduction"),
        "matches",
        "{reproduced}"
    );
    assert_eq!(field(&reproduced, "reproduced_artifact"), artifact);
}
