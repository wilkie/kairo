use std::collections::HashMap;
use std::path::PathBuf;

use kairo_core::{ObjectId, SnapshotId};
use kairo_object::ObjectManifest;
use kairo_statement::RevisionId;
use kairo_test_support::store::{CreatedActor, CreatedObject, CreatedRevision, StoreFixture};

use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Manifests by `(object, revision)`, standing in for the Git cache.
#[derive(Default)]
struct Manifests(HashMap<(String, String), String>);

impl ManifestSource for Manifests {
    fn manifest(
        &self,
        object: &ObjectId,
        revision: &RevisionId,
    ) -> Result<Option<ObjectManifest>, SourceError> {
        self.0
            .get(&(object.to_string(), revision.as_str().to_owned()))
            .map(|text| ObjectManifest::parse_toml(text).map_err(|e| SourceError(e.to_string())))
            .transpose()
    }
}

struct Held(Vec<(String, String, BlobId)>);

impl ArtifactSource for Held {
    fn artifact(
        &self,
        object: &ObjectId,
        _snapshot: &SnapshotId,
        target: &str,
    ) -> Result<Option<BlobId>, SourceError> {
        Ok(self
            .0
            .iter()
            .find(|(o, t, _)| o == &object.to_string() && t == target)
            .map(|(_, _, id)| id.clone()))
    }
}

struct World {
    _dir: tempfile::TempDir,
    fixture: StoreFixture,
    actor: CreatedActor,
    manifests: Manifests,
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
            manifests: Manifests::default(),
            revisions: 0,
        }
    }

    /// An object with one revision on `head`, its manifest readable.
    fn object(&mut self, manifest: &str) -> (CreatedObject, CreatedRevision) {
        let object = self.fixture.make_object(&self.actor, "software");
        let revision = self.revision(&object, manifest, Vec::new());
        self.fixture
            .set_branch(&self.actor, &object, &revision, "head");
        (object, revision)
    }

    fn revision(
        &mut self,
        object: &CreatedObject,
        manifest: &str,
        parents: Vec<RevisionId>,
    ) -> CreatedRevision {
        self.revisions += 1;
        let id = RevisionId::new(format!("git:sha256:r{}", self.revisions));
        self.manifests.0.insert(
            (object.object_id.to_string(), id.as_str().to_owned()),
            manifest.to_owned(),
        );
        self.fixture
            .make_revision(&self.actor, object, id, manifest, parents)
    }

    fn host() -> Host {
        Host {
            system: "linux".to_owned(),
            arch: "x86-64".to_owned(),
            engines: Vec::new(),
        }
    }

    fn resolve_with(
        &self,
        artifacts: &dyn ArtifactSource,
        host: &Host,
        tools: &[HostTool],
        object: &ObjectId,
        target: &str,
    ) -> Result<Resolution, ResolveError> {
        Resolver::new(&self.fixture.store, &self.manifests, artifacts, host, tools).resolve(
            object,
            &SnapshotSelector::Current,
            target,
        )
    }

    fn resolve(&self, object: &ObjectId, target: &str) -> Result<Resolution, ResolveError> {
        self.resolve_with(&NoArtifacts, &Self::host(), &[], object, target)
    }
}

fn manifest(name: &str, body: &str) -> String {
    format!(
        r#"
        [kairo]
        schema = 1
        kind = "software"
        name = "{name}"
        {body}
        "#
    )
}

const RELEASE_TARGET: &str = r#"
    [[build.targets]]
    name = "release"
    command = ["make"]
    [[build.targets.outputs]]
    name = "bin"
    path = "out/bin"
    artifact_path = "bin"
    kind = "directory"
"#;

fn app_requiring(label: &str, version: Option<&str>) -> String {
    let version = version
        .map(|v| format!("version = \"{v}\""))
        .unwrap_or_default();
    manifest(
        "app",
        &format!(
            r#"
            [[build.targets]]
            name = "release"
            command = ["make"]
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "{label}"
            {version}
            [[build.targets.outputs]]
            name = "bin"
            path = "out/app"
            artifact_path = "bin/app"
            kind = "file"
            "#
        ),
    )
}

