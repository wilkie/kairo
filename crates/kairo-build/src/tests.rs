use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kairo_core::{BlobId, ObjectId, SnapshotId};
use kairo_object::ObjectManifest;
use kairo_resolver::{
    Host, HostTool, ManifestSource, NoArtifacts, PlanDocument, Resolver, SnapshotSelector,
};
use kairo_statement::RevisionId;
use kairo_store::BlobStore;
use kairo_test_support::store::{CreatedActor, CreatedObject, StoreFixture};

use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Trees by object: a `kairo.toml` plus extra files, standing in for
/// the Git cache.
#[derive(Default)]
struct Trees(HashMap<String, Vec<(String, String)>>);

impl ManifestSource for Trees {
    fn manifest(
        &self,
        object: &ObjectId,
        _revision: &RevisionId,
    ) -> Result<Option<ObjectManifest>, SourceError> {
        self.0
            .get(&object.to_string())
            .and_then(|files| files.iter().find(|(name, _)| name == "kairo.toml"))
            .map(|(_, text)| {
                ObjectManifest::parse_toml(text).map_err(|e| SourceError(e.to_string()))
            })
            .transpose()
    }
}

impl TreeSource for Trees {
    fn export(
        &self,
        object: &ObjectId,
        _snapshot: &SnapshotId,
        dest: &Path,
    ) -> Result<bool, SourceError> {
        let Some(files) = self.0.get(&object.to_string()) else {
            return Ok(false);
        };
        for (name, contents) in files {
            let path = dest.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| SourceError(e.to_string()))?;
            }
            // `@copy:<host path>` copies an executable in; `@link:<target>`
            // makes a symbolic link, as a Git tree can.
            if let Some(from) = contents.strip_prefix("@copy:") {
                fs::copy(from, &path).map_err(|e| SourceError(e.to_string()))?;
                set_executable(&path, true).map_err(|e| SourceError(e.to_string()))?;
                continue;
            }
            if let Some(to) = contents.strip_prefix("@link:") {
                std::os::unix::fs::symlink(to, &path).map_err(|e| SourceError(e.to_string()))?;
                continue;
            }
            fs::write(&path, contents).map_err(|e| SourceError(e.to_string()))?;
            if name.ends_with(".sh") {
                set_executable(&path, true).map_err(|e| SourceError(e.to_string()))?;
            }
        }
        Ok(true)
    }
}

struct World {
    _dir: tempfile::TempDir,
    fixture: StoreFixture,
    actor: CreatedActor,
    trees: Trees,
    revisions: u32,
}

impl World {
    fn new() -> Self {
        let (dir, fixture) = StoreFixture::temp();
        let actor = fixture.make_actor();
        Self {
            _dir: dir,
            fixture,
            actor,
            trees: Trees::default(),
            revisions: 0,
        }
    }

    fn object(&mut self, files: &[(&str, &str)]) -> CreatedObject {
        let object = self.fixture.make_object(&self.actor, "software");
        self.revisions += 1;
        let manifest = files
            .iter()
            .find(|(name, _)| *name == "kairo.toml")
            .map(|(_, text)| *text)
            .expect("kairo.toml");
        let revision = self.fixture.make_revision(
            &self.actor,
            &object,
            RevisionId::new(format!("git:sha256:r{}", self.revisions)),
            manifest,
            Vec::new(),
        );
        self.fixture
            .set_branch(&self.actor, &object, &revision, "head");
        self.trees.0.insert(
            object.object_id.to_string(),
            files
                .iter()
                .map(|(name, text)| ((*name).to_owned(), (*text).to_owned()))
                .collect(),
        );
        object
    }

    fn plan_with(&self, host: &Host, object: &ObjectId, target: &str) -> PlanDocument {
        Resolver::new(&self.fixture.store, &self.trees, &NoArtifacts, host, &[])
            .resolve(object, &SnapshotSelector::Current, target)
            .expect("plan")
            .plan
    }

    fn plan(&self, object: &ObjectId, target: &str) -> PlanDocument {
        let host = Host {
            system: "linux".to_owned(),
            arch: "x86-64".to_owned(),
            engines: Vec::new(),
        };
        // Development mode: `sh` is a declared host tool, so it is the
        // only thing on PATH.
        let tools = [HostTool {
            provides: "tool:sh".to_owned(),
            version: None,
            path: PathBuf::from("/bin/sh"),
        }];
        Resolver::new(
            &self.fixture.store,
            &self.trees,
            &NoArtifacts,
            &host,
            &tools,
        )
        .resolve(object, &SnapshotSelector::Current, target)
        .expect("plan")
        .plan
    }

    fn run(&self, plan: &PlanDocument, timeout: Duration) -> Result<Execution, BuildError> {
        run_plan(
            &self.fixture.store,
            &self.trees,
            plan,
            1_700_000_000,
            &Limits { timeout },
            &Engine::Native,
        )
    }

    fn blob_text(&self, id: &str) -> String {
        let bytes = self
            .fixture
            .store
            .get_blob(&BlobId::new(id).expect("id"))
            .expect("blob");
        String::from_utf8(bytes).expect("utf8")
    }
}

fn app_manifest(command: &str, outputs: &str, deps: &str) -> String {
    format!(
        r#"
        [kairo]
        schema = 1
        kind = "software"
        name = "app"
        {deps}
        [[build.targets]]
        name = "release"
        command = {command}
        [[build.targets.dependencies]]
        kind = "provides"
        provides = "tool:sh"
        {outputs}
        "#
    )
}

const FILE_AND_DIR_OUTPUTS: &str = r#"
    [[build.targets.outputs]]
    name = "cli"
    path = "out/app"
    artifact_path = "bin/app"
    kind = "file"
    [[build.targets.outputs]]
    name = "share"
    path = "out/share"
    artifact_path = "share"
    kind = "directory"
"#;

