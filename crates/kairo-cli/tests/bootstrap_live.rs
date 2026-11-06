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
//! With `KAIRO_LIVE_BOOTSTRAP_PASSES` set to 2 to 6, it then builds the
//! later passes (`specs/DECISIONS.md` §49), each from the previous one's
//! root, and checks their packages and children too: pass 2, the perl
//! chain (`bootstrap/live-bootstrap-perl/`), and pass 3, to gcc 4.7.4
//! (`bootstrap/live-bootstrap-gcc-4.7.4/`), after which the hello
//! consumer's dynamic target builds with gcc 4.7.4 against the shared
//! musl; at 4, pass 4, python and guile
//! (`bootstrap/live-bootstrap-python/`); and at 5, pass 5, gcc 10.5.0
//! (`bootstrap/live-bootstrap-gcc-10.5.0/`), with which hello builds
//! again; and at 6, pass 6, gcc 15.2.0 to the end of upstream's CI pass
//! 2 (`bootstrap/live-bootstrap-gcc-15.2.0/`), and hello with it.
//!
//! The wrappers' manifests name the stage before them by its object id
//! in the archive's canonical store (`specs/DECISIONS.md` §59, §61); a
//! fresh store's imports get other ids. So the test imports a copy of
//! `bootstrap/` in which, before each import, the canonical ids of the
//! wrappers it has imported are replaced by the ids they got here, and
//! re-creates the hello consumer the same way after each pass.
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

/// The last `lines` lines of the guest log blob named by `field` (`stdout`
/// or `stderr`) in a build's output, read from the store.
fn guest_log(store: &Path, output: &str, field: &str, lines: usize) -> String {
    let prefix = format!("{field} = ");
    output
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .and_then(|blob| kairo_core::BlobId::new(blob).ok())
        .and_then(|blob| {
            let store = kairo_store::FilesystemStore::open(store).ok()?;
            kairo_store::BlobStore::get_blob(&store, &blob).ok()
        })
        .map(|bytes| {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let all: Vec<&str> = text.lines().collect();
            all[all.len().saturating_sub(lines)..].join("\n")
        })
        .unwrap_or_default()
}

/// `kairo build <wrapper> --target x86`.
fn build(store: &Path, wrapper: &str, actor: &str) -> String {
    build_target(store, wrapper, "x86", actor)
}

/// `kairo build <wrapper> --target <target>`; on failure, panics with
/// the end of the guest's stdout and stderr, read from the store, since
/// the summary alone doesn't say which step failed (kaem's `--verbose`
/// trace goes to stdout).
fn build_target(store: &Path, wrapper: &str, target: &str, actor: &str) -> String {
    let output = Command::new(KAIRO)
        .arg("--store")
        .arg(store)
        .args(["build", wrapper, "--target", target, "--actor", actor])
        .output()
        .expect("run kairo");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        return stdout;
    }
    let summary: String = stdout
        .lines()
        .filter(|line| !line.starts_with("  usr/") && !line.starts_with("  repo/"))
        .collect::<Vec<_>>()
        .join("\n");
    panic!(
        "kairo build {wrapper} failed:\n{summary}\n{}\n--- guest stdout, last lines ---\n{}\n--- guest stderr, last lines ---\n{}",
        String::from_utf8_lossy(&output.stderr),
        guest_log(store, &stdout, "stdout", 120),
        guest_log(store, &stdout, "stderr", 80),
    );
}