fn content_provider(label: &str, version: Option<&str>) -> String {
    let version = version
        .map(|v| format!("version = \"{v}\""))
        .unwrap_or_default();
    manifest(
        "lib",
        &format!(
            r#"
            [[provides]]
            provides = "{label}"
            {version}
            "#
        ),
    )
}

fn artifact_provider(label: &str, extra: &str) -> String {
    manifest(
        "tool",
        &format!(
            r#"
            [[provides]]
            provides = "{label}"
            version = "4.4"
            target = "release"
            output = "bin"
            {RELEASE_TARGET}
            {extra}
            "#
        ),
    )
}

#[test]
fn a_content_provider_resolves_and_the_plan_hash_is_stable() -> TestResult {
    let mut world = World::new();
    let (lib, _) = world.object(&content_provider("lib:zlib:source", Some("1.3.1")));
    let (app, _) = world.object(&app_requiring("lib:zlib:source", Some(">=1.3, <2")));

    let first = world.resolve(&app.object_id, "release")?;
    let second = world.resolve(&app.object_id, "release")?;
    assert_eq!(first.plan.hash, second.plan.hash);
    assert_eq!(first.plan.bytes, second.plan.bytes);
    assert!(first.prerequisites.is_empty());

    let plan = &first.plan.manifest;
    assert_eq!(plan.schema, RESOLVED_MANIFEST_SCHEMA);
    assert_eq!(plan.object, app.object_id.to_string());
    assert_eq!(plan.environment, "none");
    assert!(!plan.host_dependent);
    assert!(plan.builds_first.is_empty());
    assert!(matches!(
        plan.requirements.as_slice(),
        [Requirement { requested: Requested::Provides { provides, version: Some(version) }, resolved: Resolved::Content { object, .. } }]
            if provides == "lib:zlib:source" && version == ">=1.3, <2" && object == &lib.object_id.to_string()
    ));

    let reread = PlanDocument::from_bytes(first.plan.bytes.clone())?;
    assert_eq!(reread, first.plan);
    Ok(())
}

#[test]
fn an_artifact_provider_without_a_result_is_a_build_needed_first() -> TestResult {
    let mut world = World::new();
    let (tool, _) = world.object(&artifact_provider("tool:make", ""));
    let (app, _) = world.object(&app_requiring("tool:make", Some(">=4")));

    let resolution = world.resolve(&app.object_id, "release")?;
    assert_eq!(resolution.prerequisites.len(), 1);
    let first = &resolution.prerequisites[0];
    assert_eq!(first.manifest.object, tool.object_id.to_string());
    assert_eq!(first.manifest.target, "release");
    assert_eq!(
        resolution.plan.manifest.builds_first,
        vec![BuildFirst {
            object: tool.object_id.to_string(),
            snapshot: first.manifest.snapshot.clone(),
            target: "release".to_owned(),
            plan: first.hash.to_string(),
        }]
    );
    assert!(matches!(
        &resolution.plan.manifest.requirements[0].resolved,
        Resolved::Artifact { artifact: None, output, .. } if output == "bin"
    ));

    // With the result held, nothing needs building first and the plan
    // hash changes, since the artifact id is now pinned.
    let artifact = BlobId::from_bytes(b"test", b"artifact");
    let held = Held(vec![(
        tool.object_id.to_string(),
        "release".to_owned(),
        artifact.clone(),
    )]);
    let with = world.resolve_with(&held, &World::host(), &[], &app.object_id, "release")?;
    assert!(with.prerequisites.is_empty());
    assert!(with.plan.manifest.builds_first.is_empty());
    assert!(matches!(
        &with.plan.manifest.requirements[0].resolved,
        Resolved::Artifact { artifact: Some(id), .. } if id == &artifact.to_string()
    ));
    assert_ne!(with.plan.hash, resolution.plan.hash);
    Ok(())
}