#[test]
fn a_build_collects_outputs_into_an_artifact_tree() -> TestResult {
    let mut world = World::new();
    let lib = world.object(&[
        (
            "kairo.toml",
            r#"
            [kairo]
            schema = 1
            kind = "data"
            name = "lib"
            [[provides]]
            provides = "lib:greeting:source"
            version = "1.0"
            "#,
        ),
        ("greeting.txt", "hello"),
    ]);
    let app = world.object(&[
        (
            "kairo.toml",
            &app_manifest(
                r#"["sh", "build.sh"]"#,
                FILE_AND_DIR_OUTPUTS,
                r#"
                [[dependencies]]
                kind = "provides"
                provides = "lib:greeting:source"
                "#,
            ),
        ),
        (
            "build.sh",
            "#!/bin/sh\nset -e\nmkdir -p out/share/nested\n\
             printf '#!/bin/sh\\necho app\\n' > out/app\nchmod +x out/app\n\
             cat \"$KAIRO_PROVIDER_LIB_GREETING_SOURCE/greeting.txt\" > out/share/greeting\n\
             echo \"$SOURCE_DATE_EPOCH\" > out/share/nested/epoch\n\
             env | sort > out/share/env\n",
        ),
    ]);

    let plan = world.plan(&app.object_id, "release");
    let execution = world.run(&plan, Duration::from_secs(30))?;
    let record = &execution.record;
    assert_eq!(record.status, Status::Success, "{:?}", record.diagnostics);
    assert_eq!(record.exit_code, Some(0));
    assert_eq!(record.plan, plan.hash.to_string());
    assert_eq!(record.executor.id, "native");
    assert!(record.host_dependent);
    assert_eq!(record.environment, "none");
    assert_eq!(record.outputs.len(), 2);
    assert_eq!(record.outputs[0].files, 1);
    assert_eq!(record.outputs[1].files, 3);

    let tree = execution.artifact.as_ref().expect("artifact");
    let paths: Vec<&str> = tree.entries.iter().map(|e| e.path()).collect();
    assert_eq!(
        paths,
        [
            "bin/app",
            "share/env",
            "share/greeting",
            "share/nested/epoch"
        ]
    );
    assert!(tree.entries[0].executable());
    assert!(!tree.entries[2].executable());
    assert_eq!(
        world.blob_text(tree.entries[2].blob().expect("a file")),
        "hello"
    );
    assert_eq!(
        world.blob_text(tree.entries[3].blob().expect("a file")),
        "1700000000\n"
    );
    let env = world.blob_text(tree.entries[1].blob().expect("a file"));
    assert!(env.contains("LANG=C\n"), "{env}");
    assert!(env.contains("HOME="), "{env}");
    assert!(env.contains("KAIRO_PROVIDER_LIB_GREETING_SOURCE="), "{env}");
    assert!(!env.contains("USER="), "host environment leaked: {env}");
    assert!(
        env.lines().any(|line| line == "PATH=/bin"),
        "PATH is only the declared host tool's directory: {env}"
    );
    assert!(env.contains("KAIRO_PROVIDER_TOOL_SH=/bin/sh\n"), "{env}");
    assert_eq!(record.host.tools, ["/bin/sh"]);
    let _ = lib;

    // The artifact id is the tree document's blob id, held in the store.
    let artifact = BlobId::new(record.artifact.clone().expect("artifact id"))?;
    assert_eq!(load_artifact(&world.fixture.store, &artifact)?, *tree);
    assert_eq!(tree.id()?, artifact);

    // The record is on disk under executions/, by its id.
    let reloaded = load_record(&world.fixture.store, &execution.id)?;
    assert_eq!(&reloaded, record);
    assert_eq!(reloaded.id()?, execution.id);

    // Materializing it reproduces the files and modes.
    let out = tempfile::TempDir::new()?;
    materialize_artifact(&world.fixture.store, &artifact, out.path())?;
    assert_eq!(
        fs::read_to_string(out.path().join("share/greeting"))?,
        "hello"
    );
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(out.path().join("bin/app"))?
                .permissions()
                .mode()
                & 0o111,
            0
        );
    }
    Ok(())
}

