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
