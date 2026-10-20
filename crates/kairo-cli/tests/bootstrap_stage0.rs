//! The stage0-posix bootstrap, end to end (`specs/DECISIONS.md` §36):
//! `bootstrap/stage0-posix/import.sh` imports the seed and the sources
//! into a fresh store, `kairo build` runs the whole AMD64 chain under
//! the bubblewrap engine from the seeds alone, the artifact matches
//! upstream's `amd64.answers`, and `kairo reproduce` matches.
//!
//! Ignored by default: it needs a local checkout of the pinned upstream
//! (tests don't use the network), bubblewrap, and about a minute.
//!
//! ```text
//! git clone --branch Release_1.9.1 --recurse-submodules \
//!   https://github.com/oriansj/stage0-posix.git /path/to/stage0-posix
//! KAIRO_STAGE0_DIR=/path/to/stage0-posix \
//!   cargo test -p kairo-cli --test bootstrap_stage0 -- --ignored
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
use kairo_core as _;
use kairo_daemon as _;
use kairo_daemon_client as _;
use kairo_federation_core as _;
use kairo_federation_http as _;
use kairo_federation_service as _;
use kairo_git as _;
use kairo_identity as _;
use kairo_keystore as _;
use kairo_object as _;
use kairo_resolver as _;
use kairo_statement as _;
use kairo_store as _;
use kairo_test_support as _;
use kairo_web as _;
use nix as _;
use serde_json as _;
use tokio as _;

use std::path::{Path, PathBuf};
use std::process::Command;

const KAIRO: &str = env!("CARGO_BIN_EXE_kairo");

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

fn sha256(path: &Path) -> String {
    let output = Command::new("sha256sum")
        .arg(path)
        .output()
        .expect("sha256sum");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .expect("a digest")
        .to_owned()
}

#[test]
#[ignore = "needs KAIRO_STAGE0_DIR (a Release_1.9.1 checkout), bubblewrap, and about a minute"]
fn stage0_posix_bootstraps_from_the_seeds_under_the_engine() {
    let checkout = PathBuf::from(
        std::env::var_os("KAIRO_STAGE0_DIR")
            .expect("KAIRO_STAGE0_DIR names a stage0-posix checkout"),
    );
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
        .arg(&checkout)
        .arg(&actor)
        .arg("--store")
        .arg(store)
        .env("KAIRO", KAIRO)
        .output()
        .expect("run import.sh");
    let imported = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "import.sh failed:\n{imported}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let seed = field(&imported, "seed").to_owned();
    let sources = field(&imported, "sources").to_owned();
    let config = &imported[imported.find("[[runtime.seeds]]").expect("config lines")..];
    let snapshot_ref = format!("object:{sources}");
    let build = [
        "build",
        snapshot_ref.as_str(),
        "--target",
        "amd64",
        "--actor",
        actor.as_str(),
    ];

    // Without accepting the seed, the label is unsatisfiable.
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
        String::from_utf8_lossy(&refused.stderr).contains("is not an accepted seed"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );

    std::fs::write(store.join("config.toml"), config).expect("config");
    let built = kairo(store, &build);
    assert_eq!(field(&built, "status"), "success", "{built}");
    assert_eq!(field(&built, "environment"), "environment:linux/x86-64");
    assert_eq!(field(&built, "host_dependent"), "no");
    assert!(built.contains("executor = bubblewrap/1"), "{built}");
    let artifact = field(&built, "artifact").to_owned();

    // The artifact agrees with upstream's published hashes.
    let out = tempfile::TempDir::new().expect("tempdir");
    let mut files = std::collections::BTreeMap::new();
    for line in built.lines().filter(|line| line.starts_with("  bin/")) {
        let mut parts = line.split_whitespace();
        let path = parts.next().expect("path").trim_end_matches('*');
        let blob = parts.next().expect("blob");
        files.insert(path.to_owned(), blob.to_owned());
    }
    let answers = std::fs::read_to_string(checkout.join("amd64.answers")).expect("answers");
    let mut checked = 0;
    for line in answers.lines() {
        let (digest, path) = line.split_once("  ").expect("answers line");
        let path = path.replacen("AMD64/bin/", "bin/", 1);
        let blob = files
            .get(&path)
            .unwrap_or_else(|| panic!("{path} is not in the artifact"));
        let file = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "ls {}/blobs/*/*/{blob}* | head -n 1",
                store.display()
            ))
            .output()
            .expect("find blob");
        let file = String::from_utf8_lossy(&file.stdout).trim().to_owned();
        let copy = out.path().join(path.replace('/', "_"));
        std::fs::copy(&file, &copy).expect("copy blob");
        assert_eq!(sha256(&copy), digest, "{path}");
        checked += 1;
    }
    assert_eq!(checked, answers.lines().count());

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
    assert!(!seed.is_empty());
}