#[test]
fn prerequisites_come_dependencies_first_and_once() -> TestResult {
    let mut world = World::new();
    // cc is built from source; make needs cc; app needs make and cc.
    let (cc, _) = world.object(&artifact_provider("tool:cc", ""));
    let (make, _) = world.object(&artifact_provider(
        "tool:make",
        r#"
        [[dependencies]]
        kind = "provides"
        provides = "tool:cc"
        "#,
    ));
    let (app, _) = world.object(&manifest(
        "app",
        r#"
            [[dependencies]]
            kind = "provides"
            provides = "tool:cc"
            [[build.targets]]
            name = "release"
            command = ["make"]
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:make"
            [[build.targets.outputs]]
            name = "bin"
            path = "out"
            artifact_path = "bin"
            kind = "directory"
            "#,
    ));

    let resolution = world.resolve(&app.object_id, "release")?;
    let order: Vec<&str> = resolution
        .prerequisites
        .iter()
        .map(|plan| plan.manifest.object.as_str())
        .collect();
    assert_eq!(
        order,
        [cc.object_id.to_string(), make.object_id.to_string()]
    );
    let first: Vec<&str> = resolution
        .plan
        .manifest
        .builds_first
        .iter()
        .map(|build| build.object.as_str())
        .collect();
    assert_eq!(
        first,
        [cc.object_id.to_string(), make.object_id.to_string()]
    );
    Ok(())
}

#[test]
fn a_cycle_between_artifact_providers_is_refused() -> TestResult {
    let mut world = World::new();
    let (a, _) = world.object(&artifact_provider(
        "tool:a",
        r#"
        [[dependencies]]
        kind = "provides"
        provides = "tool:b"
        "#,
    ));
    world.object(&artifact_provider(
        "tool:b",
        r#"
        [[dependencies]]
        kind = "provides"
        provides = "tool:a"
        "#,
    ));

    let error = world.resolve(&a.object_id, "release").expect_err("cycle");
    assert!(
        matches!(&error, ResolveError::Cycle { chain } if chain.len() == 3),
        "{error}"
    );
    Ok(())
}

#[test]
fn versions_filter_candidates_and_read_leniently() -> TestResult {
    let mut world = World::new();
    let (old, _) = world.object(&content_provider("tool:make", Some("3.81")));
    let (new, _) = world.object(&content_provider("tool:make", Some("4.4.1")));
    let (app, _) = world.object(&app_requiring("tool:make", Some(">=4")));

    let resolution = world.resolve(&app.object_id, "release")?;
    assert!(matches!(
        &resolution.plan.manifest.requirements[0].resolved,
        Resolved::Content { object, .. } if object == &new.object_id.to_string()
    ));
    assert!(resolution.diagnostics.alternatives.is_empty());

    let (lenient, _) = world.object(&app_requiring("tool:make", Some("^3")));
    let resolution = world.resolve(&lenient.object_id, "release")?;
    assert!(matches!(
        &resolution.plan.manifest.requirements[0].resolved,
        Resolved::Content { object, .. } if object == &old.object_id.to_string()
    ));

    let (none, _) = world.object(&app_requiring("tool:make", Some(">=5")));
    let error = world
        .resolve(&none.object_id, "release")
        .expect_err("unsatisfied");
    assert!(
        matches!(&error, ResolveError::Unsatisfied(u) if u.considered.len() == 2),
        "{error}"
    );
    assert!(error.to_string().contains("doesn't satisfy >=5"), "{error}");
    assert_eq!(
        lenient_version("3.81").map(|v| v.to_string()),
        Some("3.81.0".into())
    );
    assert_eq!(
        lenient_version("4").map(|v| v.to_string()),
        Some("4.0.0".into())
    );
    assert_eq!(lenient_version("v4"), None);
    Ok(())
}