#[test]
fn a_nonzero_exit_or_missing_output_is_a_failure_record() -> TestResult {
    let mut world = World::new();
    let failing = world.object(&[(
        "kairo.toml",
        &app_manifest(
            r#"["sh", "-c", "echo oops >&2; exit 3"]"#,
            FILE_AND_DIR_OUTPUTS,
            "",
        ),
    )]);
    let plan = world.plan(&failing.object_id, "release");
    let execution = world.run(&plan, Duration::from_secs(30))?;
    assert_eq!(execution.record.status, Status::Failure);
    assert_eq!(execution.record.exit_code, Some(3));
    assert!(execution.artifact.is_none());
    assert!(execution.record.artifact.is_none());
    assert_eq!(world.blob_text(&execution.record.logs.stderr), "oops\n");
    assert!(execution.record.diagnostics[0].contains("status 3"));

    let missing = world.object(&[(
        "kairo.toml",
        &app_manifest(
            r#"["sh", "-c", "mkdir -p out/share"]"#,
            FILE_AND_DIR_OUTPUTS,
            "",
        ),
    )]);
    let plan = world.plan(&missing.object_id, "release");
    let execution = world.run(&plan, Duration::from_secs(30))?;
    assert_eq!(execution.record.status, Status::Failure);
    assert_eq!(execution.record.exit_code, Some(0));
    assert!(
        execution.record.diagnostics[0].contains("output cli"),
        "{:?}",
        execution.record.diagnostics
    );
    assert!(execution.record.diagnostics[0].contains("was not produced"));

    let unstartable = world.object(&[(
        "kairo.toml",
        &app_manifest(r#"["kairo-no-such-program"]"#, FILE_AND_DIR_OUTPUTS, ""),
    )]);
    let plan = world.plan(&unstartable.object_id, "release");
    let execution = world.run(&plan, Duration::from_secs(30))?;
    assert_eq!(execution.record.status, Status::Failure);
    assert!(execution.record.diagnostics[0].contains("cannot start"));
    Ok(())
}

#[test]
fn outputs_record_links_that_stay_inside_and_refuse_the_rest() -> TestResult {
    let mut world = World::new();
    // A link to a file, one to a directory, and one dangling; the
    // file output is itself a link.
    let linked = world.object(&[(
        "kairo.toml",
        &app_manifest(
            r#"["sh", "-c", "mkdir -p out/share/dir && echo hi > out/share/dir/a.txt && ln -s dir/a.txt out/share/a && ln -s dir out/share/d && ln -s ../lib/libz.so.1 out/share/gone && ln -s /usr/bin/bzip2 out/share/bunzip2 && echo x > out/real && ln -s real out/app"]"#,
            FILE_AND_DIR_OUTPUTS,
            "",
        ),
    )]);
    let plan = world.plan(&linked.object_id, "release");
    let execution = world.run(&plan, Duration::from_secs(30))?;
    assert_eq!(
        execution.record.status,
        Status::Success,
        "{:?}",
        execution.record.diagnostics
    );
    let tree = execution.artifact.as_ref().expect("artifact");
    let links: Vec<(&str, &str)> = tree
        .entries
        .iter()
        .filter_map(|entry| Some((entry.path(), entry.link()?)))
        .collect();
    assert_eq!(
        links,
        [
            ("bin/app", "real"),
            ("share/a", "dir/a.txt"),
            ("share/bunzip2", "/usr/bin/bzip2"),
            ("share/d", "dir"),
            ("share/gone", "../lib/libz.so.1")
        ]
    );
    // Links aren't followed: the directory's file appears once.
    assert_eq!(
        tree.entries
            .iter()
            .filter(|entry| entry.path().ends_with("a.txt"))
            .count(),
        1
    );
    // Materialized, they are links again.
    let artifact = BlobId::new(execution.record.artifact.clone().expect("artifact id"))?;
    let out = tempfile::TempDir::new()?;
    materialize_artifact(&world.fixture.store, &artifact, out.path())?;
    assert_eq!(fs::read_link(out.path().join("share/d"))?, Path::new("dir"));
    assert_eq!(fs::read_to_string(out.path().join("share/a"))?, "hi\n");
    assert!(out
        .path()
        .join("share/gone")
        .symlink_metadata()?
        .is_symlink());
    assert_eq!(
        fs::read_link(out.path().join("share/bunzip2"))?,
        Path::new("/usr/bin/bzip2")
    );

    // Escaping relative targets fail the build, naming the link; so
    // does a directory output that is a link. An absolute target is the
    // guest's path, kept as written.
    for (command, expected) in [
        ("mkdir -p out/share && ln -s / out/app", "is only /"),
        (
            "mkdir -p out/share && echo x > out/app && ln -s ../../../x out/share/up",
            "climbs above",
        ),
        (
            "mkdir -p elsewhere out && echo x > out/app && ln -s ../elsewhere out/share",
            "directory output",
        ),
    ] {
        let object = world.object(&[(
            "kairo.toml",
            &app_manifest(
                &format!(r#"["sh", "-c", "{command}"]"#),
                FILE_AND_DIR_OUTPUTS,
                "",
            ),
        )]);
        let plan = world.plan(&object.object_id, "release");
        let execution = world.run(&plan, Duration::from_secs(30))?;
        assert_eq!(execution.record.status, Status::Failure, "{command}");
        assert!(
            execution
                .record
                .diagnostics
                .iter()
                .any(|line| line.contains(expected)),
            "{command}: {:?}",
            execution.record.diagnostics
        );
    }
    Ok(())
}

#[test]
fn artifact_trees_refuse_links_that_escape_or_hold_entries() {
    let tree = |entries: &str| {
        ArtifactTree::from_bytes(
            format!(r#"{{"schema":"kairo.artifact-tree.v1","entries":[{entries}]}}"#).as_bytes(),
        )
    };
    let blob = BlobId::from_sha256_digest([1; 32]);
    let file = |path: &str| format!(r#"{{"path":"{path}","blob":"{blob}","executable":false}}"#);
    let link = |path: &str, target: &str| format!(r#"{{"path":"{path}","link":"{target}"}}"#);
    assert!(tree(&format!("{},{}", link("a/b", "../c"), file("c"))).is_ok());
    assert!(tree(&link("a", "/etc/passwd")).is_ok());
    // A trailing slash or a doubled one, as util-linux installs.
    assert!(tree(&link("usr/lib/x/libblkid.so", "../../usr/lib/x/")).is_ok());
    assert!(tree(&link("a/b", "c//d")).is_ok());
    for (entries, expected) in [
        (
            format!("{},{}", link("a", "x"), file("a/passwd")),
            "lies under the link",
        ),
        (link("a", "/"), "is only /"),
        (link("a/b", "../../c"), "climbs above"),
        (link("a", ""), "is empty"),
        (format!("{},{}", file("b"), file("a")), "out of order"),
        (
            format!(r#"{{"path":"a","link":"b","blob":"{blob}"}}"#),
            "did not match",
        ),
    ] {
        let result = tree(&entries);
        assert!(
            matches!(&result, Err(error) if error.to_string().contains(expected)),
            "{entries}: {result:?}"
        );
    }
    // A tree of files encodes as it did before links existed.
    let files = ArtifactTree::new(vec![ArtifactEntry::File(FileEntry {
        path: "bin/app".to_owned(),
        blob: blob.to_string(),
        executable: true,
    })]);
    assert_eq!(
        String::from_utf8(files.to_bytes().expect("bytes")).expect("utf8"),
        format!(
            r#"{{"schema":"kairo.artifact-tree.v1","entries":[{{"path":"bin/app","blob":"{blob}","executable":true}}]}}"#
        )
    );
}

#[test]
fn file_sources_are_placed_in_the_tree_by_hash() -> TestResult {
    use kairo_store::SourceStore as _;
    // SHA-256("abc").
    let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let deps = format!(
        r#"
        [[dependencies]]
        kind = "file"
        sha256 = "{abc}"
        path = "distfiles/abc.txt"
        "#
    );
    let outputs = r#"
        [[build.targets.outputs]]
        name = "copy"
        path = "out/abc.txt"
        artifact_path = "abc.txt"
        kind = "file"
    "#;
    let mut world = World::new();
    let app = world.object(&[(
        "kairo.toml",
        &app_manifest(
            r#"["sh", "-c", "mkdir out && cp distfiles/abc.txt out/"]"#,
            outputs,
            &deps,
        ),
    )]);
    // Not held: the plan can't be made.
    let host = Host {
        system: "linux".to_owned(),
        arch: "x86-64".to_owned(),
        engines: Vec::new(),
    };
    let tools = [HostTool {
        provides: "tool:sh".to_owned(),
        version: None,
        path: PathBuf::from("/bin/sh"),
    }];
    let unheld = Resolver::new(
        &world.fixture.store,
        &world.trees,
        &NoArtifacts,
        &host,
        &tools,
    )
    .resolve(&app.object_id, &SnapshotSelector::Current, "release");
    assert!(
        matches!(&unheld, Err(error) if error.to_string().contains("fetch-file")),
        "{unheld:?}"
    );

    let genesis =
        kairo_statement::FileSourceGenesis::new(kairo_statement::Sha256Digest::parse(abc)?);
    world.fixture.store.put_file_source(&genesis, b"abc")?;
    let plan = world.plan(&app.object_id, "release");
    let execution = world.run(&plan, Duration::from_secs(30))?;
    assert_eq!(
        execution.record.status,
        Status::Success,
        "{:?}",
        execution.record.diagnostics
    );
    let tree = execution.artifact.as_ref().expect("artifact");
    assert_eq!(
        world.blob_text(tree.entries[0].blob().expect("a file")),
        "abc"
    );

    // A tree whose own symlink would carry the placement elsewhere is
    // refused, as is one that already holds the path.
    for (name, contents) in [("distfiles", "@link:/tmp"), ("distfiles/abc.txt", "mine")] {
        let other = world.object(&[
            (
                "kairo.toml",
                &app_manifest(r#"["sh", "-c", "true"]"#, outputs, &deps),
            ),
            (name, contents),
        ]);
        let plan = world.plan(&other.object_id, "release");
        let refused = world.run(&plan, Duration::from_secs(30));
        assert!(
            matches!(&refused, Err(BuildError::PlacementOccupied { path }) if path == "distfiles/abc.txt"),
            "{name}: {refused:?}"
        );
    }
    Ok(())
}

#[test]
fn the_wall_clock_limit_kills_the_process_group() -> TestResult {
    let mut world = World::new();
    let slow = world.object(&[(
        "kairo.toml",
        &app_manifest(
            r#"["sh", "-c", "sleep 30 & wait"]"#,
            FILE_AND_DIR_OUTPUTS,
            "",
        ),
    )]);
    let plan = world.plan(&slow.object_id, "release");
    let started = std::time::Instant::now();
    let execution = world.run(&plan, Duration::from_millis(300))?;
    assert_eq!(execution.record.status, Status::Timeout);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "killed promptly"
    );
    assert!(execution.record.diagnostics[0].contains("killed after"));
    Ok(())
}

#[test]
fn a_plan_with_builds_first_is_refused() -> TestResult {
    let mut world = World::new();
    world.object(&[(
        "kairo.toml",
        r#"
        [kairo]
        schema = 1
        kind = "software"
        name = "make"
        [[provides]]
        provides = "tool:make"
        target = "release"
        output = "bin"
        [[build.targets]]
        name = "release"
        command = ["true"]
        [[build.targets.outputs]]
        name = "bin"
        path = "out"
        artifact_path = "bin"
        kind = "directory"
        "#,
    )]);
    let app = world.object(&[(
        "kairo.toml",
        &app_manifest(
            r#"["true"]"#,
            FILE_AND_DIR_OUTPUTS,
            r#"
            [[dependencies]]
            kind = "provides"
            provides = "tool:make"
            "#,
        ),
    )]);
    let plan = world.plan(&app.object_id, "release");
    let error = world
        .run(&plan, Duration::from_secs(5))
        .expect_err("needs a build first");
    assert!(
        matches!(error, BuildError::PrerequisitesUnbuilt { .. }),
        "{error}"
    );
    Ok(())
}

#[test]
fn provider_variables_are_derived_from_labels() {
    assert_eq!(provider_var("tool:make"), "KAIRO_PROVIDER_TOOL_MAKE");
    assert_eq!(
        provider_var("lib:zlib:source"),
        "KAIRO_PROVIDER_LIB_ZLIB_SOURCE"
    );
    assert_eq!(
        provider_var("environment:linux/x86-64"),
        "KAIRO_PROVIDER_ENVIRONMENT_LINUX_X86_64"
    );
}

/// A usable bubblewrap and a statically linked busybox, or `None` to
/// skip: the engine needs user namespaces, and the guest root is empty.
/// Whether a 64-bit little-endian ELF has a `PT_INTERP` program
/// header, i.e. is dynamically linked. `None` if it isn't such an ELF.
fn has_interpreter(elf: &[u8]) -> Option<bool> {
    if elf.get(..4)? != b"\x7fELF" || elf.get(4)? != &2 || elf.get(5)? != &1 {
        return None;
    }
    let u16_at = |at: usize| Some(u16::from_le_bytes(elf.get(at..at + 2)?.try_into().ok()?));
    let phoff = usize::try_from(u64::from_le_bytes(elf.get(0x20..0x28)?.try_into().ok()?)).ok()?;
    let entsize = usize::from(u16_at(0x36)?);
    let count = usize::from(u16_at(0x38)?);
    for index in 0..count {
        let at = phoff + index * entsize;
        let kind = u32::from_le_bytes(elf.get(at..at + 4)?.try_into().ok()?);
        if kind == 3 {
            return Some(true);
        }
    }
    Some(false)
}

/// With `KAIRO_REQUIRE_ENGINE` set, a missing engine fails the test
/// instead of skipping it, so CI on a capable host can't pass vacuously.
fn engine_fixture() -> Option<Bubblewrap> {
    let found = (|| {
        let busybox = fs::read("/usr/bin/busybox").ok()?;
        if has_interpreter(&busybox)? {
            return None; // dynamically linked: unusable in an empty root
        }
        // KAIRO_TEST_BWRAP names another bubblewrap, such as one new
        // enough to overlay (§41 sub-decision 5).
        let bwrap = std::env::var_os("KAIRO_TEST_BWRAP").unwrap_or("/usr/bin/bwrap".into());
        Bubblewrap::probe(Path::new(&bwrap)).ok()
    })();
    assert!(
        found.is_some() || std::env::var_os("KAIRO_REQUIRE_ENGINE").is_none(),
        "KAIRO_REQUIRE_ENGINE is set but bubblewrap or a static busybox is unavailable"
    );
    found
}

#[test]
fn the_bubblewrap_engine_runs_with_providers_and_nothing_from_the_host() -> TestResult {
    let Some(bwrap) = engine_fixture() else {
        return Ok(());
    };
    let mut world = World::new();
    world.object(&[
        (
            "kairo.toml",
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "busybox"
            [[provides]]
            provides = "tool:sh"
            "#,
        ),
        ("bin/busybox", "@copy:/usr/bin/busybox"),
        ("bin/sh", "@link:busybox"),
    ]);
    let host = Host::detect();
    let contract = host.contract();
    let app = world.object(&[(
        "kairo.toml",
        &format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "app"
            [[dependencies]]
            kind = "provides"
            provides = "{contract}"
            [[build.targets]]
            name = "release"
            command = ["sh", "-c", "busybox mkdir -p out; {{ test -e /usr && echo usr-visible || echo no-usr; busybox id -u; busybox hostname; busybox cat /proc/net/dev | busybox grep -c ':'; echo $HOME $KAIRO_SRC $TZ; echo $PATH; busybox pwd; }} > out/report"]
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:sh"
            [[build.targets.outputs]]
            name = "report"
            path = "out/report"
            artifact_path = "report"
            kind = "file"
            "#
        ),
    )]);

    let mut engine_host = host.clone();
    engine_host.engines.push(contract.clone());
    let plan = world.plan_with(&engine_host, &app.object_id, "release");
    assert_eq!(plan.manifest.environment, contract);
    assert!(!plan.manifest.host_dependent);

    // A plan with a contract can't run natively, nor one without under the engine.
    assert!(matches!(
        run_plan(
            &world.fixture.store,
            &world.trees,
            &plan,
            1,
            &Limits {
                timeout: Duration::from_secs(5)
            },
            &Engine::Native
        ),
        Err(BuildError::EngineMismatch { .. })
    ));

    let execution = run_plan(
        &world.fixture.store,
        &world.trees,
        &plan,
        1_700_000_000,
        &Limits {
            timeout: Duration::from_secs(30),
        },
        &Engine::Bubblewrap(bwrap.clone()),
    )?;
    let record = &execution.record;
    assert_eq!(
        record.status,
        Status::Success,
        "{:?} {}",
        record.diagnostics,
        world.blob_text(&record.logs.stderr)
    );
    assert_eq!(record.executor.id, "bubblewrap");
    assert!(!record.host_dependent);
    assert_eq!(record.environment, contract);
    assert_eq!(
        record.executor.known_nondeterminism,
        ["wall clock not frozen"]
    );
    assert_eq!(
        record.executor.engine.as_ref().map(|e| e.version.clone()),
        Some(bwrap.version.clone())
    );
    assert!(!record.host.kernel.is_empty());

    let tree = execution.artifact.as_ref().expect("artifact");
    let report = world.blob_text(tree.entries[0].blob().expect("a file"));
    let lines: Vec<&str> = report.lines().collect();
    assert_eq!(lines[0], "no-usr", "{report}");
    // Root in its own namespaces (`DECISIONS.md` §41 sub-decision 1).
    assert_eq!(lines[1], "0", "{report}");
    assert_eq!(lines[2], "kairo", "{report}");
    assert_eq!(lines[3], "1", "only loopback: {report}");
    assert_eq!(lines[4], "/tmp /kairo/src UTC", "{report}");
    assert!(
        lines[5].starts_with("/kairo/content/")
            && lines[5].ends_with("/bin:/usr/local/bin:/usr/bin:/bin"),
        "{report}"
    );
    assert_eq!(lines[6], "/kairo/src", "{report}");

    // The same plan under the engine again gives the same artifact.
    let again = run_plan(
        &world.fixture.store,
        &world.trees,
        &plan,
        1_700_000_000,
        &Limits {
            timeout: Duration::from_secs(30),
        },
        &Engine::Bubblewrap(bwrap),
    )?;
    assert_eq!(again.record.artifact, record.artifact);
    Ok(())
}

#[test]
fn the_probe_reports_whether_sandboxes_run_i386() -> TestResult {
    let Some(bwrap) = engine_fixture() else {
        return Ok(());
    };
    // The same program, run directly, says whether this kernel runs
    // i386 at all.
    let dir = tempfile::TempDir::new()?;
    let program = dir.path().join("probe");
    fs::write(&program, i386_exit_42())?;
    set_executable(&program, true)?;
    let direct = std::process::Command::new(&program)
        .status()
        .is_ok_and(|status| status.code() == Some(42));
    assert_eq!(bwrap.runs_x86, direct && std::env::consts::ARCH == "x86_64");
    Ok(())
}

#[test]
fn a_probe_of_a_missing_engine_says_why() {
    let error = Bubblewrap::probe(Path::new("/nonexistent/bwrap")).expect_err("no such engine");
    assert!(error.contains("cannot run"), "{error}");
}

#[test]
fn placements_take_a_subtree_and_overlay_what_the_tree_holds() -> TestResult {
    use kairo_store::SourceStore as _;
    let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let mut world = World::new();
    world.object(&[
        (
            "kairo.toml",
            r#"
            [kairo]
            schema = 1
            kind = "data"
            name = "upstream"
            [[provides]]
            provides = "data:upstream"
            "#,
        ),
        ("sub/a.txt", "upstream a"),
        ("sub/b.txt", "upstream b"),
        ("sub/deep/c.txt", "upstream c"),
        ("top.txt", "not placed"),
    ]);
    world.fixture.store.put_file_source(
        &kairo_statement::FileSourceGenesis::new(kairo_statement::Sha256Digest::parse(abc)?),
        b"abc",
    )?;
    let outputs = r#"
        [[build.targets.outputs]]
        name = "vendor"
        path = "vendor"
        artifact_path = "vendor"
        kind = "directory"
    "#;
    let app = |world: &mut World, overlay: bool| {
        let deps = format!(
            r#"
            [[dependencies]]
            kind = "provides"
            provides = "data:upstream"
            path = "vendor"
            from = "sub"
            overlay = {overlay}
            [[dependencies]]
            kind = "file"
            sha256 = "{abc}"
            path = "vendor/deep/c.txt"
            overlay = {overlay}
            "#
        );
        world.object(&[
            (
                "kairo.toml",
                &app_manifest(r#"["sh", "-c", "true"]"#, outputs, &deps),
            ),
            // The wrapper's own files, which an overlay keeps.
            ("vendor/a.txt", "mine"),
            ("vendor/deep/keep.txt", "mine too"),
        ])
    };

    let overlaid = app(&mut world, true);
    let plan = world.plan(&overlaid.object_id, "release");
    let execution = world.run(&plan, Duration::from_secs(30))?;
    assert_eq!(
        execution.record.status,
        Status::Success,
        "{:?}",
        execution.record.diagnostics
    );
    let tree = execution.artifact.as_ref().expect("artifact");
    let files: Vec<(String, String)> = tree
        .entries
        .iter()
        .map(|entry| {
            (
                entry.path().to_owned(),
                world.blob_text(entry.blob().expect("a file")),
            )
        })
        .collect();
    let expected = [
        ("vendor/a.txt", "mine"),
        ("vendor/b.txt", "upstream b"),
        // The file lands after the shallower placement, which already
        // put a c.txt there; an overlay never replaces what exists.
        ("vendor/deep/c.txt", "upstream c"),
        ("vendor/deep/keep.txt", "mine too"),
    ];
    assert_eq!(
        files,
        expected
            .iter()
            .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
            .collect::<Vec<_>>()
    );

    // Without overlay, the wrapper's own vendor/ is in the way.
    let plain = app(&mut world, false);
    let plan = world.plan(&plain.object_id, "release");
    assert!(matches!(
        world.run(&plan, Duration::from_secs(30)),
        Err(BuildError::PlacementOccupied { path }) if path == "vendor"
    ));
    Ok(())
}

/// Artifacts by `(object, target)`, as the store's build results would
/// give them.
struct Built(Vec<(ObjectId, String, BlobId)>);

impl kairo_resolver::ArtifactSource for Built {
    fn artifact(
        &self,
        object: &ObjectId,
        _snapshot: &SnapshotId,
        target: &str,
    ) -> Result<Option<BlobId>, SourceError> {
        Ok(self
            .0
            .iter()
            .find(|(o, t, _)| o == object && t == target)
            .map(|(_, _, id)| id.clone()))
    }
}

#[test]
fn artifacts_land_at_their_install_roots_and_builds_stage_into_the_package() -> TestResult {
    use kairo_store::SourceStore as _;
    let Some(bwrap) = engine_fixture() else {
        return Ok(());
    };
    let mut world = World::new();
    world.object(&[
        (
            "kairo.toml",
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "busybox"
            [[provides]]
            provides = "tool:sh"
            "#,
        ),
        ("bin/busybox", "@copy:/usr/bin/busybox"),
        ("bin/sh", "@link:busybox"),
    ]);
    let contract = Host::detect().contract();
    let mut host = Host::detect();
    host.engines.push(contract.clone());
    // Two providers that stage what they build under /kairo/package: one
    // installed at /, one only invoked, at /kairo/artifact/<id>.
    let provider = |name: &str, install: &str| {
        format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "{name}"
            [[dependencies]]
            kind = "provides"
            provides = "{contract}"
            [[provides]]
            provides = "tool:{name}"
            target = "release"
            [[build.targets]]
            name = "release"
            command = ["sh", "-c", "busybox mkdir -p /kairo/package/usr/bin && busybox cp $KAIRO_PROVIDER_TOOL_SH/bin/busybox /kairo/package/usr/bin/busybox && busybox ln -s busybox /kairo/package/usr/bin/ls"]
            [build.targets.artifact]
            install = {install}
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:sh"
            "#
        )
    };
    let installed = world.object(&[("kairo.toml", &provider("boxed", "\"/\""))]);
    let invoked = world.object(&[("kairo.toml", &provider("loose", "false"))]);
    let mut built = Vec::new();
    for object in [&installed, &invoked] {
        let plan = world.plan_with(&host, &object.object_id, "release");
        let execution = run_plan(
            &world.fixture.store,
            &world.trees,
            &plan,
            1_700_000_000,
            &Limits {
                timeout: Duration::from_secs(30),
            },
            &Engine::Bubblewrap(bwrap.clone()),
        )?;
        assert_eq!(
            execution.record.status,
            Status::Success,
            "{:?} {}",
            execution.record.diagnostics,
            world.blob_text(&execution.record.logs.stderr)
        );
        // The artifact is the package alone: nothing from / or
        // /kairo/src.
        let paths: Vec<&str> = execution
            .artifact
            .as_ref()
            .expect("artifact")
            .entries
            .iter()
            .map(ArtifactEntry::path)
            .collect();
        assert_eq!(paths, ["usr/bin/busybox", "usr/bin/ls"]);
        built.push((
            object.object_id.clone(),
            "release".to_owned(),
            BlobId::new(execution.record.artifact.clone().expect("artifact id"))?,
        ));
    }
    let built = Built(built);

    // A consumer: both providers, and a file placed at an absolute path.
    let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    world.fixture.store.put_file_source(
        &kairo_statement::FileSourceGenesis::new(kairo_statement::Sha256Digest::parse(abc)?),
        b"abc",
    )?;
    let consumer = world.object(&[(
        "kairo.toml",
        &format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "consumer"
            [[dependencies]]
            kind = "provides"
            provides = "{contract}"
            [[build.targets]]
            name = "release"
            command = ["/usr/bin/busybox", "sh", "-c", "{{ ls /usr/bin; echo $KAIRO_PROVIDER_TOOL_BOXED; echo $KAIRO_PROVIDER_TOOL_LOOSE; busybox test -x $KAIRO_PROVIDER_TOOL_LOOSE/usr/bin/busybox && echo loose-ok; busybox cat /opt/data/abc.txt; echo; echo $PATH; }} > /kairo/package/report"]
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:boxed"
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:loose"
            [[build.targets.dependencies]]
            kind = "file"
            sha256 = "{abc}"
            path = "/opt/data/abc.txt"
            "#
        ),
    )]);
    let plan = Resolver::new(&world.fixture.store, &world.trees, &built, &host, &[])
        .resolve(&consumer.object_id, &SnapshotSelector::Current, "release")
        .expect("plan")
        .plan;
    let execution = run_plan(
        &world.fixture.store,
        &world.trees,
        &plan,
        1_700_000_000,
        &Limits {
            timeout: Duration::from_secs(30),
        },
        &Engine::Bubblewrap(bwrap),
    )?;
    assert_eq!(
        execution.record.status,
        Status::Success,
        "{:?} {}",
        execution.record.diagnostics,
        world.blob_text(&execution.record.logs.stderr)
    );
    let tree = execution.artifact.as_ref().expect("artifact");
    let report = world.blob_text(tree.entries[0].blob().expect("a file"));
    let lines: Vec<&str> = report.lines().collect();
    // boxed's files are at their real paths, its link with them; loose
    // stays at /kairo/artifact/<id>, out of /usr.
    assert_eq!(&lines[..2], ["busybox", "ls"], "{report}");
    assert_eq!(lines[2], "/", "{report}");
    assert!(lines[3].starts_with("/kairo/artifact/"), "{report}");
    assert_eq!(lines[4], "loose-ok", "{report}");
    assert_eq!(lines[5], "abc", "{report}");
    assert!(
        lines[6].starts_with("/kairo/artifact/")
            && lines[6].ends_with("/usr/bin:/usr/local/bin:/usr/bin:/bin"),
        "{report}"
    );
    Ok(())
}

#[test]
fn a_slice_is_the_part_under_its_root_and_stands_alone() {
    let file = |path: &str| {
        ArtifactEntry::File(FileEntry {
            path: path.to_owned(),
            blob: "zQm".to_owned(),
            executable: false,
        })
    };
    let link = |path: &str, link: &str| {
        ArtifactEntry::Link(LinkEntry {
            path: path.to_owned(),
            link: link.to_owned(),
        })
    };
    let tree = ArtifactTree::new(vec![
        file("bash/usr/bin/bash"),
        link("bash/usr/bin/sh", "bash"),
        file("bashful"),
        file("make/bin/make"),
        link("make/bin/gmake", "../../bash/usr/bin/bash"),
    ]);
    let bash = slice_artifact(&tree, "bash").expect("bash");
    assert_eq!(
        bash.entries
            .iter()
            .map(ArtifactEntry::path)
            .collect::<Vec<_>>(),
        ["usr/bin/bash", "usr/bin/sh"]
    );
    assert_eq!(
        slice_artifact(&tree, "bash/usr")
            .expect("usr")
            .entries
            .len(),
        2
    );
    // A link out of the slice doesn't stand alone; nothing isn't a slice.
    assert!(slice_artifact(&tree, "make").is_err());
    assert!(slice_artifact(&tree, "perl").is_err());
}

#[test]
fn a_build_slices_its_children_and_a_consumer_installs_one() -> TestResult {
    let Some(bwrap) = engine_fixture() else {
        return Ok(());
    };
    let mut world = World::new();
    world.object(&[
        (
            "kairo.toml",
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "busybox"
            [[provides]]
            provides = "tool:sh"
            "#,
        ),
        ("bin/busybox", "@copy:/usr/bin/busybox"),
        ("bin/sh", "@link:busybox"),
    ]);
    let contract = Host::detect().contract();
    let mut host = Host::detect();
    host.engines.push(contract.clone());
    // One build staging two packages, each a child.
    let bundle = world.object(&[(
        "kairo.toml",
        &format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "bundle"
            [[dependencies]]
            kind = "provides"
            provides = "{contract}"
            [[build.targets]]
            name = "release"
            command = ["sh", "-c", "for p in boxed loose; do busybox mkdir -p /kairo/package/$p/usr/bin && busybox cp $KAIRO_PROVIDER_TOOL_SH/bin/busybox /kairo/package/$p/usr/bin/busybox; done && busybox ln -s busybox /kairo/package/boxed/usr/bin/ls"]
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:sh"
            [[build.targets.children]]
            name = "boxed"
            root = "boxed"
            provides = [{{ provides = "tool:boxed" }}]
            [[build.targets.children]]
            name = "loose"
            root = "loose"
            install = false
            provides = [{{ provides = "tool:loose" }}]
            "#
        ),
    )]);
    let plan = world.plan_with(&host, &bundle.object_id, "release");
    let execution = run_plan(
        &world.fixture.store,
        &world.trees,
        &plan,
        1_700_000_000,
        &Limits {
            timeout: Duration::from_secs(30),
        },
        &Engine::Bubblewrap(bwrap.clone()),
    )?;
    assert_eq!(
        execution.record.status,
        Status::Success,
        "{:?} {}",
        execution.record.diagnostics,
        world.blob_text(&execution.record.logs.stderr)
    );
    let artifact = BlobId::new(execution.record.artifact.clone().expect("artifact id"))?;
    let tree = execution.artifact.as_ref().expect("artifact");
    let children = &execution.record.children;
    assert_eq!(
        children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["boxed", "loose"]
    );
    for child in children {
        assert_eq!(
            child.object,
            ChildGenesis::new(bundle.object_id.clone(), "release", &child.name)
                .object_id()
                .to_string()
        );
        let slice = slice_artifact(tree, &child.name).expect("slice");
        assert_eq!(child.artifact, slice.id()?.to_string());
        assert_eq!(
            load_artifact(&world.fixture.store, &BlobId::new(child.artifact.clone())?)?,
            slice
        );
    }
    // A slice's tree is among what the record carries.
    let blobs = evidence_blobs(&world.fixture.store, &execution.record)?;
    assert!(blobs
        .iter()
        .any(|(id, _)| id.to_string() == children[1].artifact));

    let consumer = world.object(&[(
        "kairo.toml",
        &format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "consumer"
            [[dependencies]]
            kind = "provides"
            provides = "{contract}"
            [[build.targets]]
            name = "release"
            command = ["/usr/bin/busybox", "sh", "-c", "{{ ls /usr/bin; ls /; echo $KAIRO_PROVIDER_TOOL_LOOSE; }} > /kairo/package/report"]
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:boxed"
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:loose"
            "#
        ),
    )]);
    let built = Built(vec![(
        bundle.object_id.clone(),
        "release".to_owned(),
        artifact,
    )]);
    let plan = Resolver::new(&world.fixture.store, &world.trees, &built, &host, &[])
        .resolve(&consumer.object_id, &SnapshotSelector::Current, "release")
        .expect("plan")
        .plan;
    let execution = run_plan(
        &world.fixture.store,
        &world.trees,
        &plan,
        1_700_000_000,
        &Limits {
            timeout: Duration::from_secs(30),
        },
        &Engine::Bubblewrap(bwrap),
    )?;
    assert_eq!(
        execution.record.status,
        Status::Success,
        "{:?} {}",
        execution.record.diagnostics,
        world.blob_text(&execution.record.logs.stderr)
    );
    let report = execution.artifact.as_ref().expect("artifact").entries[0]
        .blob()
        .map(|blob| world.blob_text(blob))
        .expect("report");
    // Only the boxed child is installed, at its root's contents; the
    // loose one is at its own artifact id.
    assert_eq!(
        report,
        format!(
            "busybox\nls\ndev\nkairo\nproc\nsys\ntmp\nusr\n/kairo/artifact/{}\n",
            children[1].artifact
        )
    );
    Ok(())
}

#[test]
fn overlaid_and_copied_dependencies_give_the_guest_the_same_tree() -> TestResult {
    use kairo_store::SourceStore as _;
    let Some(bwrap) = engine_fixture() else {
        return Ok(());
    };
    let mut world = World::new();
    world.object(&[
        (
            "kairo.toml",
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "busybox"
            [[provides]]
            provides = "tool:sh"
            "#,
        ),
        ("bin/busybox", "@copy:/usr/bin/busybox"),
        ("bin/sh", "@link:busybox"),
    ]);
    let contract = Host::detect().contract();
    let mut host = Host::detect();
    host.engines.push(contract.clone());
    // Two providers installed at /, both with a usr/share/who; one also
    // at /opt/second.
    let provider = |name: &str, install: &str| {
        format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "{name}"
            [[dependencies]]
            kind = "provides"
            provides = "{contract}"
            [[provides]]
            provides = "tool:{name}"
            target = "release"
            [[build.targets]]
            name = "release"
            command = ["sh", "-c", "busybox mkdir -p /kairo/package/usr/bin /kairo/package/usr/share && busybox cp $KAIRO_PROVIDER_TOOL_SH/bin/busybox /kairo/package/usr/bin/busybox && echo {name} > /kairo/package/usr/share/who && echo {name} > /kairo/package/usr/share/{name}"]
            [build.targets.artifact]
            install = "{install}"
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:sh"
            "#
        )
    };
    let first = world.object(&[("kairo.toml", &provider("first", "/"))]);
    let second = world.object(&[("kairo.toml", &provider("second", "/"))]);
    let third = world.object(&[("kairo.toml", &provider("third", "/opt/third"))]);
    let limits = Limits {
        timeout: Duration::from_secs(30),
    };
    let mut built = Vec::new();
    for object in [&first, &second, &third] {
        let plan = world.plan_with(&host, &object.object_id, "release");
        let execution = run_plan(
            &world.fixture.store,
            &world.trees,
            &plan,
            1_700_000_000,
            &limits,
            &Engine::Bubblewrap(bwrap.clone()),
        )?;
        assert_eq!(execution.record.status, Status::Success);
        built.push((
            object.object_id.clone(),
            "release".to_owned(),
            BlobId::new(execution.record.artifact.clone().expect("artifact id"))?,
        ));
    }
    let built = Built(built);

    // The consumer: the first provider wins usr/share/who over the
    // second, and over an overlaid file placed there; a file placed
    // where nothing is lands.
    let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    world.fixture.store.put_file_source(
        &kairo_statement::FileSourceGenesis::new(kairo_statement::Sha256Digest::parse(abc)?),
        b"abc",
    )?;
    let consumer = |placed: &str, overlay: bool| {
        format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "consumer"
            [[dependencies]]
            kind = "provides"
            provides = "{contract}"
            [[build.targets]]
            name = "release"
            command = ["/usr/bin/busybox", "sh", "-c", "{{ busybox find / -path /proc -prune -o -path /dev -prune -o -print | busybox sort; busybox cat /usr/share/who /usr/share/abc /opt/third/usr/share/third; }} > /kairo/package/report"]
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:first"
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:second"
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:third"
            # Met by the same artifact as the first: one layer.
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:first"
            [[build.targets.dependencies]]
            kind = "file"
            sha256 = "{abc}"
            path = "/usr/share/abc"
            [[build.targets.dependencies]]
            kind = "file"
            sha256 = "{abc}"
            path = "{placed}"
            overlay = {overlay}
            "#
        )
    };
    let gives_way = world.object(&[("kairo.toml", &consumer("/usr/share/who", true))]);
    // A placement that doesn't overlay is refused where a dependency
    // already is, either way.
    let refused = world.object(&[("kairo.toml", &consumer("/usr/share/who", false))]);
    let copying = Bubblewrap {
        overlay: false,
        ..bwrap.clone()
    };
    let run = |object: &ObjectId, engine: &Bubblewrap| {
        let plan = Resolver::new(&world.fixture.store, &world.trees, &built, &host, &[])
            .resolve(object, &SnapshotSelector::Current, "release")
            .expect("plan")
            .plan;
        run_plan(
            &world.fixture.store,
            &world.trees,
            &plan,
            1_700_000_000,
            &limits,
            &Engine::Bubblewrap(engine.clone()),
        )
    };
    let copied = run(&gives_way.object_id, &copying)?;
    assert_eq!(copied.record.provisioning, Some(Provisioning::Copy));
    assert_eq!(
        copied.record.status,
        Status::Success,
        "{:?} {}",
        copied.record.diagnostics,
        world.blob_text(&copied.record.logs.stderr)
    );
    let report = world.blob_text(
        copied.artifact.as_ref().expect("artifact").entries[0]
            .blob()
            .expect("a file"),
    );
    assert!(report.ends_with("first\nabcthird\n"), "{report}");
    if bwrap.overlay {
        let overlaid = run(&gives_way.object_id, &bwrap)?;
        assert_eq!(overlaid.record.provisioning, Some(Provisioning::Overlay));
        let theirs = world.blob_text(
            overlaid.artifact.as_ref().expect("artifact").entries[0]
                .blob()
                .expect("a file"),
        );
        assert_eq!(theirs, report);
        assert_eq!(overlaid.record.artifact, copied.record.artifact);
        // Its workspace is gone, overlayfs's work directory with it.
        assert_eq!(
            fs::read_dir(world.fixture.store.root().join("build"))?.count(),
            0
        );
        // The layers are cached for the next build.
        assert_eq!(
            fs::read_dir(world.fixture.store.root().join("cache/layers"))?.count(),
            3
        );
    }

    for engine in [&copying, &bwrap] {
        assert!(matches!(
            run(&refused.object_id, engine),
            Err(BuildError::PlacementOccupied { path }) if path == "/usr/share/who"
        ));
    }
    Ok(())
}