/// Run a wrapper's `import.sh`; its stdout.
/// Each wrapper's object id in the archive's canonical store, which the
/// manifests after it name (`specs/DECISIONS.md` §59, §61).
const CANONICAL: [(&str, &str); 8] = [
    (
        "stage0-posix",
        "zQmUovNuu85b7r5osUhLMKA1WdKNhAG5JcG8dR9iPPLJBte",
    ),
    (
        "live-bootstrap",
        "zQmbn8aYLLJJkzCj8wcmTCHUhycMNozG3vWNXxnDrQRziAQ",
    ),
    (
        "linux-filesystem",
        "zQmPdhpRFdK8r1tAKXivvrayNW7ME4iacqg8Gt92zmyKZxa",
    ),
    (
        "live-bootstrap-perl",
        "zQmRcSie6cMMJSCkJv9tJnkuF1HMSpwqzJQNjJiSLJmAFXu",
    ),
    (
        "live-bootstrap-gcc-4.7.4",
        "zQmRJS223L2J8WGQKQZwCjQh5r1NbMiLmNyQf1vc3QtM3eR",
    ),
    (
        "live-bootstrap-python",
        "zQmULyVeK4Tmvq3sNAh69bdXwUeKXYUbP27cSva9QgJUA4X",
    ),
    (
        "live-bootstrap-gcc-10.5.0",
        "zQmUUoeihepsPtywUtHvfu2s6PFV6fFw3YQg8AALvcSo35V",
    ),
    (
        "live-bootstrap-gcc-15.2.0",
        "zQmNRiLHtCnvdDer5i5GRqPxzwjDoioZW7oxVSeudCKnNBk",
    ),
];

/// The bootstrap's wrappers, copied, so that a fresh store can import
/// them: the manifests name the stage before them by its id in the
/// canonical store, and a fresh import of that stage gets another. Before
/// each import, every canonical id whose wrapper this test has imported
/// is replaced, in the copy, by the id its import here got.
struct Wrappers {
    dir: tempfile::TempDir,
    fresh: Vec<(&'static str, String)>,
}

impl Wrappers {
    fn new() -> Self {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bootstrap");
        let copied = Command::new("cp")
            .arg("-a")
            .arg(&source)
            .arg(dir.path().join("bootstrap"))
            .status()
            .expect("copy the wrappers");
        assert!(copied.success(), "copying {} failed", source.display());
        Self {
            dir,
            fresh: Vec::new(),
        }
    }

    fn root(&self) -> std::path::PathBuf {
        self.dir.path().join("bootstrap")
    }

    /// Every manifest in the copy, with the canonical ids imported so far
    /// replaced by their fresh ones.
    fn rewrite(&self) {
        fn manifests(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("read the copy") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    manifests(&path, found);
                } else if path.file_name().is_some_and(|name| name == "kairo.toml") {
                    found.push(path);
                }
            }
        }
        let mut found = Vec::new();
        manifests(&self.root(), &mut found);
        for path in found {
            let text = std::fs::read_to_string(&path).expect("manifest");
            let mut rewritten = text.clone();
            for (canonical, fresh) in &self.fresh {
                rewritten = rewritten.replace(canonical, fresh);
            }
            if rewritten != text {
                std::fs::write(&path, rewritten).expect("rewrite manifest");
            }
        }
    }

    /// Run `wrapper`'s import.sh from the copy and note the id it made.
    fn import(&mut self, store: &Path, actor: &str, wrapper: &str) -> String {
        self.rewrite();
        let script = self.root().join(wrapper).join("import.sh");
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
        let made = stdout
            .lines()
            .find_map(|line| {
                line.strip_prefix("wrapper = ")
                    .or_else(|| line.strip_prefix("object = "))
            })
            .unwrap_or_else(|| panic!("{wrapper}/import.sh printed no id:\n{stdout}"))
            .to_owned();
        if let Some((_, canonical)) = CANONICAL.iter().find(|(name, _)| *name == wrapper) {
            self.fresh.push((canonical, made));
        }
        stdout
    }

    /// Create the hello consumer from the copy, or, given its id, its next
    /// revision, naming the passes imported so far by their ids here.
    fn hello(&self, store: &Path, actor: &str, consumer: Option<&str>) -> String {
        self.rewrite();
        let mut command = Command::new("bash");
        command
            .arg(self.root().join("live-bootstrap/hello/create.sh"))
            .arg(actor);
        if let Some(consumer) = consumer {
            command.arg("--wrapper").arg(consumer);
        }
        let created = command
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
        field(&String::from_utf8_lossy(&created.stdout), "object").to_owned()
    }
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The test's store: removed afterwards, or kept.
enum KeptOrTemp {
    Temp(tempfile::TempDir),
    Kept(std::path::PathBuf),
}

