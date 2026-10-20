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
    let paths: Vec<&str> = tree.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "bin/app",
            "share/env",
            "share/greeting",
            "share/nested/epoch"
        ]
    );
    assert!(tree.entries[0].executable);
    assert!(!tree.entries[2].executable);
    assert_eq!(world.blob_text(&tree.entries[2].blob), "hello");
    assert_eq!(world.blob_text(&tree.entries[3].blob), "1700000000\n");
    let env = world.blob_text(&tree.entries[1].blob);
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
fn symlinked_outputs_are_refused() -> TestResult {
    let mut world = World::new();
    let escaping = world.object(&[(
        "kairo.toml",
        &app_manifest(
            r#"["sh", "-c", "mkdir -p out/share && ln -s /etc/passwd out/app"]"#,
            FILE_AND_DIR_OUTPUTS,
            "",
        ),
    )]);
    let plan = world.plan(&escaping.object_id, "release");
    let execution = world.run(&plan, Duration::from_secs(30))?;
    assert_eq!(execution.record.status, Status::Failure);
    assert!(
        execution.record.diagnostics[0].contains("symbolic link"),
        "{:?}",
        execution.record.diagnostics
    );
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
        Bubblewrap::probe(Path::new("/usr/bin/bwrap")).ok()
    })();
    assert!(
        found.is_some() || std::env::var_os("KAIRO_REQUIRE_ENGINE").is_none(),
        "KAIRO_REQUIRE_ENGINE is set but bubblewrap or a static busybox is unavailable"
    );
    found
}

#[test]
fn the_bubblewrap_engine_runs_in_an_empty_root_with_providers_only() -> TestResult {
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
    let report = world.blob_text(&tree.entries[0].blob);
    let lines: Vec<&str> = report.lines().collect();
    assert_eq!(lines[0], "no-usr", "{report}");
    assert_eq!(lines[1], "1000", "{report}");
    assert_eq!(lines[2], "kairo", "{report}");
    assert_eq!(lines[3], "1", "only loopback: {report}");
    assert_eq!(lines[4], "/kairo/home /kairo/src UTC", "{report}");
    assert!(
        lines[5].starts_with("/kairo/content/") && lines[5].ends_with("/bin"),
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
fn a_probe_of_a_missing_engine_says_why() {
    let error = Bubblewrap::probe(Path::new("/nonexistent/bwrap")).expect_err("no such engine");
    assert!(error.contains("cannot run"), "{error}");
}
