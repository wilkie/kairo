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

    /// These trees hold their kairo.toml alone; any other path in them
    /// is an empty directory.
    fn tree_entries(
        &self,
        object: &ObjectId,
        revision: &RevisionId,
        path: &str,
    ) -> Result<Option<Vec<TreeEntry>>, SourceError> {
        Ok(self
            .0
            .contains_key(&(object.to_string(), revision.as_str().to_owned()))
            .then(|| match path {
                "." => vec![("100644".to_owned(), "kairo.toml".to_owned(), String::new())],
                _ => Vec::new(),
            }))
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

    /// An object `actor` creates and makes current at `seconds`.
    fn object_as(&mut self, actor: &CreatedActor, manifest: &str, seconds: i64) -> CreatedObject {
        let object = self.fixture.make_object(actor, "software");
        self.revisions += 1;
        let id = RevisionId::new(format!("git:sha256:r{}", self.revisions));
        self.manifests.0.insert(
            (object.object_id.to_string(), id.as_str().to_owned()),
            manifest.to_owned(),
        );
        let revision = self
            .fixture
            .make_revision(actor, &object, id, manifest, Vec::new());
        self.fixture
            .set_branch_at(actor, &object, &revision, "head", seconds);
        object
    }

    /// Plan `target` of `object` ranking offers by `owners`.
    fn resolve_preferring(
        &self,
        owners: &OwnerPreference,
        object: &ObjectId,
        target: &str,
    ) -> Result<Resolution, ResolveError> {
        Resolver::new(
            &self.fixture.store,
            &self.manifests,
            &NoArtifacts,
            &Self::host(),
            &[],
        )
        .with_owner_preference(owners)
        .resolve(object, &SnapshotSelector::Current, target)
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
        [Requirement { requested: Requested::Provides { provides, version: Some(version), .. }, resolved: Resolved::Content { object, .. }, .. }]
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
            format!(
                "object:{} (content) version 4.4, owner actor:{} (unranked): \
                 content, not an artifact",
                content.object_id, world.actor.actor_id
            ),
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
        Resolved::Content { object, snapshot, .. } if object == &lib.object_id.to_string()
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

#[test]
fn artifact_offers_must_be_built_for_the_consumers_architecture() -> TestResult {
    let mut world = World::new();
    let built_for = |arch: &str| {
        manifest(
            &format!("tool-{arch}"),
            &format!(
                r#"
                [[provides]]
                provides = "tool:kaem"
                target = "release"
                output = "bin"
                [[build.targets]]
                name = "release"
                command = ["make"]
                [[build.targets.dependencies]]
                kind = "provides"
                provides = "environment:linux/{arch}"
                [[build.targets.outputs]]
                name = "bin"
                path = "out/bin"
                artifact_path = "bin"
                kind = "directory"
                "#
            ),
        )
    };
    let (amd64, _) = world.object(&built_for("x86-64"));
    let (x86, _) = world.object(&built_for("x86"));
    let consumer = |arch: &str| {
        manifest(
            "app",
            &format!(
                r#"
                [[dependencies]]
                kind = "provides"
                provides = "environment:linux/{arch}"
                [[build.targets]]
                name = "release"
                command = ["make"]
                [[build.targets.dependencies]]
                kind = "provides"
                provides = "tool:kaem"
                [[build.targets.outputs]]
                name = "bin"
                path = "out"
                artifact_path = "out"
                kind = "directory"
                "#
            ),
        )
    };
    let mut host = World::host();
    host.engines = ["environment:linux/x86-64", "environment:linux/x86"]
        .map(str::to_owned)
        .to_vec();
    for (arch, wanted) in [("x86", &x86), ("x86-64", &amd64)] {
        let (app, _) = world.object(&consumer(arch));
        let resolution = world.resolve_with(&NoArtifacts, &host, &[], &app.object_id, "release")?;
        let chosen = resolution
            .plan
            .manifest
            .requirements
            .iter()
            .find_map(|requirement| match &requirement.resolved {
                Resolved::Artifact { object, .. } => Some(object.clone()),
                _ => None,
            });
        assert_eq!(chosen, Some(wanted.object_id.to_string()), "{arch}");
        // The other architecture's offer was never an alternative.
        assert!(resolution.diagnostics.alternatives.is_empty(), "{arch}");
    }

    // With only the wrong architecture held, nothing satisfies it.
    let mut world = World::new();
    world.object(&built_for("x86-64"));
    let (app, _) = world.object(&consumer("x86"));
    let unsatisfied = world.resolve_with(&NoArtifacts, &host, &[], &app.object_id, "release");
    assert!(
        matches!(&unsatisfied, Err(ResolveError::Unsatisfied(details))
            if details.considered.iter().any(|line| line.contains("runs in environment:linux/x86-64, not x86"))),
        "{unsatisfied:?}"
    );
    Ok(())
}

fn bundle() -> String {
    manifest(
        "bundle",
        r#"
        [[build.targets]]
        name = "release"
        command = ["make"]
        [[build.targets.children]]
        name = "bash"
        root = "bash"
        version = "5.2"
        provides = [{ provides = "tool:bash" }]
        [[build.targets.children]]
        name = "make"
        root = "make"
        version = "4.4"
        install = false
        "#,
    )
}

#[test]
fn a_child_object_resolves_by_label_and_by_id_to_its_parents_build() -> TestResult {
    let mut world = World::new();
    let (parent, revision) = world.object(&bundle());
    // A child's versions are its parent's.
    world
        .fixture
        .bind_version_tag(&world.actor, &parent, &revision, "1.0.0");
    let child = |name: &str| {
        kairo_object::ChildGenesis::new(parent.object_id.clone(), "release", name).object_id()
    };
    let (by_label, _) = world.object(&app_requiring("tool:bash", Some(">=5")));
    let (by_id, _) = world.object(&manifest(
        "app",
        &format!(
            r#"
            [[dependencies]]
            kind = "object"
            object = "{}"
            version = "^4"
            {RELEASE_TARGET}
            "#,
            child("make")
        ),
    ));

    // Unbuilt, the parent's target is what builds first.
    let resolution = world.resolve(&by_label.object_id, "release")?;
    assert_eq!(resolution.prerequisites.len(), 1);
    assert_eq!(
        resolution.prerequisites[0].manifest.object,
        parent.object_id.to_string()
    );
    assert!(matches!(
        &resolution.plan.manifest.requirements[0].resolved,
        Resolved::Artifact { object, artifact: None, root: Some(root), install: Some(install), .. }
            if object == &child("bash").to_string() && root == "bash" && install == "/"
    ));

    // Built, both children are slices of the parent's one artifact.
    let artifact = BlobId::from_bytes(b"test", b"bundle");
    let held = Held(vec![(
        parent.object_id.to_string(),
        "release".to_owned(),
        artifact.clone(),
    )]);
    let with = world.resolve_with(&held, &World::host(), &[], &by_label.object_id, "release")?;
    assert!(with.plan.manifest.builds_first.is_empty());
    let id = world.resolve_with(&held, &World::host(), &[], &by_id.object_id, "release")?;
    assert!(id.prerequisites.is_empty());
    assert!(matches!(
        &id.plan.manifest.requirements[0].resolved,
        Resolved::Artifact { object, artifact: Some(held), root: Some(root), install: None, target, .. }
            if object == &child("make").to_string()
                && held == &artifact.to_string()
                && root == "make"
                && target == "release"
    ));

    // The labels a held object's children provide are listed.
    let index = Index::build(&world.fixture.store, &world.manifests)?;
    assert!(index
        .all_offers()
        .get("tool:bash")
        .is_some_and(|offers| offers.iter().any(|offer| offer.object == child("bash"))));
    Ok(())
}

#[test]
fn the_highest_version_that_satisfies_every_requirement_is_chosen() -> TestResult {
    let mut world = World::new();
    let mut tools = HashMap::new();
    for version in ["1.0", "2.0", "1.5"] {
        let (tool, _) = world.object(&manifest(
            "tool",
            &format!(
                r#"
                [[provides]]
                provides = "tool:x"
                version = "{version}"
                target = "release"
                output = "bin"
                {RELEASE_TARGET}
                "#
            ),
        ));
        tools.insert(version, tool.object_id.to_string());
    }
    let chosen =
        |resolution: &Resolution, index: usize| match &resolution.plan.manifest.requirements[index]
            .resolved
        {
            Resolved::Artifact { object, .. } => object.clone(),
            _ => String::new(),
        };

    // Without a version, the highest.
    let (any, _) = world.object(&app_requiring("tool:x", None));
    assert_eq!(
        chosen(&world.resolve(&any.object_id, "release")?, 0),
        tools["2.0"]
    );
    // With one, the highest that satisfies it.
    let (one, _) = world.object(&app_requiring("tool:x", Some("^1")));
    assert_eq!(
        chosen(&world.resolve(&one.object_id, "release")?, 0),
        tools["1.5"]
    );
    // Another requirement on the label in the same target narrows what
    // the unversioned one takes.
    let (both, _) = world.object(&manifest(
        "app",
        r#"
        [[build.targets]]
        name = "release"
        command = ["make"]
        [[build.targets.dependencies]]
        kind = "provides"
        provides = "tool:x"
        [[build.targets.dependencies]]
        kind = "provides"
        provides = "tool:x"
        version = "<1.5"
        "#,
    ));
    let resolution = world.resolve(&both.object_id, "release")?;
    assert_eq!(chosen(&resolution, 0), tools["1.0"]);
    assert_eq!(chosen(&resolution, 1), tools["1.0"]);
    Ok(())
}

#[test]
fn an_object_dependency_without_a_selector_takes_the_highest_version_tag() -> TestResult {
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
    let (untagged, _) = world.object(&manifest("untagged", ""));
    let app = |object: &ObjectId| {
        manifest(
            "app",
            &format!(
                r#"
                [[dependencies]]
                kind = "object"
                object = "{object}"
                {RELEASE_TARGET}
                "#
            ),
        )
    };
    let (by_id, _) = world.object(&app(&lib.object_id));
    let resolution = world.resolve(&by_id.object_id, "release")?;
    let second_snapshot = Snapshot::from_object_revision(&lib.object_id, &second.statement)?;
    assert!(matches!(
        &resolution.plan.manifest.requirements[0].resolved,
        Resolved::Content { snapshot, .. } if snapshot == &second_snapshot.snapshot_id().to_string()
    ));
    // An object with no versions needs a snapshot named.
    let (no_tags, _) = world.object(&app(&untagged.object_id));
    assert!(matches!(
        world.resolve(&no_tags.object_id, "release"),
        Err(ResolveError::Unsatisfied(_))
    ));
    Ok(())
}

#[test]
fn a_child_is_chosen_by_its_own_version_across_its_parents_snapshots() -> TestResult {
    let mut world = World::new();
    let declaring = |make: &str, extra: &str| {
        manifest(
            "bundle",
            &format!(
                r#"
                [[build.targets]]
                name = "release"
                command = ["make"]
                [[build.targets.children]]
                name = "make"
                root = "make"
                version = "{make}"
                {extra}
                "#
            ),
        )
    };
    // Revision 1, tagged 1.0.0: make 4.3 and a child later dropped.
    let (parent, first) = world.object(&declaring(
        "4.3",
        "[[build.targets.children]]\nname = \"gone\"\nroot = \"gone\"\nversion = \"1.0\"",
    ));
    world
        .fixture
        .bind_version_tag(&world.actor, &parent, &first, "1.0.0");
    // Revision 2, the head: make 4.4, untagged.
    let second = world.revision(
        &parent,
        &declaring("4.4", ""),
        vec![first.statement.unsigned().body().revision().clone()],
    );
    world
        .fixture
        .set_branch(&world.actor, &parent, &second, "head");
    let first_snapshot = Snapshot::from_object_revision(&parent.object_id, &first.statement)?;
    let child = |name: &str| {
        kairo_object::ChildGenesis::new(parent.object_id.clone(), "release", name).object_id()
    };
    let snapshot_for =
        |world: &mut World, object: ObjectId, selector: &str| -> Result<String, ResolveError> {
            let (app, _) = world.object(&manifest(
                "app",
                &format!(
                    r#"
                [[dependencies]]
                kind = "object"
                object = "{object}"
                {selector}
                {RELEASE_TARGET}
                "#
                ),
            ));
            let resolution = world.resolve(&app.object_id, "release")?;
            Ok(match &resolution.plan.manifest.requirements[0].resolved {
                Resolved::Artifact { snapshot, .. } => snapshot.clone(),
                _ => String::new(),
            })
        };
    let first_id = first_snapshot.snapshot_id().to_string();
    let second_id = current_snapshot(&world.fixture.store, &parent.object_id)?
        .ok_or("a head")?
        .snapshot_id()
        .to_string();
    // The child's version picks the snapshot, not the parent's tag.
    assert_eq!(snapshot_for(&mut world, child("make"), "")?, second_id);
    assert_eq!(
        snapshot_for(&mut world, child("make"), "version = \"^4\"")?,
        second_id
    );
    assert_eq!(
        snapshot_for(&mut world, child("make"), "version = \"~4.3\"")?,
        first_id
    );
    assert!(matches!(
        snapshot_for(&mut world, child("make"), "version = \"^1\""),
        Err(ResolveError::Unsatisfied(_))
    ));
    // Pinned to the parent's tagged snapshot.
    assert_eq!(
        snapshot_for(
            &mut world,
            child("make"),
            &format!("snapshot = \"{first_id}\"")
        )?,
        first_id
    );
    // Declared only at the tagged revision, it's still found there.
    assert_eq!(snapshot_for(&mut world, child("gone"), "")?, first_id);
    Ok(())
}

/// Artifacts with the plans that built them, as a store with build
/// results would give them.
struct HeldWithPlans {
    artifacts: Vec<(String, String, BlobId)>,
    plans: Vec<(String, String, Vec<u8>)>,
}

impl ArtifactSource for HeldWithPlans {
    fn artifact(
        &self,
        object: &ObjectId,
        _snapshot: &SnapshotId,
        target: &str,
    ) -> Result<Option<BlobId>, SourceError> {
        Ok(self
            .artifacts
            .iter()
            .find(|(o, t, _)| o == &object.to_string() && t == target)
            .map(|(_, _, id)| id.clone()))
    }

    fn build_plan(
        &self,
        object: &ObjectId,
        _snapshot: &SnapshotId,
        target: &str,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        Ok(self
            .plans
            .iter()
            .find(|(o, t, _)| o == &object.to_string() && t == target)
            .map(|(_, _, bytes)| bytes.clone()))
    }
}

/// A toolchain bundle: g++ requires binutils wherever it is installed,
/// and confers its own libstdc++ on whatever it builds (§46).
fn toolchain() -> String {
    manifest(
        "toolchain",
        r#"
        [[build.targets]]
        name = "release"
        command = ["make"]
        [[build.targets.children]]
        name = "g++"
        root = "g++"
        version = "11.4.0"
        requires = [{ provides = "tool:binutils" }]
        provides = [{ provides = "tool:g++" }]
        [[build.targets.children]]
        name = "libstdc++"
        root = "libstdc++"
        version = "11.4.0"
        provides = [{ provides = "lib:stdc++", confers = [{ provides = "lib:stdc++" }] }]
        "#,
    )
}

fn runtime_labels(resolution: &Resolution) -> Vec<(String, Vec<String>)> {
    resolution
        .plan
        .manifest
        .runtime
        .iter()
        .map(|entry| (entry.provides.clone(), entry.via.clone()))
        .collect()
}

#[test]
fn a_build_dependency_requires_and_confers_through_the_plan() -> TestResult {
    let mut world = World::new();
    let (toolchain, _) = world.object(&toolchain());
    let (binutils, _) = world.object(&artifact_provider("tool:binutils", ""));
    let child = |name: &str| {
        kairo_object::ChildGenesis::new(toolchain.object_id.clone(), "release", name).object_id()
    };
    // The app builds with g++ and links libstdc++, which it names
    // itself: binutils joins its build environment as what g++ needs,
    // and its artifact will require the libstdc++ it chose, pinned to
    // that one, since libstdc++ confers itself without a version (§51).
    let (app, _) = world.object(&manifest(
        "app",
        r#"
        [[provides]]
        provides = "tool:app"
        target = "release"
        [[build.targets]]
        name = "release"
        command = ["make"]
        [[build.targets.dependencies]]
        kind = "provides"
        provides = "tool:g++"
        [[build.targets.dependencies]]
        kind = "provides"
        provides = "lib:stdc++"
        "#,
    ));
    let resolution = world.resolve(&app.object_id, "release")?;
    assert_eq!(
        runtime_labels(&resolution),
        [(
            "tool:binutils".to_owned(),
            vec!["tool:g++".to_owned(), "tool:binutils".to_owned()]
        )]
    );
    assert!(matches!(
        &resolution.plan.manifest.runtime[0].resolved,
        Resolved::Artifact { object, .. } if object == &binutils.object_id.to_string()
    ));
    let [required] = resolution.plan.manifest.requires.as_slice() else {
        return Err(format!("{:?}", resolution.plan.manifest.requires).into());
    };
    assert_eq!(required.provides, "lib:stdc++");
    assert_eq!(
        required.pin.as_ref().map(|pin| pin.object.clone()),
        Some(child("libstdc++").to_string())
    );
    assert_eq!(
        required.via,
        format!("conferred by lib:stdc++ from object:{}", child("libstdc++"))
    );
    // The binutils build is needed first, as the toolchain's is.
    let first: Vec<&str> = resolution
        .plan
        .manifest
        .builds_first
        .iter()
        .map(|b| b.object.as_str())
        .collect();
    assert!(
        first.contains(&toolchain.object_id.to_string().as_str()),
        "{first:?}"
    );
    assert!(
        first.contains(&binutils.object_id.to_string().as_str()),
        "{first:?}"
    );

    // With the app built, a consumer of tool:app gets libstdc++ too,
    // pinned to the toolchain's, read from the plan that built the app.
    let artifact = BlobId::from_bytes(b"test", b"app");
    let held = HeldWithPlans {
        artifacts: vec![(app.object_id.to_string(), "release".to_owned(), artifact)],
        plans: vec![(
            app.object_id.to_string(),
            "release".to_owned(),
            resolution.plan.bytes.clone(),
        )],
    };
    let (user, _) = world.object(&app_requiring("tool:app", None));
    let using = world.resolve_with(&held, &World::host(), &[], &user.object_id, "release")?;
    assert_eq!(
        runtime_labels(&using),
        [(
            "lib:stdc++".to_owned(),
            vec!["tool:app".to_owned(), "lib:stdc++".to_owned()]
        )]
    );
    assert!(matches!(
        &using.plan.manifest.runtime[0].resolved,
        Resolved::Artifact { object, root: Some(root), .. }
            if object == &child("libstdc++").to_string() && root == "libstdc++"
    ));
    // Nothing is conferred onward by merely using the app.
    assert!(using.plan.manifest.requires.is_empty());
    Ok(())
}

/// A tool whose label confers `lib:stdc++` with the given requirement.
fn conferring_tool(name: &str, label: &str, confers: &str) -> String {
    manifest(
        name,
        &format!(
            r#"
            [[provides]]
            provides = "{label}"
            target = "release"
            confers = [{{ provides = "lib:stdc++"{confers} }}]
            {RELEASE_TARGET}
            "#
        ),
    )
}

fn stdcxx(version: &str, coexists: bool) -> String {
    manifest(
        "libstdc++",
        &format!(
            r#"
            [[provides]]
            provides = "lib:stdc++"
            version = "{version}"
            target = "release"
            coexists = {coexists}
            {RELEASE_TARGET}
            "#
        ),
    )
}

#[test]
fn ranges_unify_to_the_highest_version_that_satisfies_them_all() -> TestResult {
    let mut world = World::new();
    world.object(&stdcxx("11.4.0", false));
    let (thirteen, _) = world.object(&stdcxx("13.2.0", false));
    world.object(&conferring_tool("foo", "tool:foo", r#", version = ">=11""#));
    world.object(&conferring_tool("bar", "tool:bar", r#", version = ">=13""#));
    // The app's own artifact requires both tools' libstdc++ ranges;
    // one that uses the app gets a single libstdc++ at 13.
    let (app, _) = world.object(&manifest(
        "app",
        r#"
        [[provides]]
        provides = "tool:app"
        target = "release"
        [[build.targets]]
        name = "release"
        command = ["make"]
        [[build.targets.dependencies]]
        kind = "provides"
        provides = "tool:foo"
        [[build.targets.dependencies]]
        kind = "provides"
        provides = "tool:bar"
        "#,
    ));
    let plan = world.resolve(&app.object_id, "release")?.plan;
    let versions: Vec<Option<&str>> = plan
        .manifest
        .requires
        .iter()
        .map(|r| r.version.as_deref())
        .collect();
    assert_eq!(versions, [Some(">=11"), Some(">=13")]);
    let held = HeldWithPlans {
        artifacts: vec![(
            app.object_id.to_string(),
            "release".to_owned(),
            BlobId::from_bytes(b"test", b"app"),
        )],
        plans: vec![(app.object_id.to_string(), "release".to_owned(), plan.bytes)],
    };
    let (user, _) = world.object(&app_requiring("tool:app", None));
    let using = world.resolve_with(&held, &World::host(), &[], &user.object_id, "release")?;
    assert_eq!(using.plan.manifest.runtime.len(), 1);
    assert!(matches!(
        &using.plan.manifest.runtime[0].resolved,
        Resolved::Artifact { object, .. } if object == &thirteen.object_id.to_string()
    ));
    Ok(())
}

#[test]
fn a_closure_constraint_re_chooses_a_declared_requirement() -> TestResult {
    let mut world = World::new();
    let mut xs = HashMap::new();
    for version in ["1.0", "2.0"] {
        let (x, _) = world.object(&manifest(
            "x",
            &format!(
                r#"
                [[provides]]
                provides = "tool:x"
                version = "{version}"
                target = "release"
                {RELEASE_TARGET}
                "#
            ),
        ));
        xs.insert(version, x.object_id.to_string());
    }
    // y needs an x below 2 beside it; the app, which wanted any x,
    // takes the one that serves both.
    world.object(&manifest(
        "y",
        r#"
        [[provides]]
        provides = "tool:y"
        target = "release"
        [[build.targets]]
        name = "release"
        command = ["make"]
        requires = [{ provides = "tool:x", version = "<2" }]
        "#,
    ));
    let (app, _) = world.object(&manifest(
        "app",
        r#"
        [[build.targets]]
        name = "release"
        command = ["make"]
        [[build.targets.dependencies]]
        kind = "provides"
        provides = "tool:x"
        [[build.targets.dependencies]]
        kind = "provides"
        provides = "tool:y"
        "#,
    ));
    let resolution = world.resolve(&app.object_id, "release")?;
    assert!(matches!(
        &resolution.plan.manifest.requirements[0].resolved,
        Resolved::Artifact { object, .. } if object == &xs["1.0"]
    ));
    // The declared x serves y's requirement: nothing extra is installed.
    assert!(resolution.plan.manifest.runtime.is_empty());
    Ok(())
}

#[test]
fn pins_that_no_one_offer_satisfies_conflict_unless_they_coexist() -> TestResult {
    for coexists in [false, true] {
        let mut world = World::new();
        // The app links libstdc++ 11, which confers itself, pinned;
        // bar wants >=13.
        world.object(&stdcxx("13.2.0", coexists));
        let (baz, _) = world.object(&manifest(
            "baz",
            &format!(
                r#"
                [[provides]]
                provides = "lib:stdc++"
                version = "11.0.0"
                target = "release"
                confers = [{{ provides = "lib:stdc++" }}]
                coexists = {coexists}
                {RELEASE_TARGET}
                "#
            ),
        ));
        world.object(&conferring_tool("bar", "tool:bar", r#", version = ">=13""#));
        let (app, _) = world.object(&manifest(
            "app",
            r#"
            [[provides]]
            provides = "tool:app"
            target = "release"
            [[build.targets]]
            name = "release"
            command = ["make"]
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "lib:stdc++"
            version = "<12"
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "tool:bar"
            "#,
        ));
        let plan = world.resolve(&app.object_id, "release")?.plan;
        assert_eq!(
            plan.manifest.requires[0]
                .pin
                .as_ref()
                .map(|pin| pin.object.clone()),
            Some(baz.object_id.to_string())
        );
        let held = HeldWithPlans {
            artifacts: vec![(
                app.object_id.to_string(),
                "release".to_owned(),
                BlobId::from_bytes(b"test", b"app"),
            )],
            plans: vec![(app.object_id.to_string(), "release".to_owned(), plan.bytes)],
        };
        let (user, _) = world.object(&app_requiring("tool:app", None));
        let using = world.resolve_with(&held, &World::host(), &[], &user.object_id, "release");
        if coexists {
            let using = using?;
            let labels: Vec<&str> = using
                .plan
                .manifest
                .runtime
                .iter()
                .map(|entry| entry.provides.as_str())
                .collect();
            assert_eq!(labels, ["lib:stdc++", "lib:stdc++"]);
        } else {
            let error = using.expect_err("a conflict");
            assert!(
                matches!(&error, ResolveError::Conflict { label, unmet, .. } if label == "lib:stdc++" && unmet.len() == 2),
                "{error}"
            );
            let text = error.to_string();
            assert!(text.contains("pinned to object:"), "{text}");
            assert!(
                text.contains(">=13, required via tool:app > lib:stdc++"),
                "{text}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_child_named_by_id_brings_what_it_requires() -> TestResult {
    let mut world = World::new();
    let (toolchain, _) = world.object(&toolchain());
    let (binutils, _) = world.object(&artifact_provider("tool:binutils", ""));
    let gxx =
        kairo_object::ChildGenesis::new(toolchain.object_id.clone(), "release", "g++").object_id();
    let (app, _) = world.object(&manifest(
        "app",
        &format!(
            r#"
            [[build.targets]]
            name = "release"
            command = ["make"]
            [[build.targets.dependencies]]
            kind = "object"
            object = "{gxx}"
            "#
        ),
    ));
    let resolution = world.resolve(&app.object_id, "release")?;
    assert_eq!(
        runtime_labels(&resolution),
        [(
            "tool:binutils".to_owned(),
            vec![format!("object:{gxx}"), "tool:binutils".to_owned()]
        )]
    );
    assert!(matches!(
        &resolution.plan.manifest.runtime[0].resolved,
        Resolved::Artifact { object, .. } if object == &binutils.object_id.to_string()
    ));
    // A label confers; an id names no label, so nothing is conferred.
    assert!(resolution.plan.manifest.requires.is_empty());
    Ok(())
}

/// The content object `resolution`'s first requirement resolved to.
fn first_content(resolution: &Resolution) -> Option<&str> {
    match &resolution.plan.manifest.requirements[0].resolved {
        Resolved::Content { object, .. } => Some(object),
        _ => None,
    }
}

#[test]
fn an_owners_tier_outranks_a_higher_version() -> TestResult {
    let mut world = World::new();
    let stranger = world.fixture.make_actor();
    let trusted = world.fixture.make_actor();
    let named = world.fixture.make_actor();
    // A local actor that offers nothing itself, and trusts one owner.
    let operator = world.fixture.make_actor();
    world.fixture.trust(&operator, &trusted.actor_id);
    let theirs = world.object_as(&stranger, &content_provider("tool:make", Some("9.0")), 1);
    let trusteds = world.object_as(&trusted, &content_provider("tool:make", Some("5.0")), 1);
    let nameds = world.object_as(&named, &content_provider("tool:make", Some("3.0")), 1);
    let (app, _) = world.object(&app_requiring("tool:make", None));
    let (picky, _) = world.object(&app_requiring("tool:make", Some(">=6")));
    let chosen = |owners: &OwnerPreference, app: &ObjectId| -> Result<String, ResolveError> {
        let resolution = world.resolve_preferring(owners, app, "release")?;
        Ok(first_content(&resolution).unwrap_or_default().to_owned())
    };

    // With no preference, the highest version, as before.
    let none = OwnerPreference::default();
    assert_eq!(chosen(&none, &app.object_id)?, theirs.object_id.to_string());
    // A local actor's trust ranks its owner above a stranger.
    let local = OwnerPreference {
        preferred: Vec::new(),
        local: vec![operator.actor_id.clone()],
    };
    assert_eq!(
        chosen(&local, &app.object_id)?,
        trusteds.object_id.to_string()
    );
    // A named owner above both, at a lower version still.
    let prefer = OwnerPreference {
        preferred: vec![named.actor_id.clone()],
        local: vec![operator.actor_id.clone()],
    };
    assert_eq!(
        chosen(&prefer, &app.object_id)?,
        nameds.object_id.to_string()
    );
    let resolution = world.resolve_preferring(&prefer, &app.object_id, "release")?;
    let others = &resolution.diagnostics.alternatives[0].others;
    assert_eq!(others.len(), 2, "{others:?}");
    assert!(
        others[0].contains(&format!(
            "owner actor:{} (trusted): its owner ranks lower",
            trusted.actor_id
        )),
        "{others:?}"
    );
    assert!(
        others[1].contains("(unranked): its owner ranks lower"),
        "{others:?}"
    );
    // A stranger's offer serves when no ranked one satisfies.
    assert_eq!(
        chosen(&prefer, &picky.object_id)?,
        theirs.object_id.to_string()
    );
    Ok(())
}

#[test]
fn among_equals_the_newest_published_wins_then_the_object_id() -> TestResult {
    let mut world = World::new();
    let actor = world.actor.clone();
    let older = world.object_as(&actor, &content_provider("tool:make", Some("4.4")), 100);
    let newer = world.object_as(&actor, &content_provider("tool:make", Some("4.4")), 200);
    let (app, _) = world.object(&app_requiring("tool:make", None));
    let resolution = world.resolve(&app.object_id, "release")?;
    assert_eq!(first_content(&resolution), Some(newer.object_id.as_str()));
    let others = &resolution.diagnostics.alternatives[0].others;
    assert_eq!(others.len(), 1);
    assert!(
        others[0].starts_with(&format!("object:{} (content)", older.object_id))
            && others[0].ends_with(": published earlier"),
        "{others:?}"
    );

    // Published together, the lower object id.
    let mut world = World::new();
    let actor = world.actor.clone();
    let a = world.object_as(&actor, &content_provider("tool:make", Some("4.4")), 100);
    let b = world.object_as(&actor, &content_provider("tool:make", Some("4.4")), 100);
    let (app, _) = world.object(&app_requiring("tool:make", None));
    let resolution = world.resolve(&app.object_id, "release")?;
    let first = a.object_id.clone().min(b.object_id.clone());
    assert_eq!(first_content(&resolution), Some(first.as_str()));
    assert!(resolution.diagnostics.alternatives[0].others[0].ends_with(": after it by object id"));
    Ok(())
}

#[test]
fn a_release_candidate_gives_way_to_the_release() -> TestResult {
    let mut world = World::new();
    let actor = world.actor.clone();
    let candidate = world.object_as(
        &actor,
        &content_provider("tool:gcc", Some("4.0.4-rc.1")),
        200,
    );
    let (any, _) = world.object(&app_requiring("tool:gcc", None));
    let (major, _) = world.object(&app_requiring("tool:gcc", Some("^4")));
    let (asks_rc, _) = world.object(&app_requiring("tool:gcc", Some(">=4.0.4-rc.1")));
    // Alone, the candidate serves what names no version, and what
    // names a pre-release; a plain range doesn't take one.
    let resolution = world.resolve(&any.object_id, "release")?;
    assert_eq!(
        first_content(&resolution),
        Some(candidate.object_id.as_str())
    );
    assert!(world.resolve(&major.object_id, "release").is_err());
    let release = world.object_as(&actor, &content_provider("tool:gcc", Some("4.0.4")), 100);
    for app in [&any, &major, &asks_rc] {
        let resolution = world.resolve(&app.object_id, "release")?;
        assert_eq!(first_content(&resolution), Some(release.object_id.as_str()));
    }
    Ok(())
}

#[test]
fn a_tree_that_doesnt_build_is_installed_as_what_requires_it_needs() -> TestResult {
    let mut world = World::new();
    // The layout: its own tree, the part under files/, installed at /.
    let (layout, _) = world.object(&manifest(
        "linux-filesystem",
        r#"
        [content]
        kind = "tree"
        root = "files"
        install = "/"
        [[provides]]
        provides = "data:linux-filesystem:merged-usr"
        version = "1.0.0"
        "#,
    ));
    let (_libc, _) = world.object(&manifest(
        "libc",
        r#"
        [[build.targets]]
        name = "release"
        command = ["make"]
        [[build.targets.children]]
        name = "libc"
        root = "libc"
        version = "1.2.5"
        requires = [{ provides = "data:linux-filesystem:merged-usr" }]
        provides = [{ provides = "lib:c" }]
        "#,
    ));
    let (app, _) = world.object(&app_requiring("lib:c", None));
    let resolution = world.resolve(&app.object_id, "release")?;
    assert_eq!(
        runtime_labels(&resolution),
        [(
            "data:linux-filesystem:merged-usr".to_owned(),
            vec![
                "lib:c".to_owned(),
                "data:linux-filesystem:merged-usr".to_owned()
            ]
        )]
    );
    assert!(
        matches!(
            &resolution.plan.manifest.runtime[0].resolved,
            Resolved::Content { object, install, root, .. }
                if object == &layout.object_id.to_string()
                    && install.as_deref() == Some("/")
                    && root.as_deref() == Some("files")
        ),
        "{:?}",
        resolution.plan.manifest.runtime
    );
    // A tree that says neither is resolved as before, its plan entry
    // spelling no install or root, only what it holds (§52).
    let (lib, _) = world.object(&content_provider("lib:zlib:source", Some("1.3.1")));
    let (plain, _) = world.object(&app_requiring("lib:zlib:source", None));
    let resolution = world.resolve(&plain.object_id, "release")?;
    let resolved = &resolution.plan.manifest.requirements[0].resolved;
    let Resolved::Content {
        snapshot,
        tree: Some(tree),
        ..
    } = resolved
    else {
        return Err(format!("not content: {resolved:?}").into());
    };
    assert_eq!(
        serde_json::to_string(resolved)?,
        format!(
            r#"{{"kind":"content","object":"{}","snapshot":"{snapshot}","tree":"{tree}"}}"#,
            lib.object_id
        )
    );
    Ok(())
}

#[test]
fn one_of_obliges_the_build_and_only_its_own_choices_confer() -> TestResult {
    let mut world = World::new();
    // A libc in three parts, only the shared one conferring itself, as
    // a range (§51); and a compiler that needs its headers and one of
    // the other two, chosen by whatever builds with it.
    world.object(&manifest(
        "libc",
        r#"
        [[build.targets]]
        name = "release"
        command = ["make"]
        [[build.targets.children]]
        name = "headers"
        root = "headers"
        version = "1.2.5"
        provides = [{ provides = "lib:c:headers" }]
        [[build.targets.children]]
        name = "static"
        root = "static"
        version = "1.2.5"
        provides = [{ provides = "lib:c:static" }]
        [[build.targets.children]]
        name = "shared"
        root = "shared"
        version = "1.2.5"
        provides = [{ provides = "lib:c:shared", confers = [{ provides = "lib:c:shared", version = "^1.2.5" }] }]
        "#,
    ));
    world.object(&manifest(
        "cc",
        r#"
        [[build.targets]]
        name = "release"
        command = ["make"]
        [[build.targets.children]]
        name = "cc"
        root = "cc"
        version = "4.7.4"
        requires = [{ provides = "lib:c:headers" }, { one_of = [{ provides = "lib:c:static" }, { provides = "lib:c:shared" }] }]
        provides = [{ provides = "tool:cc" }]
        "#,
    ));
    let app = |world: &mut World, libc: &str| {
        world
            .object(&manifest(
                "app",
                &format!(
                    r#"
                    [[build.targets]]
                    name = "release"
                    command = ["make"]
                    [[build.targets.dependencies]]
                    kind = "provides"
                    provides = "tool:cc"
                    {libc}
                    "#
                ),
            ))
            .0
            .object_id
    };
    let dependency = |label: &str, extra: &str| {
        format!(
            "[[build.targets.dependencies]]\nkind = \"provides\"\nprovides = \"{label}\"\n{extra}"
        )
    };
    let requires = |resolution: &Resolution| -> Vec<(String, Option<String>, bool)> {
        resolution
            .plan
            .manifest
            .requires
            .iter()
            .map(|entry| {
                (
                    entry.provides.clone(),
                    entry.version.clone(),
                    entry.pin.is_some(),
                )
            })
            .collect()
    };

    // Naming neither: the compiler's obligation is unmet.
    let neither = app(&mut world, "");
    let error = world.resolve(&neither, "release").expect_err("unmet");
    assert!(
        matches!(&error, ResolveError::ObligationUnmet { alternatives, .. }
            if alternatives == &["lib:c:static".to_owned(), "lib:c:shared".to_owned()]),
        "{error}"
    );
    assert!(
        error.to_string().contains(
            "tool:cc needs this build to require one of lib:c:static, lib:c:shared itself"
        ),
        "{error}"
    );

    // Shared: the build gets the headers through the compiler, and the
    // artifact requires the shared libc at the range it gives.
    let shared = app(&mut world, &dependency("lib:c:shared", ""));
    let resolution = world.resolve(&shared, "release")?;
    assert_eq!(
        runtime_labels(&resolution),
        [(
            "lib:c:headers".to_owned(),
            vec!["tool:cc".to_owned(), "lib:c:headers".to_owned()]
        )]
    );
    assert_eq!(
        requires(&resolution),
        [("lib:c:shared".to_owned(), Some("^1.2.5".to_owned()), false)]
    );
    // The compiler confers nothing; static linking needs nothing later.
    let static_ = app(&mut world, &dependency("lib:c:static", ""));
    assert!(requires(&world.resolve(&static_, "release")?).is_empty());
    // The build may refuse what a dependency would confer, or confer
    // what it wouldn't: pinned to what it resolved, having no range.
    let refused = app(&mut world, &dependency("lib:c:shared", "confer = false"));
    assert!(requires(&world.resolve(&refused, "release")?).is_empty());
    let kept = app(&mut world, &dependency("lib:c:static", "confer = true"));
    assert_eq!(
        requires(&world.resolve(&kept, "release")?),
        [("lib:c:static".to_owned(), None, true)]
    );
    Ok(())
}

/// An object that doesn't build, providing `label`, with `dependencies`
/// (TOML `[[dependencies]]` entries) for a build that depends on it to
/// take as its own (§57).
fn environment_object(name: &str, label: &str, dependencies: &str) -> String {
    manifest(
        name,
        &format!(
            r#"
            [[provides]]
            provides = "{label}"
            version = "1.0.0"
            {dependencies}
            "#
        ),
    )
}

fn top_dependency(label: &str, version: Option<&str>) -> String {
    let version = version
        .map(|v| format!("version = \"{v}\"\n"))
        .unwrap_or_default();
    format!("[[dependencies]]\nkind = \"provides\"\nprovides = \"{label}\"\n{version}")
}

/// Each requirement's label, or `object:<id>` for an object dependency,
/// what it resolved to, and who brought it.
fn requirement_summary(resolution: &Resolution) -> Vec<(String, String, Option<String>)> {
    resolution
        .plan
        .manifest
        .requirements
        .iter()
        .map(|requirement| {
            let requested = match &requirement.requested {
                Requested::Provides { provides, .. } => provides.clone(),
                Requested::Object { object, .. } => format!("object:{object}"),
                other => format!("{other:?}"),
            };
            let resolved = match &requirement.resolved {
                Resolved::Content { object, .. } | Resolved::Artifact { object, .. } => {
                    object.clone()
                }
                other => format!("{other:?}"),
            };
            (requested, resolved, requirement.brought_by.clone())
        })
        .collect()
}

#[test]
fn an_object_that_doesnt_build_brings_its_dependencies_as_the_builds_own() -> TestResult {
    let mut world = World::new();
    let (a, _) = world.object(&content_provider("lib:a", Some("1.0.0")));
    let (b, _) = world.object(&content_provider("lib:b", Some("1.0.0")));
    let (env, _) = world.object(&environment_object(
        "build-base",
        "data:build-base",
        &format!(
            "{}{}",
            top_dependency("lib:a", None),
            top_dependency("lib:b", None)
        ),
    ));
    let (app, _) = world.object(&app_requiring("data:build-base", None));

    let resolution = world.resolve(&app.object_id, "release")?;
    let by = Some(format!("object:{}", env.object_id));
    assert_eq!(
        requirement_summary(&resolution),
        [
            (
                "data:build-base".to_owned(),
                env.object_id.to_string(),
                None
            ),
            ("lib:a".to_owned(), a.object_id.to_string(), by.clone()),
            ("lib:b".to_owned(), b.object_id.to_string(), by),
        ]
    );
    // The same plan every time, and it reads back.
    let again = world.resolve(&app.object_id, "release")?;
    assert_eq!(again.plan.bytes, resolution.plan.bytes);
    assert_eq!(
        PlanDocument::from_bytes(resolution.plan.bytes.clone())?,
        resolution.plan
    );
    // A plan with nothing brought has no brought_by.
    let (plain, _) = world.object(&app_requiring("lib:a", None));
    let plain = world.resolve(&plain.object_id, "release")?;
    assert!(!String::from_utf8(plain.plan.bytes.clone())?.contains("brought_by"));
    Ok(())
}

#[test]
fn a_label_the_build_names_itself_replaces_what_is_brought() -> TestResult {
    let mut world = World::new();
    world.object(&content_provider("lib:a", Some("1.0.0")));
    let (newer, _) = world.object(&content_provider("lib:a", Some("2.0.0")));
    let (b, _) = world.object(&content_provider("lib:b", Some("1.0.0")));
    let (env, _) = world.object(&environment_object(
        "build-base",
        "data:build-base",
        &format!(
            "{}{}",
            top_dependency("lib:a", Some("^1")),
            top_dependency("lib:b", None)
        ),
    ));
    // The build asks for lib:a 2, after the environment, which asks for 1.
    let (app, _) = world.object(&manifest(
        "app",
        r#"
        [[build.targets]]
        name = "release"
        command = ["make"]
        [[build.targets.dependencies]]
        kind = "provides"
        provides = "data:build-base"
        [[build.targets.dependencies]]
        kind = "provides"
        provides = "lib:a"
        version = "^2"
        "#,
    ));

    let resolution = world.resolve(&app.object_id, "release")?;
    assert_eq!(
        requirement_summary(&resolution),
        [
            (
                "data:build-base".to_owned(),
                env.object_id.to_string(),
                None
            ),
            (
                "lib:b".to_owned(),
                b.object_id.to_string(),
                Some(format!("object:{}", env.object_id))
            ),
            ("lib:a".to_owned(), newer.object_id.to_string(), None),
        ]
    );
    Ok(())
}

#[test]
fn objects_that_dont_build_bring_one_another_by_label_and_by_id() -> TestResult {
    let mut world = World::new();
    let (a, _) = world.object(&content_provider("lib:a", Some("1.0.0")));
    let (b, _) = world.object(&content_provider("lib:b", Some("1.0.0")));
    let (inner, inner_revision) = world.object(&environment_object(
        "inner",
        "data:inner",
        &top_dependency("lib:a", None),
    ));
    let inner_snapshot =
        Snapshot::from_object_revision(&inner.object_id, &inner_revision.statement)?;
    let (outer, _) = world.object(&environment_object(
        "outer",
        "data:outer",
        &format!(
            "{}{}",
            top_dependency("data:inner", None),
            top_dependency("lib:b", None)
        ),
    ));
    // By label: the outer brings the inner, which brings lib:a.
    let (app, _) = world.object(&app_requiring("data:outer", None));
    let resolution = world.resolve(&app.object_id, "release")?;
    let by_outer = Some(format!("object:{}", outer.object_id));
    let by_inner = Some(format!("object:{}", inner.object_id));
    assert_eq!(
        requirement_summary(&resolution),
        [
            ("data:outer".to_owned(), outer.object_id.to_string(), None),
            (
                "data:inner".to_owned(),
                inner.object_id.to_string(),
                by_outer.clone()
            ),
            ("lib:a".to_owned(), a.object_id.to_string(), by_inner),
            ("lib:b".to_owned(), b.object_id.to_string(), by_outer),
        ]
    );

    // By id: an object dependency on the inner brings lib:a too.
    let (by_id, _) = world.object(&manifest(
        "app",
        &format!(
            r#"
            [[build.targets]]
            name = "release"
            command = ["make"]
            [[build.targets.dependencies]]
            kind = "object"
            object = "{}"
            snapshot = "{}"
            "#,
            inner.object_id,
            inner_snapshot.snapshot_id()
        ),
    ));
    let resolution = world.resolve(&by_id.object_id, "release")?;
    assert_eq!(
        requirement_summary(&resolution)[1..],
        [(
            "lib:a".to_owned(),
            a.object_id.to_string(),
            Some(format!("object:{}", inner.object_id))
        )]
    );
    Ok(())
}

#[test]
fn an_object_that_doesnt_build_cant_bring_an_environment() -> TestResult {
    let mut world = World::new();
    world.object(&environment_object(
        "build-base",
        "data:build-base",
        &top_dependency("environment:linux/x86-64", None),
    ));
    let (app, _) = world.object(&app_requiring("data:build-base", None));
    let error = world
        .resolve(&app.object_id, "release")
        .expect_err("refused");
    assert!(
        matches!(&error, ResolveError::BroughtUnsupported { dependency, .. }
            if dependency == "environment:linux/x86-64"),
        "{error}"
    );
    Ok(())
}

#[test]
fn what_is_brought_meets_obligations_and_confers_as_the_builds_own() -> TestResult {
    let mut world = World::new();
    world.object(&manifest(
        "libc",
        r#"
        [[build.targets]]
        name = "release"
        command = ["make"]
        [[build.targets.children]]
        name = "headers"
        root = "headers"
        version = "1.2.5"
        provides = [{ provides = "lib:c:headers" }]
        [[build.targets.children]]
        name = "shared"
        root = "shared"
        version = "1.2.5"
        provides = [{ provides = "lib:c:shared", confers = [{ provides = "lib:c:shared", version = "^1.2.5" }] }]
        "#,
    ));
    world.object(&manifest(
        "cc",
        r#"
        [[build.targets]]
        name = "release"
        command = ["make"]
        [[build.targets.children]]
        name = "cc"
        root = "cc"
        version = "4.7.4"
        requires = [{ provides = "lib:c:headers" }, { one_of = [{ provides = "lib:c:static" }, { provides = "lib:c:shared" }] }]
        provides = [{ provides = "tool:cc" }]
        "#,
    ));
    world.object(&environment_object(
        "build-base",
        "data:build-base",
        &format!(
            "{}{}",
            top_dependency("tool:cc", None),
            top_dependency("lib:c:shared", None)
        ),
    ));
    let (app, _) = world.object(&app_requiring("data:build-base", None));

    // The compiler's obligation is met by the shared libc the
    // environment brought, which the artifact then requires, as if the
    // build had named it.
    let resolution = world.resolve(&app.object_id, "release")?;
    let requires: Vec<(String, Option<String>)> = resolution
        .plan
        .manifest
        .requires
        .iter()
        .map(|entry| (entry.provides.clone(), entry.version.clone()))
        .collect();
    assert_eq!(
        requires,
        [("lib:c:shared".to_owned(), Some("^1.2.5".to_owned()))]
    );
    Ok(())
}

/// Builds known only by their build key (§52), at no snapshot.
struct ByKey(Vec<(String, String, BlobId)>);

impl ArtifactSource for ByKey {
    fn artifact(
        &self,
        _object: &ObjectId,
        _snapshot: &SnapshotId,
        _target: &str,
    ) -> Result<Option<BlobId>, SourceError> {
        Ok(None)
    }

    fn artifact_by_key(
        &self,
        object: &ObjectId,
        _target: &str,
        build_key: &BlobId,
    ) -> Result<Option<BlobId>, SourceError> {
        Ok(self
            .0
            .iter()
            .find(|(o, k, _)| o == &object.to_string() && k == build_key.as_str())
            .map(|(_, _, id)| id.clone()))
    }
}

#[test]
fn a_build_key_is_what_the_build_consumes_and_finds_a_held_build() -> TestResult {
    let mut world = World::new();
    let first = artifact_provider("tool:make", "");
    let (make, revision) = world.object(&first);
    let key = |world: &World| -> Result<String, ResolveError> {
        Ok(world
            .resolve(&make.object_id, "release")?
            .plan
            .manifest
            .build_key)
    };
    let held = key(&world)?;
    let actor = world.actor.clone();
    // Each head signed later than the one before, so it is current.
    let mut signed_at = 1_800_000_000;
    let mut revise = |world: &mut World, text: &str, parent: &CreatedRevision| {
        let next = world.revision(
            &make,
            text,
            vec![parent.statement.unsigned().body().revision().clone()],
        );
        signed_at += 1;
        world
            .fixture
            .set_branch_at(&actor, &make, &next, "head", signed_at);
        next
    };

    // A new revision that changes only metadata keeps the key.
    let tagged = first.replace(
        "name = \"tool\"",
        "name = \"tool\"\nsummary = \"make, again\"\ntags = [\"build\"]",
    );
    let before = world
        .resolve(&make.object_id, "release")?
        .plan
        .manifest
        .snapshot;
    let second = revise(&mut world, &tagged, &revision);
    let after = world
        .resolve(&make.object_id, "release")?
        .plan
        .manifest
        .snapshot;
    assert_ne!(before, after, "the new revision is current");
    assert_eq!(key(&world)?, held);
    let plan = world.resolve(&make.object_id, "release")?.plan;
    assert_eq!(plan.manifest.source_date_epoch, 315_532_800);

    // A consumer finds the build made at the first revision by its key.
    let artifact = BlobId::from_bytes(b"test", b"make");
    let built = ByKey(vec![(
        make.object_id.to_string(),
        held.clone(),
        artifact.clone(),
    )]);
    let (app, _) = world.object(&app_requiring("tool:make", None));
    let resolution = world.resolve_with(&built, &World::host(), &[], &app.object_id, "release")?;
    assert!(resolution.plan.manifest.builds_first.is_empty());
    assert!(matches!(
        &resolution.plan.manifest.requirements[0].resolved,
        Resolved::Artifact { artifact: Some(id), build_key: Some(key), .. }
            if id == &artifact.to_string() && key == &held
    ));

    // What the guest runs, or the epoch it gets, is a new key.
    let changed = tagged.replace("command = [\"make\"]", "command = [\"make\", \"-j2\"]");
    assert_ne!(changed, tagged);
    let third = revise(&mut world, &changed, &second);
    assert_ne!(key(&world)?, held);
    revise(
        &mut world,
        &tagged.replace(
            "command = [\"make\"]",
            "command = [\"make\"]\nsource_date_epoch = 0",
        ),
        &third,
    );
    assert_ne!(key(&world)?, held);
    assert_eq!(
        world
            .resolve(&make.object_id, "release")?
            .plan
            .manifest
            .source_date_epoch,
        0
    );
    Ok(())
}

#[test]
fn a_build_of_the_snapshot_under_another_key_isnt_this_plans_artifact() -> TestResult {
    let mut world = World::new();
    let (make, _) = world.object(&artifact_provider("tool:make", ""));
    let plan = world.resolve(&make.object_id, "release")?.plan;
    let key = plan.manifest.build_key.clone();
    let (app, _) = world.object(&app_requiring("tool:make", None));
    let artifact = BlobId::from_bytes(b"test", b"make");
    let held_with = |bytes: Vec<u8>| HeldWithPlans {
        artifacts: vec![(
            make.object_id.to_string(),
            "release".to_owned(),
            artifact.clone(),
        )],
        plans: vec![(make.object_id.to_string(), "release".to_owned(), bytes)],
    };

    // Built at this snapshot under this plan's key: it is the artifact.
    let same = held_with(plan.bytes.clone());
    let resolution = world.resolve_with(&same, &World::host(), &[], &app.object_id, "release")?;
    assert!(resolution.plan.manifest.builds_first.is_empty());

    // Built at this snapshot under another key, as by older rules
    // (§54 sub-decision 8): it is built again.
    let other = BlobId::from_bytes(b"test", b"older rules").to_string();
    let older = String::from_utf8(plan.bytes.clone())?.replace(&key, &other);
    assert_ne!(older.as_bytes(), plan.bytes.as_slice());
    let stale = held_with(older.into_bytes());
    let resolution = world.resolve_with(&stale, &World::host(), &[], &app.object_id, "release")?;
    assert_eq!(resolution.plan.manifest.builds_first.len(), 1);
    Ok(())
}

#[test]
fn an_object_built_in_one_environment_can_run_in_another() -> TestResult {
    let mut world = World::new();
    let host = Host {
        system: "linux".to_owned(),
        arch: "x86-64".to_owned(),
        engines: vec![
            "environment:linux/x86".to_owned(),
            "environment:linux/x86-64".to_owned(),
        ],
    };
    // A 64-bit program built by a 32-bit environment (§53): the object
    // says what it runs in, its target where it is built.
    let (cross_built, _) = world.object(&manifest(
        "cross-built",
        &format!(
            r#"
            [[dependencies]]
            kind = "provides"
            provides = "environment:linux/x86-64"
            [[provides]]
            provides = "tool:x"
            target = "release"
            output = "bin"
            {RELEASE_TARGET}
            [[build.targets.dependencies]]
            kind = "provides"
            provides = "environment:linux/x86"
            "#
        ),
    ));
    let plan = world
        .resolve_with(&NoArtifacts, &host, &[], &cross_built.object_id, "release")?
        .plan;
    assert_eq!(plan.manifest.environment, "environment:linux/x86");
    let environments: Vec<&Resolved> = plan
        .manifest
        .requirements
        .iter()
        .map(|requirement| &requirement.resolved)
        .filter(|resolved| matches!(resolved, Resolved::Environment { .. }))
        .collect();
    assert!(
        matches!(environments.as_slice(), [Resolved::Environment { contract, .. }] if contract == "environment:linux/x86"),
        "{environments:?}"
    );

    // It is a 64-bit program: a build in the 64-bit environment can run
    // it, and one in the 32-bit environment can't.
    let consumer = |world: &mut World, environment: &str| {
        world
            .object(&manifest(
                "consumer",
                &format!(
                    r#"
                    [[build.targets]]
                    name = "release"
                    command = ["x"]
                    [[build.targets.dependencies]]
                    kind = "provides"
                    provides = "{environment}"
                    [[build.targets.dependencies]]
                    kind = "provides"
                    provides = "tool:x"
                    "#
                ),
            ))
            .0
            .object_id
    };
    let wide = consumer(&mut world, "environment:linux/x86-64");
    let resolution = world.resolve_with(&NoArtifacts, &host, &[], &wide, "release")?;
    assert!(resolution
        .plan
        .manifest
        .requirements
        .iter()
        .any(|requirement| matches!(
            &requirement.resolved,
            Resolved::Artifact { object, .. } if object == &cross_built.object_id.to_string()
        )));
    let narrow = consumer(&mut world, "environment:linux/x86");
    let error = world
        .resolve_with(&NoArtifacts, &host, &[], &narrow, "release")
        .expect_err("a 64-bit program in a 32-bit build");
    assert!(
        error
            .to_string()
            .contains("runs in environment:linux/x86-64, not x86"),
        "{error}"
    );
    Ok(())
}