#[test]
fn candidates_rank_artifact_then_content_then_host_tool() -> TestResult {
    let mut world = World::new();
    let (content, _) = world.object(&content_provider("tool:make", Some("4.4")));
    let (artifact, _) = world.object(&artifact_provider("tool:make", ""));
    let (app, _) = world.object(&app_requiring("tool:make", None));
    let tools = [HostTool {
        provides: "tool:make".to_owned(),
        version: Some("4.4.1".to_owned()),
        path: PathBuf::from("/usr/bin/make"),
    }];

    let resolution = world.resolve_with(
        &NoArtifacts,
        &World::host(),
        &tools,
        &app.object_id,
        "release",
    )?;
    assert!(matches!(
        &resolution.plan.manifest.requirements[0].resolved,
        Resolved::Artifact { object, .. } if object == &artifact.object_id.to_string()
    ));
    assert!(!resolution.plan.manifest.host_dependent);
    let alternatives = &resolution.diagnostics.alternatives;
    assert_eq!(alternatives.len(), 1);
    assert_eq!(
        alternatives[0].others,
        [
            format!("object:{} (content)", content.object_id),
            "host tool /usr/bin/make".to_owned()
        ]
    );

    // Only the host tool: the plan is host-dependent.
    let (needs_cc, _) = world.object(&app_requiring("tool:cc", None));
    let tools = [HostTool {
        provides: "tool:cc".to_owned(),
        version: None,
        path: PathBuf::from("/usr/bin/cc"),
    }];
    let resolution = world.resolve_with(
        &NoArtifacts,
        &World::host(),
        &tools,
        &needs_cc.object_id,
        "release",
    )?;
    assert!(matches!(
        &resolution.plan.manifest.requirements[0].resolved,
        Resolved::HostTool { path, version: None, .. } if path == "/usr/bin/cc"
    ));
    assert!(resolution.plan.manifest.host_dependent);
    assert!(resolution.diagnostics.alternatives.is_empty());
    Ok(())
}

#[test]
fn environment_labels_resolve_to_an_engine_or_the_host_in_development_mode() -> TestResult {
    let mut world = World::new();
    let (app, _) = world.object(&app_requiring("environment:linux/x86-64", None));

    let dev = world.resolve(&app.object_id, "release")?;
    assert_eq!(dev.plan.manifest.environment, "none");
    assert!(dev.plan.manifest.host_dependent);
    assert!(matches!(
        &dev.plan.manifest.requirements[0].resolved,
        Resolved::Environment { contract, engine: None } if contract == "environment:linux/x86-64"
    ));

    let mut host = World::host();
    host.engines.push("environment:linux/x86-64".to_owned());
    let engine = world.resolve_with(&NoArtifacts, &host, &[], &app.object_id, "release")?;
    assert_eq!(engine.plan.manifest.environment, "environment:linux/x86-64");
    assert!(!engine.plan.manifest.host_dependent);
    assert_ne!(engine.plan.hash, dev.plan.hash);

    // Under the engine, a host tool is not a candidate.
    let (needs_cc, _) = world.object(&manifest(
        "cc-app",
        r#"
        [[dependencies]]
        kind = "provides"
        provides = "environment:linux/x86-64"
        [[build.targets]]
        name = "release"
        command = ["cc"]
        [[build.targets.dependencies]]
        kind = "provides"
        provides = "tool:cc"
        [[build.targets.outputs]]
        name = "bin"
        path = "out"
        artifact_path = "bin"
        kind = "directory"
        "#,
    ));
    let tools = [HostTool {
        provides: "tool:cc".to_owned(),
        version: None,
        path: PathBuf::from("/usr/bin/cc"),
    }];
    let error = world
        .resolve_with(&NoArtifacts, &host, &tools, &needs_cc.object_id, "release")
        .expect_err("host tools are refused under an engine");
    assert!(
        error
            .to_string()
            .contains("not usable under an isolation engine"),
        "{error}"
    );
    let dev = world.resolve_with(
        &NoArtifacts,
        &World::host(),
        &tools,
        &needs_cc.object_id,
        "release",
    )?;
    assert!(dev.plan.manifest.host_dependent);

    let other = Host {
        system: "linux".to_owned(),
        arch: "aarch64".to_owned(),
        engines: Vec::new(),
    };
    let error = world
        .resolve_with(&NoArtifacts, &other, &[], &app.object_id, "release")
        .expect_err("unsatisfied");
    assert!(
        error.to_string().contains("environment:linux/aarch64"),
        "{error}"
    );
    assert_eq!(Host::detect().arch, label_arch(std::env::consts::ARCH));
    assert!(!Host::detect().arch.contains('_'));
    Ok(())
}