#[test]
fn a_union_lookup_follows_overlayfs_precedence() -> TestResult {
    let dir = tempfile::tempdir()?;
    let [top, bottom] = ["top", "bottom"].map(|name| dir.path().join(name));
    fs::create_dir_all(top.join("usr/bin"))?;
    fs::write(top.join("usr/bin/sh"), "top")?;
    std::os::unix::fs::symlink("usr/lib", top.join("lib"))?;
    fs::create_dir_all(bottom.join("usr/bin"))?;
    fs::create_dir_all(bottom.join("lib/x"))?;
    fs::create_dir_all(bottom.join("usr/bin/sh"))?;
    fs::write(bottom.join("usr/bin/ls"), "bottom")?;
    let layers = [top.clone(), bottom.clone()];
    // Directories merge; the top's file hides the bottom's directory;
    // what's under the top's link is hidden too.
    assert!(matches!(union_lookup(&layers, "/usr/bin"), UnionEntry::Dir(dirs) if dirs.len() == 2));
    assert!(matches!(
        union_lookup(&layers, "/usr/bin/sh"),
        UnionEntry::Other
    ));
    assert!(matches!(
        union_lookup(&layers, "/usr/bin/ls"),
        UnionEntry::Other
    ));
    assert!(matches!(union_lookup(&layers, "/lib"), UnionEntry::Other));
    assert!(matches!(
        union_lookup(&layers, "/lib/x"),
        UnionEntry::Blocked
    ));
    assert!(matches!(union_lookup(&layers, "/opt"), UnionEntry::Missing));
    assert!(matches!(union_lookup(&layers, "/"), UnionEntry::Dir(_)));
    assert!(matches!(union_lookup(&[], "/usr"), UnionEntry::Missing));
    Ok(())
}

