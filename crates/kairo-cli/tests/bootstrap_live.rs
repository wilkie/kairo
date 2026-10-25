//! live-bootstrap into upstream's CI pass 2 for x86, end to end
//! (`specs/DECISIONS.md` §38): the stage0-posix wrapper is imported and
//! its x86 target built; `bootstrap/live-bootstrap/import.sh` archives
//! live-bootstrap and its distfiles and creates the wrapper; `kairo
//! build` runs live-bootstrap's own scripts under
//! `environment:linux/x86`, each step checking what it built
//! against upstream's checksums; every package the bash-run steps made
//! matches the checksums upstream publishes, read from the archived
//! source; and `kairo reproduce` matches.
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

/// `kairo build <wrapper> --target x86`; on failure, panics with the
/// end of the guest's stderr, read from the store, since the summary
/// alone doesn't say which step failed.
fn build(store: &Path, wrapper: &str, actor: &str) -> String {
    let output = Command::new(KAIRO)
        .arg("--store")
        .arg(store)
        .args(["build", wrapper, "--target", "x86", "--actor", actor])
        .output()
        .expect("run kairo");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        return stdout;
    }
    let guest = stdout
        .lines()
        .find_map(|line| line.strip_prefix("stderr = "))
        .and_then(|blob| kairo_core::BlobId::new(blob).ok())
        .and_then(|blob| {
            let store = kairo_store::FilesystemStore::open(store).ok()?;
            kairo_store::BlobStore::get_blob(&store, &blob).ok()
        })
        .map(|bytes| {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let lines: Vec<&str> = text.lines().collect();
            lines[lines.len().saturating_sub(80)..].join("\n")
        })
        .unwrap_or_default();
    let summary: String = stdout
        .lines()
        .filter(|line| !line.starts_with("  usr/") && !line.starts_with("  repo/"))
        .collect::<Vec<_>>()
        .join("\n");
    panic!(
        "kairo build {wrapper} failed:\n{summary}\n{}\n--- guest stderr, last lines ---\n{guest}",
        String::from_utf8_lossy(&output.stderr)
    );
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
fn live_bootstrap_builds_into_its_ci_pass_2_from_the_stage0_artifact_under_the_engine() {
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
    // The full chain runs longer than the default hour, and the Linux
    // tarball can take longer than the default half hour to download.
    std::fs::write(
        store.join("config.toml"),
        format!(
            "[runtime]\ntimeout_secs = 14400\n\n\
             [federation.limits]\nfile_timeout_secs = 7200\n\n{config}"
        ),
    )
    .expect("config");
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
    let built = build(store, &wrapper, &actor);
    assert_eq!(field(&built, "status"), "success", "{built}");
    assert_eq!(field(&built, "environment"), "environment:linux/x86");
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
    // The kaem-run steps' binaries (tcc, bash 2.05b) are replaced by
    // later builds, so the final tree can't be checked against their
    // checksums; each step checked its own outputs as it ran. What the
    // final artifact can be checked against is every package.
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
    assert_eq!(packages.len(), 77, "{built}");
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

    // Each package is a child object, its artifact the package's own
    // files (DECISIONS.md §41 sub-decision 6): bash's is bash alone.
    assert_eq!(field(&built, "children"), "77", "{built}");
    let bash = built
        .lines()
        .find_map(|line| {
            let rest = line.strip_prefix("  bash-5.2.15_0 object:")?;
            rest.split_once(" (artifact ")
                .map(|(_, artifact)| artifact.trim_end_matches(')').to_owned())
        })
        .unwrap_or_else(|| panic!("no bash child in:\n{built}"));
    let tree: serde_json::Value = serde_json::from_slice(
        &kairo_store::BlobStore::get_blob(
            &blob_store,
            &kairo_core::BlobId::new(bash).expect("blob id"),
        )
        .expect("the bash child's artifact"),
    )
    .expect("an artifact tree");
    let paths: Vec<&str> = tree["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter_map(|entry| entry["path"].as_str())
        .collect();
    assert_eq!(paths, ["usr/bin/bash", "usr/bin/sh"]);

    // A consumer names tool:gcc alone: binutils, musl, and the kernel
    // headers come as what gcc requires (DECISIONS.md §46), and the
    // program it builds runs.
    let hello = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bootstrap/live-bootstrap/hello");
    let created = Command::new("bash")
        .arg(hello.join("create.sh"))
        .arg(&actor)
        .arg("--store")
        .arg(store)
        .env("KAIRO", KAIRO)
        .output()
        .expect("run create.sh");
    assert!(
        created.status.success(),
        "create.sh failed:\n{}{}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr)
    );
    let consumer = field(&String::from_utf8_lossy(&created.stdout), "object").to_owned();
    let built = build(store, &format!("object:{consumer}"), &actor);
    assert_eq!(field(&built, "status"), "success", "{built}");
    assert_eq!(field(&built, "provisioning"), "overlay", "{built}");
    let plan = kairo(
        store,
        &[
            "build",
            &format!("object:{consumer}"),
            "--target",
            "x86",
            "--plan-only",
        ],
    );
    for label in ["tool:binutils", "lib:musl 1.2.5", "lib:linux:headers"] {
        assert!(
            plan.contains(&format!("  {label}")) && plan.contains("via tool:gcc"),
            "{plan}"
        );
    }
    let out = built
        .lines()
        .find_map(|line| line.strip_prefix("  out "))
        .expect("the out file");
    let text = kairo_store::BlobStore::get_blob(
        &blob_store,
        &kairo_core::BlobId::new(out.trim()).expect("blob id"),
    )
    .expect("out");
    assert_eq!(String::from_utf8_lossy(&text), "hello from gcc 4.0.4\n");

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