#[test]
fn object_dependencies_resolve_by_version_tag_or_pinned_snapshot() -> TestResult {
    let mut world = World::new();
    let (lib, first) = world.object(&manifest("lib", ""));
    world
        .fixture
        .bind_version_tag(&world.actor, &lib, &first, "1.0.0");
    let second = world.revision(
        &lib,
        &manifest("lib", "summary = \"v2\""),
        vec![first.statement.unsigned().body().revision().clone()],
    );
    world
        .fixture
        .bind_version_tag(&world.actor, &lib, &second, "2.0.0");
    let first_snapshot = Snapshot::from_object_revision(&lib.object_id, &first.statement)?;
    let second_snapshot = Snapshot::from_object_revision(&lib.object_id, &second.statement)?;

    let by_version = |req: &str| {
        manifest(
            "app",
            &format!(
                r#"
                [[dependencies]]
                kind = "object"
                object = "{}"
                version = "{req}"
                {RELEASE_TARGET}
                "#,
                lib.object_id
            ),
        )
    };
    let (app, _) = world.object(&by_version("^1"));
    let resolution = world.resolve(&app.object_id, "release")?;
    assert!(matches!(
        &resolution.plan.manifest.requirements[0].resolved,
        Resolved::Content { object, snapshot } if object == &lib.object_id.to_string()
            && snapshot == &first_snapshot.snapshot_id().to_string()
    ));

    let (newest, _) = world.object(&by_version(">=1"));
    let resolution = world.resolve(&newest.object_id, "release")?;
    assert!(matches!(
        &resolution.plan.manifest.requirements[0].resolved,
        Resolved::Content { snapshot, .. } if snapshot == &second_snapshot.snapshot_id().to_string()
    ));

    let (pinned, _) = world.object(&manifest(
        "app",
        &format!(
            r#"
            [[dependencies]]
            kind = "object"
            object = "{}"
            snapshot = "{}"
            {RELEASE_TARGET}
            "#,
            lib.object_id,
            second_snapshot.snapshot_id()
        ),
    ));
    let resolution = world.resolve(&pinned.object_id, "release")?;
    assert!(matches!(
        &resolution.plan.manifest.requirements[0].resolved,
        Resolved::Content { snapshot, .. } if snapshot == &second_snapshot.snapshot_id().to_string()
    ));

    let (missing, _) = world.object(&by_version("^3"));
    let error = world
        .resolve(&missing.object_id, "release")
        .expect_err("unsatisfied");
    assert!(error.to_string().contains("version 2.0.0"), "{error}");
    Ok(())
}

#[test]
fn unknown_targets_missing_trees_and_tampered_manifests_are_errors() -> TestResult {
    let mut world = World::new();
    let (app, revision) = world.object(&app_requiring("tool:make", None));

    let error = world
        .resolve(&app.object_id, "debug")
        .expect_err("no target");
    assert!(matches!(&error, ResolveError::NoTarget { declared, .. } if declared == &["release"]));

    let key = (
        app.object_id.to_string(),
        revision
            .statement
            .unsigned()
            .body()
            .revision()
            .as_str()
            .to_owned(),
    );
    let original = world.manifests.0.remove(&key).expect("manifest");
    let error = world
        .resolve(&app.object_id, "release")
        .expect_err("unavailable");
    assert!(
        matches!(error, ResolveError::ManifestUnavailable { .. }),
        "{error}"
    );

    world
        .manifests
        .0
        .insert(key, original.replace("name = \"app\"", "name = \"evil\""));
    let error = world
        .resolve(&app.object_id, "release")
        .expect_err("mismatch");
    assert!(
        matches!(error, ResolveError::ManifestMismatch { .. }),
        "{error}"
    );

    // A provider whose tree isn't held is skipped, not fatal.
    let mut world = World::new();
    let (lib, lib_revision) = world.object(&content_provider("tool:make", None));
    world.manifests.0.remove(&(
        lib.object_id.to_string(),
        lib_revision
            .statement
            .unsigned()
            .body()
            .revision()
            .as_str()
            .to_owned(),
    ));
    let (app, _) = world.object(&app_requiring("tool:make", None));
    let error = world
        .resolve(&app.object_id, "release")
        .expect_err("unsatisfied");
    assert!(matches!(error, ResolveError::Unsatisfied(_)), "{error}");
    let index = Resolver::new(
        &world.fixture.store,
        &world.manifests,
        &NoArtifacts,
        &World::host(),
        &[],
    )
    .index()?;
    assert_eq!(index.skipped.len(), 1);
    assert_eq!(index.skipped[0].object, lib.object_id);
    assert!(index.skipped[0].reason.contains("not held"));
    Ok(())
}