/// Artifacts with the plans that built them, by `(object, target)`.
struct BuiltWithPlans(Vec<(ObjectId, String, BlobId, Vec<u8>)>);

impl kairo_resolver::ArtifactSource for BuiltWithPlans {
    fn artifact(
        &self,
        object: &ObjectId,
        _snapshot: &SnapshotId,
        target: &str,
    ) -> Result<Option<BlobId>, SourceError> {
        Ok(self
            .0
            .iter()
            .find(|(o, t, _, _)| o == object && t == target)
            .map(|(_, _, id, _)| id.clone()))
    }

    fn build_plan(
        &self,
        object: &ObjectId,
        _snapshot: &SnapshotId,
        target: &str,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        Ok(self
            .0
            .iter()
            .find(|(o, t, _, _)| o == object && t == target)
            .map(|(_, _, _, plan)| plan.clone()))
    }
}

const BUSYBOX_PROVIDER: &[(&str, &str)] = &[
    (
        "kairo.toml",
        r#"
        [kairo]
        schema = 1
        kind = "software"
        name = "busybox"
        [[provides]]
        provides = "tool:sh"
        "#,
    ),
    ("bin/busybox", "@copy:/usr/bin/busybox"),
    ("bin/sh", "@link:busybox"),
];

#[test]
fn what_a_dependency_requires_is_installed_beside_it() -> TestResult {
    let Some(bwrap) = engine_fixture() else {
        return Ok(());
    };
    let mut world = World::new();
    world.object(BUSYBOX_PROVIDER);
    let contract = Host::detect().contract();
    let mut host = Host::detect();
    host.engines.push(contract.clone());
    // A bundle of two children: main requires helper beside it (§46).
    let bundle = world.object(&[(
        "kairo.toml",
        &format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "bundle"
            [[dependencies]]
            kind = "provides"
            provides = "{contract}"
            [[build.targets]]
            name = "release"
            command = ["sh", "-c", "busybox mkdir -p /kairo/package/main/usr/bin /kairo/package/helper/usr/bin && busybox cp $KAIRO_PROVIDER_TOOL_SH/bin/busybox /kairo/package/main/usr/bin/busybox && echo main > /kairo/package/main/usr/bin/main && echo helper > /kairo/package/helper/usr/bin/helper"]
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:sh"
            [[build.targets.children]]
            name = "main"
            root = "main"
            requires = [{{ provides = "tool:helper" }}]
            provides = [{{ provides = "tool:main" }}]
            [[build.targets.children]]
            name = "helper"
            root = "helper"
            provides = [{{ provides = "tool:helper" }}]
            "#
        ),
    )]);
    let plan = world.plan_with(&host, &bundle.object_id, "release");
    let limits = Limits {
        timeout: Duration::from_secs(30),
    };
    let execution = run_plan(
        &world.fixture.store,
        &world.trees,
        &plan,
        1_700_000_000,
        &limits,
        &Engine::Bubblewrap(bwrap.clone()),
    )?;
    assert_eq!(
        execution.record.status,
        Status::Success,
        "{:?} {}",
        execution.record.diagnostics,
        world.blob_text(&execution.record.logs.stderr)
    );
    // The plan says what each child will require.
    assert!(matches!(
        plan.manifest.requires.as_slice(),
        [required] if required.provides == "tool:helper" && required.child.as_deref() == Some("main")
    ));
    let built = BuiltWithPlans(vec![(
        bundle.object_id.clone(),
        "release".to_owned(),
        BlobId::new(execution.record.artifact.clone().expect("artifact id"))?,
        plan.bytes.clone(),
    )]);

    // A consumer names main alone and finds helper beside it.
    let consumer = world.object(&[(
        "kairo.toml",
        &format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "consumer"
            [[dependencies]]
            kind = "provides"
            provides = "{contract}"
            [[build.targets]]
            name = "release"
            command = ["/usr/bin/busybox", "sh", "-c", "ls /usr/bin > /kairo/package/report"]
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:main"
            "#
        ),
    )]);
    let plan = Resolver::new(&world.fixture.store, &world.trees, &built, &host, &[])
        .resolve(&consumer.object_id, &SnapshotSelector::Current, "release")
        .expect("plan")
        .plan;
    assert!(matches!(
        plan.manifest.runtime.as_slice(),
        [entry] if entry.provides == "tool:helper" && entry.via == ["tool:main", "tool:helper"]
    ));
    let execution = run_plan(
        &world.fixture.store,
        &world.trees,
        &plan,
        1_700_000_000,
        &limits,
        &Engine::Bubblewrap(bwrap),
    )?;
    assert_eq!(
        execution.record.status,
        Status::Success,
        "{:?} {}",
        execution.record.diagnostics,
        world.blob_text(&execution.record.logs.stderr)
    );
    let report = world.blob_text(
        execution.artifact.as_ref().expect("artifact").entries[0]
            .blob()
            .expect("a file"),
    );
    assert_eq!(report, "busybox\nhelper\nmain\n");
    Ok(())
}