impl KeptOrTemp {
    fn path(&self) -> &Path {
        match self {
            Self::Temp(dir) => dir.path(),
            Self::Kept(path) => path,
        }
    }
}

#[test]
#[ignore = "needs the network, bubblewrap on a kernel that runs i386, and several minutes"]
fn live_bootstrap_builds_into_its_ci_pass_2_from_the_stage0_artifact_under_the_engine() {
    let store_dir = tempfile::TempDir::new().expect("tempdir");
    // KAIRO_KEEP_TEST_STORE keeps the store afterwards, to look at a
    // failure's execution record and workspace.
    let store_dir = if std::env::var_os("KAIRO_KEEP_TEST_STORE").is_some() {
        let kept = store_dir.keep();
        eprintln!("keeping the test store at {}", kept.display());
        KeptOrTemp::Kept(kept)
    } else {
        KeptOrTemp::Temp(store_dir)
    };
    let store = store_dir.path();
    let mut wrappers = Wrappers::new();
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
    let stage0 = wrappers.import(store, &actor, "stage0-posix");
    let config = &stage0[stage0.find("[[runtime.seeds]]").expect("config lines")..];
    // The longest pass runs longer than the default hour (gcc 15.2.0's
    // three-stage bootstrap, in pass 6, for hours), and the Linux
    // tarball can take longer than the default half hour to download.
    std::fs::write(
        store.join("config.toml"),
        format!(
            "[runtime]\ntimeout_secs = 28800\n\n\
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

    let imported = wrappers.import(store, &actor, "live-bootstrap");
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
    assert_eq!(check_packages(&built, &sums, &blob_store), 77, "{built}");

    // Each package is a child object, its artifact the package's own
    // files (DECISIONS.md §41 sub-decision 6): bash's is bash alone.
    // musl's packages are split into the parts linking needs (§51).
    assert_eq!(field(&built, "children"), "83", "{built}");
    assert_eq!(
        child_paths(&built, "bash-5.2.15_0", &blob_store),
        ["usr/bin/bash", "usr/bin/sh"]
    );

    // A consumer names tool:gcc and, as gcc obliges it to (DECISIONS.md
    // §51), the musl it links against: binutils and musl's and the
    // kernel's headers come as what gcc requires (§46), and the program
    // it builds runs. Pass 1's musl is static only, so the static target.
    let filesystem = wrappers.import(store, &actor, "linux-filesystem");
    let filesystem = field(&filesystem, "object").to_owned();
    let consumer = wrappers.hello(store, &actor, None);
    let built = build_target(store, &format!("object:{consumer}"), "x86-static", &actor);
    assert_eq!(field(&built, "status"), "success", "{built}");
    assert_eq!(field(&built, "provisioning"), "overlay", "{built}");
    let plan = |target: &str| {
        kairo(
            store,
            &[
                "build",
                &format!("object:{consumer}"),
                "--target",
                target,
                "--plan-only",
            ],
        )
    };
    let planned = plan("x86-static");
    for label in [
        "tool:binutils",
        "lib:musl:headers 1.2.5",
        "lib:linux:headers",
    ] {
        assert!(
            planned.contains(&format!("  {label}")) && planned.contains("via tool:gcc"),
            "{planned}"
        );
    }
    // Linked statically, it requires nothing at run time.
    assert!(!planned.contains("\nrequires = "), "{planned}");
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

    // The later passes (DECISIONS.md §49), as many as asked for: each
    // goes on from the previous pass's whole root, which it doesn't build
    // again.
    let passes: u32 = std::env::var("KAIRO_LIVE_BOOTSTRAP_PASSES")
        .ok()
        .map_or(1, |passes| passes.parse().expect("a number of passes"));
    if passes < 2 {
        return;
    }
    let imported = wrappers.import(store, &actor, "live-bootstrap-perl");
    let perl = format!("object:{}", field(&imported, "wrapper"));
    let built = build(store, &perl, &actor);
    assert_eq!(field(&built, "status"), "success", "{built}");
    assert!(!built.contains("\nbuilds = "), "{built}");
    // Every package so far, pass 1's and its own, is upstream's.
    assert_eq!(check_packages(&built, &sums, &blob_store), 107, "{built}");
    // Its children are its own packages.
    assert_eq!(field(&built, "children"), "30", "{built}");
    assert!(child_paths(&built, "perl-5.42.0_0", &blob_store).contains(&"usr/bin/perl".to_owned()));
    if passes < 3 {
        return;
    }

    // Pass 3, to gcc 4.7.4, with C++.
    let imported = wrappers.import(store, &actor, "live-bootstrap-gcc-4.7.4");
    let gcc = format!("object:{}", field(&imported, "wrapper"));
    let built = build(store, &gcc, &actor);
    assert_eq!(field(&built, "status"), "success", "{built}");
    assert!(!built.contains("\nbuilds = "), "{built}");
    assert_eq!(check_packages(&built, &sums, &blob_store), 114, "{built}");
    assert_eq!(field(&built, "children"), "9", "{built}");
    let paths = child_paths(&built, "gcc-4.7.4_0", &blob_store);
    for program in ["usr/bin/gcc", "usr/bin/g++"] {
        assert!(paths.contains(&program.to_owned()), "{paths:?}");
    }
    // The consumer's dynamic target, now that a shared musl is held: it
    // gets the highest gcc, with the binutils built beside it, the shared
    // musl brings the merged-/usr layout its loader is reached through
    // (§50), and the artifact requires that musl at its range. hello
    // names the passes by their ids here, this one's too.
    wrappers.hello(store, &actor, Some(&consumer));
    let planned = plan("x86");
    assert!(
        planned.contains(&format!(
            "  data:linux-filesystem:merged-usr 1.0.0 via lib:musl:shared > data:linux-filesystem:merged-usr -> object:{filesystem} root files (content, installed at /)"
        )),
        "{planned}"
    );
    assert!(
        planned.contains(
            "\nrequires = 1\n  lib:musl:shared ^1.2.5 (conferred by lib:musl:shared from object:"
        ),
        "{planned}"
    );
    let built = build(store, &format!("object:{consumer}"), &actor);
    assert_eq!(field(&built, "status"), "success", "{built}");
    let out = built
        .lines()
        .find_map(|line| line.strip_prefix("  out "))
        .expect("the out file");
    let text = kairo_store::BlobStore::get_blob(
        &blob_store,
        &kairo_core::BlobId::new(out.trim()).expect("blob id"),
    )
    .expect("out");
    assert_eq!(String::from_utf8_lossy(&text), "hello from gcc 4.7.4\n");
    if passes < 4 {
        return;
    }

    // Pass 4: the python chain, guile, and autogen.
    let imported = wrappers.import(store, &actor, "live-bootstrap-python");
    let python = format!("object:{}", field(&imported, "wrapper"));
    let built = build(store, &python, &actor);
    assert_eq!(field(&built, "status"), "success", "{built}");
    assert!(!built.contains("\nbuilds = "), "{built}");
    assert_eq!(check_packages(&built, &sums, &blob_store), 137, "{built}");
    assert_eq!(field(&built, "children"), "23", "{built}");
    for (child, program) in [
        ("python-3.11.1_0", "usr/bin/python3.11"),
        ("guile-3.0.11_0", "usr/bin/guile"),
        ("autogen-5.18.16_0", "usr/bin/autogen"),
    ] {
        let paths = child_paths(&built, child, &blob_store);
        assert!(paths.contains(&program.to_owned()), "{child}: {paths:?}");
    }
    if passes < 5 {
        return;
    }

    // Pass 5: gcc 10.5.0, and binutils 2.41 rebuilt with it.
    let imported = wrappers.import(store, &actor, "live-bootstrap-gcc-10.5.0");
    let gcc = format!("object:{}", field(&imported, "wrapper"));
    let built = build(store, &gcc, &actor);
    assert_eq!(field(&built, "status"), "success", "{built}");
    assert!(!built.contains("\nbuilds = "), "{built}");
    assert_eq!(check_packages(&built, &sums, &blob_store), 139, "{built}");
    // gcc's compiler and its six shared runtime libraries (§58), and
    // binutils.
    assert_eq!(field(&built, "children"), "8", "{built}");
    let paths = child_paths(&built, "gcc-10.5.0_0", &blob_store);
    for program in ["usr/bin/gcc", "usr/bin/g++"] {
        assert!(paths.contains(&program.to_owned()), "{paths:?}");
    }
    // hello's dynamic target now builds with gcc 10.5.0.
    // hello names the passes by their ids here, this one's too.
    wrappers.hello(store, &actor, Some(&consumer));
    let built = build(store, &format!("object:{consumer}"), &actor);
    assert_eq!(field(&built, "status"), "success", "{built}");
    let out = built
        .lines()
        .find_map(|line| line.strip_prefix("  out "))
        .expect("the out file");
    let text = kairo_store::BlobStore::get_blob(
        &blob_store,
        &kairo_core::BlobId::new(out.trim()).expect("blob id"),
    )
    .expect("out");
    assert_eq!(String::from_utf8_lossy(&text), "hello from gcc 10.5.0\n");
    if passes < 6 {
        return;
    }

    // Pass 6, the last: gcc 15.2.0 and the rest of upstream's CI pass 2.
    let imported = wrappers.import(store, &actor, "live-bootstrap-gcc-15.2.0");
    let last = format!("object:{}", field(&imported, "wrapper"));
    let built = build(store, &last, &actor);
    assert_eq!(field(&built, "status"), "success", "{built}");
    assert!(!built.contains("\nbuilds = "), "{built}");
    assert_eq!(check_packages(&built, &sums, &blob_store), 149, "{built}");
    // Its 10 packages, gcc's six shared runtime libraries apart (§58).
    assert_eq!(field(&built, "children"), "16", "{built}");
    // hello names the passes by their ids here, this one's too.
    wrappers.hello(store, &actor, Some(&consumer));
    let built = build(store, &format!("object:{consumer}"), &actor);
    assert_eq!(field(&built, "status"), "success", "{built}");
    let out = built
        .lines()
        .find_map(|line| line.strip_prefix("  out "))
        .expect("the out file");
    let text = kairo_store::BlobStore::get_blob(
        &blob_store,
        &kairo_core::BlobId::new(out.trim()).expect("blob id"),
    )
    .expect("out");
    assert_eq!(String::from_utf8_lossy(&text), "hello from gcc 15.2.0\n");
}

/// Check every package a build lists in `repo/` against upstream's
/// `SHA256SUMS.pkgs`; how many there were.
fn check_packages(built: &str, sums: &str, blob_store: &kairo_store::FilesystemStore) -> usize {
    let packages: Vec<(&str, &str)> = built
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let path = parts.next()?.strip_prefix("repo/")?;
            Some((path, parts.next()?))
        })
        .collect();
    for (package, blob) in &packages {
        let digest = sums
            .lines()
            .find_map(|line| {
                let (digest, name) = line.split_once("  ")?;
                (name == *package).then_some(digest)
            })
            .unwrap_or_else(|| panic!("{package} isn't in SHA256SUMS.pkgs"));
        let bytes = kairo_store::BlobStore::get_blob(
            blob_store,
            &kairo_core::BlobId::new(*blob).expect("blob id"),
        )
        .expect("blob");
        assert_eq!(sha256(&bytes), digest, "{package}");
    }
    packages.len()
}

/// The paths in the artifact of the child a build lists as `name`.
fn child_paths(built: &str, name: &str, blob_store: &kairo_store::FilesystemStore) -> Vec<String> {
    let prefix = format!("  {name} object:");
    let artifact = built
        .lines()
        .find_map(|line| {
            let rest = line.strip_prefix(&prefix)?;
            rest.split_once(" (artifact ")
                .map(|(_, artifact)| artifact.trim_end_matches(')').to_owned())
        })
        .unwrap_or_else(|| panic!("no {name} child in:\n{built}"));
    let tree: serde_json::Value = serde_json::from_slice(
        &kairo_store::BlobStore::get_blob(
            blob_store,
            &kairo_core::BlobId::new(artifact).expect("blob id"),
        )
        .expect("the child's artifact"),
    )
    .expect("an artifact tree");
    tree["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter_map(|entry| entry["path"].as_str().map(str::to_owned))
        .collect()
}
