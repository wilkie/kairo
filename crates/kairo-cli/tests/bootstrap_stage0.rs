//! The stage0-posix bootstrap, end to end (`specs/DECISIONS.md` §36,
//! §37): `bootstrap/stage0-posix/import.sh` archives stage0-posix and,
//! recursively, every submodule it pins as source objects from their
//! real upstream URLs, and creates the native wrapper; `kairo build`
//! then runs the whole AMD64 chain, and the x86 one (§38 sub-decision
//! 7), under the bubblewrap engine from the seeds alone; each artifact
//! matches upstream's `<arch>.answers`, read from the archived source;
//! and `kairo reproduce` matches.
//!
//! Ignored by default: it needs the network (GitHub and Savannah),
//! bubblewrap, and a few minutes.
//!
//! ```text
//! cargo test -p kairo-cli --test bootstrap_stage0 -- --ignored
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
const STAGE0_URL: &str = "https://github.com/oriansj/stage0-posix";
const STAGE0_RELEASE: &str = "45d90f5955b6907dc6cdea9ebafce558359edcd3";

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

fn sha256(bytes: &[u8]) -> String {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("file");
    std::fs::write(&path, bytes).expect("write");
    let output = Command::new("sha256sum")
        .arg(&path)
        .output()
        .expect("sha256sum");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .expect("a digest")
        .to_owned()
}

#[test]
#[ignore = "needs the network (GitHub, Savannah), bubblewrap, and a few minutes"]
fn stage0_posix_bootstraps_from_archived_sources_under_the_engine() {
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

    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bootstrap/stage0-posix/import.sh");
    let output = Command::new(&script)
        .arg(&actor)
        .arg("--store")
        .arg(store)
        .env("KAIRO", KAIRO)
        .output()
        .expect("run import.sh");
    let imported = String::from_utf8_lossy(&output.stdout).into_owned();
    let log = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "import.sh failed:\n{imported}{log}"
    );
    assert!(log.contains("complete = yes"), "{log}");
    let wrapper = field(&imported, "wrapper").to_owned();
    let config = &imported[imported.find("[[runtime.seeds]]").expect("config lines")..];
    let snapshot_ref = format!("object:{wrapper}");
    let build = [
        "build",
        snapshot_ref.as_str(),
        "--target",
        "amd64",
        "--actor",
        actor.as_str(),
    ];

    // Without accepting the seed, the build is refused.
    std::fs::write(
        store.join("config.toml"),
        "[[runtime.engines]]\nkind = \"bubblewrap\"\npath = \"/usr/bin/bwrap\"\n",
    )
    .expect("config");
    let refused = Command::new(KAIRO)
        .arg("--store")
        .arg(store)
        .args(build)
        .output()
        .expect("run kairo");
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("not an accepted seed"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );

    std::fs::write(store.join("config.toml"), config).expect("config");
    let stage0 = field(&kairo(store, &["source", "id", STAGE0_URL]), "source").to_owned();
    let cache = kairo_git::GitCache::open(store.join("git")).expect("cache");
    let repo = kairo_git::open(&cache.path_for(&stage0).expect("path")).expect("repo");
    let release = kairo_git::GitRevision::new(kairo_git::GitObjectFormat::Sha1, STAGE0_RELEASE)
        .expect("revision");
    let blob_store = kairo_store::FilesystemStore::open(store).expect("store");
    let mut built_amd64 = String::new();
    for (target, dir, environment) in [
        ("amd64", "AMD64", "environment:linux/x86-64"),
        ("x86", "x86", "environment:linux/x86"),
    ] {
        let built = kairo(
            store,
            &[
                "build",
                snapshot_ref.as_str(),
                "--target",
                target,
                "--actor",
                actor.as_str(),
            ],
        );
        assert_eq!(field(&built, "status"), "success", "{built}");
        assert_eq!(field(&built, "environment"), environment);
        assert_eq!(field(&built, "host_dependent"), "no");
        assert!(built.contains("executor = bubblewrap/1"), "{built}");

        // The artifact agrees with upstream's published hashes, as the
        // archived stage0-posix source holds them.
        let answers = String::from_utf8(
            repo.read_blob_at_path(&release, &format!("{target}.answers"))
                .expect("read")
                .expect("the answers in the release"),
        )
        .expect("utf8");
        let mut files = std::collections::BTreeMap::new();
        for line in built.lines().filter(|line| line.starts_with("  bin/")) {
            let mut parts = line.split_whitespace();
            let path = parts.next().expect("path").trim_end_matches('*').to_owned();
            let blob = kairo_core::BlobId::new(parts.next().expect("blob")).expect("blob id");
            files.insert(path, blob);
        }
        for line in answers.lines() {
            let (digest, path) = line.split_once("  ").expect("answers line");
            let path = path.replacen(&format!("{dir}/bin/"), "bin/", 1);
            let blob = files
                .get(&path)
                .unwrap_or_else(|| panic!("{target}: {path} is not in the artifact"));
            let bytes = kairo_store::BlobStore::get_blob(&blob_store, blob).expect("blob");
            assert_eq!(sha256(&bytes), digest, "{target}: {path}");
        }
        if target == "amd64" {
            built_amd64 = built;
        }
    }
    let built = built_amd64;
    let artifact = field(&built, "artifact").to_owned();

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