#[test]
fn coexisting_providers_may_not_put_a_file_at_the_same_path() -> TestResult {
    let Some(bwrap) = engine_fixture() else {
        return Ok(());
    };
    let mut world = World::new();
    world.object(BUSYBOX_PROVIDER);
    let contract = Host::detect().contract();
    let mut host = Host::detect();
    host.engines.push(contract.clone());
    // Two versions of one library, each claiming to co-exist, both
    // shipping usr/bin/busybox.
    let provider = |version: &str| {
        format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "thing"
            [[dependencies]]
            kind = "provides"
            provides = "{contract}"
            [[provides]]
            provides = "lib:thing"
            version = "{version}"
            target = "release"
            coexists = true
            [[build.targets]]
            name = "release"
            command = ["sh", "-c", "busybox mkdir -p /kairo/package/usr/bin && busybox cp $KAIRO_PROVIDER_TOOL_SH/bin/busybox /kairo/package/usr/bin/busybox && echo {version} > /kairo/package/usr/bin/version"]
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:sh"
            "#
        )
    };
    let limits = Limits {
        timeout: Duration::from_secs(30),
    };
    let mut built = Vec::new();
    for version in ["1.0", "2.0"] {
        let object = world.object(&[("kairo.toml", &provider(version))]);
        let plan = world.plan_with(&host, &object.object_id, "release");
        let execution = run_plan(
            &world.fixture.store,
            &world.trees,
            &plan,
            1_700_000_000,
            &limits,
            &Engine::Bubblewrap(bwrap.clone()),
        )?;
        assert_eq!(execution.record.status, Status::Success);
        built.push((
            object.object_id.clone(),
            "release".to_owned(),
            BlobId::new(execution.record.artifact.clone().expect("artifact id"))?,
            plan.bytes,
        ));
    }
    let built = BuiltWithPlans(built);
    let consumer = world.object(&[(
        "kairo.toml",
        &format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "consumer"
            [[dependencies]]
            kind = "provides"
            provides = "{contract}"
            [[build.targets]]
            name = "release"
            command = ["/usr/bin/busybox", "true"]
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "lib:thing"
            version = "=1.0"
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "lib:thing"
            version = "=2.0"
            "#
        ),
    )]);
    let plan = Resolver::new(&world.fixture.store, &world.trees, &built, &host, &[])
        .resolve(&consumer.object_id, &SnapshotSelector::Current, "release")
        .expect("plan")
        .plan;
    // The resolver let both in; the executor refuses the collision.
    assert!(matches!(
        run_plan(
            &world.fixture.store,
            &world.trees,
            &plan,
            1_700_000_000,
            &limits,
            &Engine::Bubblewrap(bwrap),
        ),
        Err(BuildError::CoexistOverlap { label, path }) if label == "lib:thing" && path == "/usr/bin/busybox"
    ));
    Ok(())
}

#[test]
fn a_tree_that_doesnt_build_is_installed_as_its_artifact() -> TestResult {
    let Some(bwrap) = engine_fixture() else {
        return Ok(());
    };
    let mut world = World::new();
    world.object(BUSYBOX_PROVIDER);
    let contract = Host::detect().contract();
    let mut host = Host::detect();
    host.engines.push(contract.clone());
    // The merged-/usr links, committed as links, under files/ (§50).
    world.object(&[
        (
            "kairo.toml",
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "linux-filesystem"
            [content]
            kind = "tree"
            root = "files"
            install = "/"
            [[provides]]
            provides = "data:linux-filesystem:merged-usr"
            version = "1.0.0"
            "#,
        ),
        ("README.md", "The layout.\n"),
        ("files/bin", "@link:usr/bin"),
        ("files/lib", "@link:usr/lib"),
        ("files/sbin", "@link:usr/sbin"),
    ]);
    let consumer = world.object(&[(
        "kairo.toml",
        &format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "consumer"
            [[dependencies]]
            kind = "provides"
            provides = "{contract}"
            [[build.targets]]
            name = "release"
            command = ["sh", "-c", "for d in /bin /lib /sbin; do busybox readlink $d; done > /kairo/package/report && busybox ls / > /kairo/package/root"]
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:sh"
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "data:linux-filesystem:merged-usr"
            "#
        ),
    )]);
    let plan = world.plan_with(&host, &consumer.object_id, "release");
    assert!(plan
        .manifest
        .requirements
        .iter()
        .any(|requirement| matches!(
            &requirement.resolved,
            Resolved::Content { install, root, .. }
                if install.as_deref() == Some("/") && root.as_deref() == Some("files")
        )));
    let execution = run_plan(
        &world.fixture.store,
        &world.trees,
        &plan,
        1_700_000_000,
        &Limits {
            timeout: Duration::from_secs(30),
        },
        &Engine::Bubblewrap(bwrap),
    )?;
    assert_eq!(
        execution.record.status,
        Status::Success,
        "{:?} {}",
        execution.record.diagnostics,
        world.blob_text(&execution.record.logs.stderr)
    );
    let artifact = execution.artifact.as_ref().expect("artifact");
    let file = |name: &str| {
        world.blob_text(
            artifact
                .entries
                .iter()
                .find(|entry| entry.path() == name)
                .and_then(|entry| entry.blob())
                .expect("a file"),
        )
    };
    assert_eq!(file("report"), "usr/bin\nusr/lib\nusr/sbin\n");
    // Only the part under its root: its kairo.toml and README stay out.
    let root = file("root");
    assert!(root.lines().any(|name| name == "lib"), "{root}");
    assert!(
        !root
            .lines()
            .any(|name| name == "kairo.toml" || name == "README.md" || name == "files"),
        "{root}"
    );
    Ok(())
}
