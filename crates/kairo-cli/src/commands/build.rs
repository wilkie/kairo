//! `kairo build --plan-only` and `kairo provider ...` runners
//! (`specs/DECISIONS.md` §33 sub-decisions 2–3, `specs/CLI.md` §17).
//!
//! Planning reads each candidate's `kairo.toml` from the managed Git
//! cache at its revision's commit, so an object whose tree isn't held
//! is reported as skipped rather than considered. Build results don't
//! exist yet, so every artifact provider is a build the plan needs
//! first. Running the plan is the next slice.

use std::fmt::Write as _;

use std::path::Path;
use std::time::Duration;

use kairo_build::{run_plan, Execution, Limits, Status, TreeSource};
use kairo_core::canonical::CanonicalEncode;
use kairo_core::{BlobId, KairoRef, ObjectId, SnapshotId, StatementId, Timestamp};
use kairo_keystore::Keystore;
use kairo_object::ObjectManifest;
use kairo_resolver::{
    describe_resolved, find_snapshot, version_matches, ArtifactSource, Host, HostTool, Index,
    ManifestSource, PlanDocument, Resolution, Resolver, SnapshotSelector, SourceError,
};
use kairo_statement::{
    BuildOutcome, BuildResultBody, RevisionId, Signature, SignedStatement, TrustDecision,
    TrustScope, UnsignedStatement,
};
use kairo_store::{BlobStore, BuildResultResolver, FilesystemStore, StatementStore, TrustResolver};

use crate::cli::ProviderCommand;
use crate::commands::config::load_config;
use crate::error::CliError;
use crate::store_paths::{open_keystore, open_store, StorePaths};

pub(crate) struct BuildArgs<'a> {
    pub snapshot_ref: &'a str,
    pub target: &'a str,
    /// Signs the `BuildResult`; required unless `plan_only`.
    pub actor: Option<&'a str>,
    pub plan_only: bool,
    pub json: bool,
}

/// Held artifacts, from `BuildResult`s this node accepts: signed by a
/// keystore actor or one a keystore actor trusts at `general` scope,
/// successful, and with the artifact blob present. Newest first.
struct StoreArtifacts<'a> {
    store: &'a FilesystemStore,
    accepted: Vec<kairo_core::ActorId>,
}

impl StoreArtifacts<'_> {
    fn accepts(&self, signer: &kairo_core::ActorId) -> bool {
        if self.accepted.contains(signer) {
            return true;
        }
        self.accepted.iter().any(|by_actor| {
            self.store
                .latest_trust(by_actor, signer, &TrustScope::general())
                .ok()
                .flatten()
                .is_some_and(|trust| {
                    trust.unsigned().body().decision() == Some(TrustDecision::Trusted)
                })
        })
    }
}

impl StoreArtifacts<'_> {
    /// The newest accepted, successful build of `target` at `snapshot`
    /// whose artifact is held.
    fn head(
        &self,
        object: &ObjectId,
        snapshot: &SnapshotId,
        target: &str,
    ) -> Result<Option<kairo_store::BuildResultHead>, SourceError> {
        let heads = self
            .store
            .list_build_results(object)
            .map_err(|error| SourceError(error.to_string()))?;
        for head in heads {
            if &head.snapshot != snapshot || head.target != target {
                continue;
            }
            let Some(artifact) = &head.artifact else {
                continue;
            };
            if !self.accepts(&head.actor) {
                continue;
            }
            match self.store.get_blob(artifact) {
                Ok(_) => return Ok(Some(head)),
                Err(kairo_store::StoreError::Missing) => continue,
                Err(error) => return Err(SourceError(error.to_string())),
            }
        }
        Ok(None)
    }
}

impl ArtifactSource for StoreArtifacts<'_> {
    fn build_plan(
        &self,
        object: &ObjectId,
        snapshot: &SnapshotId,
        target: &str,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        let Some(head) = self.head(object, snapshot, target)? else {
            return Ok(None);
        };
        match self.store.get_blob(&head.resolved_manifest) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(kairo_store::StoreError::Missing) => Ok(None),
            Err(error) => Err(SourceError(error.to_string())),
        }
    }

    fn artifact(
        &self,
        object: &ObjectId,
        snapshot: &SnapshotId,
        target: &str,
    ) -> Result<Option<BlobId>, SourceError> {
        let heads = self
            .store
            .list_build_results(object)
            .map_err(|error| SourceError(error.to_string()))?;
        for head in heads {
            if &head.snapshot != snapshot || head.target != target {
                continue;
            }
            let Some(artifact) = head.artifact else {
                continue;
            };
            if !self.accepts(&head.actor) {
                continue;
            }
            match self.store.get_blob(&artifact) {
                Ok(_) => return Ok(Some(artifact)),
                Err(kairo_store::StoreError::Missing) => continue,
                Err(error) => return Err(SourceError(error.to_string())),
            }
        }
        Ok(None)
    }
}

/// Manifests from the managed Git cache, at the revision's commit.
pub(crate) struct CacheManifests<'a> {
    pub(crate) paths: &'a StorePaths,
}

impl ManifestSource for CacheManifests<'_> {
    fn manifest(
        &self,
        object: &ObjectId,
        revision: &RevisionId,
    ) -> Result<Option<ObjectManifest>, SourceError> {
        let Some(repo) = crate::try_cache_repo_for_revision(self.paths, object.as_str(), revision)
            .map_err(|error| SourceError(error.to_string()))?
        else {
            return Ok(None);
        };
        let Some(commit) = revision
            .git()
            .map_err(|error| SourceError(error.to_string()))?
        else {
            return Ok(None);
        };
        let Some(bytes) = repo
            .read_blob_at_path(&commit, "kairo.toml")
            .map_err(|error| SourceError(error.to_string()))?
        else {
            return Ok(None);
        };
        let text =
            String::from_utf8(bytes).map_err(|_| SourceError("kairo.toml is not UTF-8".into()))?;
        ObjectManifest::parse_toml(&text)
            .map(Some)
            .map_err(|error| SourceError(error.to_string()))
    }
}

/// Trees from the managed Git cache, for the executor.
struct CacheTrees<'a> {
    paths: &'a StorePaths,
    store: &'a FilesystemStore,
}

impl TreeSource for CacheTrees<'_> {
    fn export(
        &self,
        object: &ObjectId,
        snapshot: &SnapshotId,
        dest: &Path,
    ) -> Result<bool, SourceError> {
        let Some(snapshot) = find_snapshot(self.store, object, snapshot)
            .map_err(|error| SourceError(error.to_string()))?
        else {
            return Ok(false);
        };
        let revision = snapshot.revision();
        let Some(repo) = crate::try_cache_repo_for_revision(self.paths, object.as_str(), revision)
            .map_err(|error| SourceError(error.to_string()))?
        else {
            return Ok(false);
        };
        let Some(commit) = revision
            .git()
            .map_err(|error| SourceError(error.to_string()))?
        else {
            return Ok(false);
        };
        repo.export_tree(&commit, dest)
            .map_err(|error| SourceError(error.to_string()))
    }

    fn export_source(
        &self,
        source: &ObjectId,
        revision: &kairo_statement::GitRevision,
        dest: &Path,
    ) -> Result<bool, SourceError> {
        let path = kairo_git::object_repo_path(&self.paths.git_root(), source.as_str())
            .map_err(|error| SourceError(error.to_string()))?;
        if !path.exists() {
            return Ok(false);
        }
        kairo_git::open(&path)
            .and_then(|repo| repo.export_tree(revision, dest))
            .map_err(|error| SourceError(error.to_string()))
    }
}

/// Source objects' commits, from the managed Git cache (§37).
struct CacheSources<'a> {
    paths: &'a StorePaths,
}

impl kairo_resolver::SourceHolder for CacheSources<'_> {
    fn holds(
        &self,
        source: &ObjectId,
        revision: &kairo_statement::GitRevision,
    ) -> Result<bool, SourceError> {
        let path = kairo_git::object_repo_path(&self.paths.git_root(), source.as_str())
            .map_err(|error| SourceError(error.to_string()))?;
        if !path.exists() {
            return Ok(false);
        }
        let repo = kairo_git::open(&path).map_err(|error| SourceError(error.to_string()))?;
        if repo
            .object_format()
            .map_err(|error| SourceError(error.to_string()))?
            != revision.format()
        {
            return Ok(false);
        }
        repo.find_commit(revision)
            .map(|commit| commit.is_some())
            .map_err(|error| SourceError(error.to_string()))
    }
}

fn source_seeds(config: &kairo_config::Config) -> Vec<kairo_resolver::SourceSeed> {
    config
        .runtime
        .seeds
        .iter()
        .map(|seed| kairo_resolver::SourceSeed {
            source: kairo_statement::SourceGenesis::new(seed.source.clone()).object_id(),
            revision: seed.revision.clone(),
        })
        .collect()
}

fn host_tools(config: &kairo_config::Config) -> Vec<HostTool> {
    config
        .runtime
        .host_tools
        .iter()
        .map(|tool| HostTool {
            provides: tool.provides.clone(),
            version: tool.version.clone(),
            path: tool.path.clone(),
        })
        .collect()
}

/// Development-mode policy (`DECISIONS.md` §33 sub-decision 4): the
/// revision's signer is an actor this node holds a key for, or one a
/// keystore actor trusts at `general` scope.
fn check_signer(
    store: &FilesystemStore,
    local: &[kairo_core::ActorId],
    object: &ObjectId,
    signer: &kairo_core::ActorId,
) -> Result<(), CliError> {
    if local.contains(signer) {
        return Ok(());
    }
    for by_actor in local {
        let trusted = store
            .latest_trust(by_actor, signer, &TrustScope::general())
            .map_err(CliError::ReadBranch)?
            .is_some_and(|trust| {
                trust.unsigned().body().decision() == Some(TrustDecision::Trusted)
            });
        if trusted {
            return Ok(());
        }
    }
    Err(CliError::BuildSignerUntrusted {
        signer: signer.clone(),
        object: object.clone(),
    })
}

/// How this node may run plans: the probed engines, and whether
/// development mode is allowed (§33 sub-decision 4, §35).
struct RunPolicy {
    engines: Vec<kairo_build::Bubblewrap>,
    engine_failures: Vec<String>,
    allow_local_process: bool,
    config_path: std::path::PathBuf,
}

impl RunPolicy {
    fn load(config: &kairo_config::Config, config_path: &std::path::Path) -> Self {
        let mut engines = Vec::new();
        let mut engine_failures = Vec::new();
        for engine in &config.runtime.engines {
            match engine.kind {
                kairo_config::EngineKind::Bubblewrap => {
                    match kairo_build::Bubblewrap::probe(&engine.path) {
                        Ok(probed) => engines.push(probed),
                        Err(reason) => engine_failures.push(reason),
                    }
                }
            }
        }
        Self {
            engines,
            engine_failures,
            allow_local_process: config.runtime.allow_local_process,
            config_path: config_path.to_path_buf(),
        }
    }

    /// The host, with the contracts its working engines implement.
    fn host(&self) -> Host {
        let mut host = Host::detect();
        // Bubblewrap implements the Linux contract (§35, §41), and on an
        // x86-64 kernel that runs i386, for x86 too (§38 sub-decision 7).
        if !self.engines.is_empty() {
            host.engines.push(host.contract());
        }
        if host.arch == "x86-64" && self.engines.iter().any(|engine| engine.runs_x86) {
            let x86 = Host {
                arch: "x86".to_owned(),
                ..host.clone()
            };
            host.engines.push(x86.contract());
        }
        host
    }

    /// The engine for `plan`, or the refusal that applies.
    fn engine_for(&self, plan: &PlanDocument) -> Result<kairo_build::Engine, CliError> {
        if plan.manifest.environment == "none" {
            if !self.allow_local_process {
                return Err(CliError::BuildNotAllowed {
                    config: self.config_path.clone(),
                });
            }
            return Ok(kairo_build::Engine::Native);
        }
        self.engines
            .first()
            .cloned()
            .map(kairo_build::Engine::Bubblewrap)
            .ok_or_else(|| CliError::NoEngine {
                environment: plan.manifest.environment.clone(),
                failures: self.engine_failures.clone(),
            })
    }
}

/// What one `kairo build` did: the builds it ran, prerequisites first.
struct Built {
    execution: Execution,
    result: StatementId,
}

pub(crate) fn run_build(args: &BuildArgs<'_>, paths: &StorePaths) -> Result<String, CliError> {
    let (object, selector) = match args.snapshot_ref.parse::<KairoRef>() {
        Ok(KairoRef::Object(object)) => (object, SnapshotSelector::Current),
        Ok(KairoRef::ObjectSnapshot { object, snapshot }) => {
            (object, SnapshotSelector::Pinned(snapshot))
        }
        Ok(_) => {
            return Err(CliError::NotASnapshotRef {
                input: args.snapshot_ref.to_owned(),
            })
        }
        Err(source) => {
            return Err(CliError::ParseSnapshotRef {
                input: args.snapshot_ref.to_owned(),
                source,
            })
        }
    };
    let store = open_store(paths)?;
    let (config, source) = load_config(paths)?;
    let policy = RunPolicy::load(&config, source.path());
    // Nothing could run here at all: say so before resolving.
    if !args.plan_only && policy.engines.is_empty() && !policy.allow_local_process {
        return Err(CliError::BuildNotAllowed {
            config: source.path().to_path_buf(),
        });
    }
    let keystore = open_keystore(paths)?;
    let local = keystore.list_actors().map_err(CliError::ListKeystore)?;
    let manifests = CacheManifests { paths };
    let host = policy.host();
    let tools = host_tools(&config);
    let artifacts = StoreArtifacts {
        store: &store,
        accepted: local.clone(),
    };
    let sources = CacheSources { paths };
    let accepted_sources = source_seeds(&config);
    let resolve = || {
        Resolver::new(&store, &manifests, &artifacts, &host, &tools)
            .with_sources(&sources)
            .with_source_seeds(&accepted_sources)
            .resolve(&object, &selector, args.target)
            .map_err(|error| CliError::Resolve(Box::new(error)))
    };
    let store_plans = |resolution: &Resolution| -> Result<(), CliError> {
        // The plan and what it needs first are stored, so results
        // and `kairo reproduce` can cite them by hash.
        for plan in resolution
            .prerequisites
            .iter()
            .chain(std::iter::once(&resolution.plan))
        {
            store
                .put_blob(&plan.hash, &plan.bytes)
                .map_err(|source| CliError::WriteBlob {
                    blob: plan.hash.clone(),
                    source,
                })?;
        }
        Ok(())
    };

    let resolution = resolve()?;
    store_plans(&resolution)?;
    if args.plan_only {
        return Ok(if args.json {
            format_resolution_json(&resolution)
        } else {
            format_resolution(&resolution)
        });
    }

    let actor = args.actor.ok_or(CliError::BuildActorRequired)?;
    let actor_id =
        kairo_core::ActorId::new(actor.to_owned()).map_err(|source| CliError::ParseActorId {
            actor: actor.to_owned(),
            source,
        })?;
    let secret = crate::require_active_signing_key(&store, &keystore, &actor_id)?;
    let trees = CacheTrees {
        paths,
        store: &store,
    };
    let limits = Limits {
        timeout: Duration::from_secs(config.runtime.timeout_secs),
    };

    // Each round builds the first build the plan still needs, whose
    // own plan needs nothing by construction (dependencies come
    // first), and re-resolves so its result pins the next one. The
    // last round builds the target itself.
    let mut built: Vec<Built> = Vec::new();
    let mut resolution = resolution;
    let outcome = loop {
        let (plan, is_root) = match resolution.plan.manifest.builds_first.first() {
            None => (resolution.plan.clone(), true),
            Some(first) => {
                let hash = BlobId::new(first.plan.clone()).map_err(|error| {
                    CliError::Build(Box::new(kairo_build::BuildError::Id(error)))
                })?;
                let plan = resolution
                    .prerequisites
                    .iter()
                    .find(|plan| plan.hash == hash)
                    .cloned()
                    .ok_or_else(|| CliError::BuildPrerequisites {
                        builds: vec![format!("object:{} target {}", first.object, first.target)],
                    })?;
                (plan, false)
            }
        };
        let one = build_one(
            &store, &trees, &policy, &local, &actor_id, &secret, &plan, &limits,
        )?;
        let status = one.execution.record.status;
        built.push(one);
        if status != Status::Success || is_root {
            break status;
        }
        resolution = resolve()?;
        store_plans(&resolution)?;
    };

    let summary = if args.json {
        format_built_json(&store, &built)
    } else {
        format_built(&store, &built)
    };
    match outcome {
        Status::Success => Ok(summary),
        status => Err(CliError::BuildFailed {
            status: status.as_str(),
            summary,
        }),
    }
}

/// Run one plan under the signer policy and sign its `BuildResult`
/// (success or failure; a timeout signs nothing, §33 sub-decision 8).
#[allow(clippy::too_many_arguments)]
fn build_one(
    store: &FilesystemStore,
    trees: &CacheTrees<'_>,
    policy: &RunPolicy,
    local: &[kairo_core::ActorId],
    actor_id: &kairo_core::ActorId,
    secret: &kairo_identity::SecretSigningKey,
    plan: &PlanDocument,
    limits: &Limits,
) -> Result<Built, CliError> {
    let object = ObjectId::new(plan.manifest.object.clone())
        .map_err(|error| CliError::Build(Box::new(kairo_build::BuildError::Id(error))))?;
    let snapshot_id = SnapshotId::new(plan.manifest.snapshot.clone())
        .map_err(|error| CliError::Build(Box::new(kairo_build::BuildError::Id(error))))?;
    let snapshot = find_snapshot(store, &object, &snapshot_id)
        .map_err(|error| CliError::Resolve(Box::new(error)))?
        .ok_or_else(|| {
            CliError::Build(Box::new(kairo_build::BuildError::TreeNotHeld {
                object: object.clone(),
                snapshot: snapshot_id.clone(),
            }))
        })?;
    let revision = store
        .get_object_revision(&snapshot.frontier()[0])
        .map_err(|source| CliError::ReadRevision {
            statement: snapshot.frontier()[0].clone(),
            source,
        })?;
    // Development mode runs the guest on the host, so it needs a signer
    // this node vouches for; an engine is the boundary instead (§35
    // sub-decision 5).
    let engine = policy.engine_for(plan)?;
    if engine == kairo_build::Engine::Native {
        check_signer(store, local, &object, revision.unsigned().actor())?;
    }

    let execution = run_plan(
        store,
        trees,
        plan,
        revision.unsigned().created_at().seconds(),
        limits,
        &engine,
    )
    .map_err(|error| CliError::Build(Box::new(error)))?;

    let record = &execution.record;
    let outcome = match record.status {
        Status::Success => BuildOutcome::Success,
        Status::Failure => BuildOutcome::Failure,
        // A timeout signs nothing; it is reported from the record.
        Status::Timeout => {
            return Err(CliError::BuildFailed {
                status: "timeout",
                summary: format_execution(store, &execution),
            })
        }
    };
    let artifact = record
        .artifact
        .as_ref()
        .map(|id| BlobId::new(id.clone()))
        .transpose()
        .map_err(|error| CliError::Build(Box::new(kairo_build::BuildError::Id(error))))?;
    let body = BuildResultBody::new(
        object.clone(),
        snapshot_id,
        plan.manifest.target.clone(),
        plan.hash.clone(),
        outcome,
        artifact,
        format!("{}/{}", record.executor.id, record.executor.version),
        plan.manifest.environment.clone(),
        record.host_dependent,
        execution.id.clone(),
    )
    .map_err(CliError::BuildResultShape)?;
    let subject: KairoRef =
        format!("object:{object}")
            .parse()
            .map_err(|source| CliError::BuildSubjectRef {
                object: object.clone(),
                source,
            })?;
    let unsigned = store
        .link_to_chain(UnsignedStatement::new(
            actor_id.clone(),
            subject,
            Timestamp::now(),
            body,
        ))
        .map_err(|source| CliError::ReadActorChain {
            actor: actor_id.clone(),
            source,
        })?;
    let signature_bytes = secret.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        actor_id.clone(),
        secret.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    let signed = SignedStatement::new(unsigned, signature);
    let result = store
        .put_build_result(&signed)
        .map_err(|source| CliError::WriteBuildResult { source })?;
    Ok(Built { execution, result })
}

/// `kairo reproduce` (`DECISIONS.md` §33 sub-decision 9).
pub(crate) fn run_reproduce(
    result_ref: &str,
    actor: &str,
    json: bool,
    paths: &StorePaths,
) -> Result<String, CliError> {
    let cited_id = match result_ref.parse::<KairoRef>() {
        Ok(KairoRef::Statement(id)) => id,
        _ => {
            return Err(CliError::NotAResultRef {
                input: result_ref.to_owned(),
            })
        }
    };
    let store = open_store(paths)?;
    let (config, source) = load_config(paths)?;
    let policy = RunPolicy::load(&config, source.path());
    let cited = store
        .get_build_result(&cited_id)
        .map_err(|source| CliError::ReadBuildResult {
            statement: cited_id.clone(),
            source,
        })?;
    let body = cited.unsigned().body();
    let plan_bytes =
        store
            .get_blob(body.resolved_manifest())
            .map_err(|_| CliError::ReproduceUnavailable {
                missing: vec![format!(
                    "the plan {} (a bundle exported with --include-executions carries it)",
                    body.resolved_manifest()
                )],
            })?;
    let plan =
        PlanDocument::from_bytes(plan_bytes).map_err(|error| CliError::Resolve(Box::new(error)))?;
    if &plan.hash != body.resolved_manifest() {
        return Err(CliError::ReproduceUnavailable {
            missing: vec![format!("a plan matching {}", body.resolved_manifest())],
        });
    }
    // Exactly what the plan pins must be here: no re-resolution.
    let mut missing: Vec<String> = plan
        .manifest
        .builds_first
        .iter()
        .map(|build| format!("a build of object:{} target {}", build.object, build.target))
        .collect();
    let resolved = plan
        .manifest
        .requirements
        .iter()
        .map(|requirement| &requirement.resolved)
        .chain(plan.manifest.runtime.iter().map(|entry| &entry.resolved));
    for resolved in resolved {
        match resolved {
            kairo_resolver::Resolved::Artifact {
                object,
                target,
                artifact,
                ..
            } => {
                let held = artifact
                    .as_ref()
                    .and_then(|id| BlobId::new(id.clone()).ok())
                    .is_some_and(|id| store.get_blob(&id).is_ok());
                if !held {
                    missing.push(format!(
                        "the artifact of object:{object} target {target}{}",
                        artifact
                            .as_ref()
                            .map(|id| format!(" ({id})"))
                            .unwrap_or_default()
                    ));
                }
            }
            kairo_resolver::Resolved::Source {
                source,
                url,
                revision,
                ..
            } => {
                let held = ObjectId::new(source.clone())
                    .ok()
                    .zip(kairo_statement::GitRevision::parse(revision).ok().flatten())
                    .is_some_and(|(source, revision)| {
                        kairo_resolver::SourceHolder::holds(
                            &CacheSources { paths },
                            &source,
                            &revision,
                        )
                        .unwrap_or(false)
                    });
                if !held {
                    missing.push(format!("{url} at {revision} (kairo source fetch {url})"));
                }
            }
            kairo_resolver::Resolved::File { source, sha256, .. } => {
                let held = ObjectId::new(source.clone()).ok().is_some_and(|source| {
                    kairo_store::SourceStore::holds_file_source(&store, &source).unwrap_or(false)
                });
                if !held {
                    missing.push(format!(
                        "the file sha256:{sha256} (kairo source fetch-file <url> --sha256 {sha256})"
                    ));
                }
            }
            kairo_resolver::Resolved::HostTool { path, .. } => {
                if !std::path::Path::new(path).is_file() {
                    missing.push(format!("the host tool {path}"));
                }
            }
            kairo_resolver::Resolved::Content { .. }
            | kairo_resolver::Resolved::Environment { .. } => {}
        }
    }
    if !missing.is_empty() {
        return Err(CliError::ReproduceUnavailable { missing });
    }

    let keystore = open_keystore(paths)?;
    let local = keystore.list_actors().map_err(CliError::ListKeystore)?;
    let actor_id =
        kairo_core::ActorId::new(actor.to_owned()).map_err(|source| CliError::ParseActorId {
            actor: actor.to_owned(),
            source,
        })?;
    let secret = crate::require_active_signing_key(&store, &keystore, &actor_id)?;
    let trees = CacheTrees {
        paths,
        store: &store,
    };
    let limits = Limits {
        timeout: Duration::from_secs(config.runtime.timeout_secs),
    };
    let one = build_one(
        &store, &trees, &policy, &local, &actor_id, &secret, &plan, &limits,
    )?;

    let cited_artifact = body.artifact().map(ToString::to_string);
    let new_artifact = one.execution.record.artifact.clone();
    let new_outcome = one.execution.record.status.as_str();
    let matches = body.result().as_str() == new_outcome && cited_artifact == new_artifact;
    let verdict = if matches { "matches" } else { "differs" };
    if json {
        return Ok(format!(
            "{}\n",
            serde_json::to_string_pretty(&serde_json::json!({
                "reproduction": verdict,
                "cited": {
                    "result": cited_id.to_string(),
                    "signer": cited.unsigned().actor().to_string(),
                    "outcome": body.result().as_str(),
                    "artifact": cited_artifact,
                },
                "reproduced": {
                    "result": one.result.to_string(),
                    "outcome": new_outcome,
                    "artifact": new_artifact,
                    "execution": one.execution.id.to_string(),
                    "record": serde_json::to_value(&one.execution.record).unwrap_or_default(),
                },
                "plan": plan.hash.to_string(),
            }))
            .unwrap_or_default()
        ));
    }
    let mut out = String::new();
    let _ = writeln!(out, "reproduction = {verdict}");
    let _ = writeln!(out, "plan = {}", plan.hash);
    let _ = writeln!(out, "cited_result = {cited_id}");
    let _ = writeln!(out, "cited_signer = {}", cited.unsigned().actor());
    let _ = writeln!(out, "cited_outcome = {}", body.result().as_str());
    let _ = writeln!(
        out,
        "cited_artifact = {}",
        cited_artifact.as_deref().unwrap_or("none")
    );
    let _ = writeln!(out, "reproduced_outcome = {new_outcome}");
    let _ = writeln!(
        out,
        "reproduced_artifact = {}",
        new_artifact.as_deref().unwrap_or("none")
    );
    out.push_str(&format_execution(&store, &one.execution));
    let _ = writeln!(out, "result = {}", one.result);
    Ok(out)
}

fn format_built(store: &FilesystemStore, built: &[Built]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "builds = {}", built.len());
    for (index, one) in built.iter().enumerate() {
        if built.len() > 1 {
            let _ = writeln!(
                out,
                "--- build {} of {}{} ---",
                index + 1,
                built.len(),
                if index + 1 == built.len() {
                    " (the target)"
                } else {
                    " (needed first)"
                }
            );
        }
        out.push_str(&format_execution(store, &one.execution));
        let _ = writeln!(out, "result = {}", one.result);
    }
    out
}

fn format_built_json(store: &FilesystemStore, built: &[Built]) -> String {
    let builds: Vec<serde_json::Value> = built
        .iter()
        .map(|one| {
            serde_json::json!({
                "execution": one.execution.id.to_string(),
                "result": one.result.to_string(),
                "record": serde_json::to_value(&one.execution.record).unwrap_or_default(),
                "record_path": kairo_build::record_path(store, &one.execution.id).display().to_string(),
                "artifact_tree": one.execution.artifact.as_ref().and_then(|tree| serde_json::to_value(tree).ok()),
            })
        })
        .collect();
    format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({ "builds": builds })).unwrap_or_default()
    )
}

fn format_resolution(resolution: &Resolution) -> String {
    let plan = &resolution.plan.manifest;
    let mut out = String::new();
    let _ = writeln!(out, "plan = {}", resolution.plan.hash);
    let _ = writeln!(out, "object = {}", plan.object);
    let _ = writeln!(out, "snapshot = {}", plan.snapshot);
    let _ = writeln!(out, "target = {}", plan.target);
    let _ = writeln!(out, "environment = {}", plan.environment);
    let _ = writeln!(
        out,
        "host_dependent = {}",
        if plan.host_dependent { "yes" } else { "no" }
    );
    let _ = writeln!(out, "requirements = {}", plan.requirements.len());
    for requirement in &plan.requirements {
        let _ = writeln!(
            out,
            "  {} -> {}",
            requirement.requested,
            describe_resolved(&requirement.resolved)
        );
    }
    if !plan.runtime.is_empty() {
        let _ = writeln!(out, "runtime = {}", plan.runtime.len());
        for entry in &plan.runtime {
            let _ = writeln!(
                out,
                "  {}{} via {} -> {}",
                entry.provides,
                entry
                    .version
                    .as_deref()
                    .map(|version| format!(" {version}"))
                    .unwrap_or_default(),
                entry.via.join(" > "),
                describe_resolved(&entry.resolved)
            );
        }
    }
    if !plan.requires.is_empty() {
        let _ = writeln!(out, "requires = {}", plan.requires.len());
        for entry in &plan.requires {
            let _ = writeln!(
                out,
                "  {}{}{}{} ({})",
                entry
                    .child
                    .as_deref()
                    .map(|child| format!("{child}: "))
                    .unwrap_or_default(),
                entry.provides,
                entry
                    .version
                    .as_deref()
                    .map(|version| format!(" {version}"))
                    .unwrap_or_default(),
                entry
                    .pin
                    .as_ref()
                    .map(|pin| format!(" pinned to object:{}", pin.object))
                    .unwrap_or_default(),
                entry.via
            );
        }
    }
    let _ = writeln!(out, "builds_first = {}", plan.builds_first.len());
    for first in &plan.builds_first {
        let _ = writeln!(
            out,
            "  object:{} target {} (plan {})",
            first.object, first.target, first.plan
        );
    }
    let diagnostics = &resolution.diagnostics;
    if !diagnostics.alternatives.is_empty() {
        let _ = writeln!(out, "alternatives:");
        for alternative in &diagnostics.alternatives {
            let _ = writeln!(
                out,
                "  {} -> {} (not {})",
                alternative.requested,
                alternative.chosen,
                alternative.others.join(", ")
            );
        }
    }
    if !diagnostics.skipped.is_empty() {
        let _ = writeln!(out, "skipped:");
        for skipped in &diagnostics.skipped {
            let _ = writeln!(out, "  object:{}: {}", skipped.object, skipped.reason);
        }
    }
    out
}

fn format_resolution_json(resolution: &Resolution) -> String {
    let plans: Vec<serde_json::Value> = resolution
        .prerequisites
        .iter()
        .map(|plan| {
            serde_json::json!({
                "plan": plan.hash.to_string(),
                "manifest": serde_json::to_value(&plan.manifest).unwrap_or_default(),
            })
        })
        .collect();
    let value = serde_json::json!({
        "plan": resolution.plan.hash.to_string(),
        "manifest": serde_json::to_value(&resolution.plan.manifest).unwrap_or_default(),
        "prerequisites": plans,
        "alternatives": resolution.diagnostics.alternatives.iter().map(|alternative| {
            serde_json::json!({
                "requested": alternative.requested.to_string(),
                "chosen": alternative.chosen,
                "others": alternative.others,
            })
        }).collect::<Vec<_>>(),
        "skipped": resolution.diagnostics.skipped.iter().map(|skipped| {
            serde_json::json!({ "object": skipped.object.to_string(), "reason": skipped.reason })
        }).collect::<Vec<_>>(),
    });
    format!(
        "{}\n",
        serde_json::to_string_pretty(&value).unwrap_or_default()
    )
}

fn format_execution(store: &FilesystemStore, execution: &Execution) -> String {
    let record = &execution.record;
    let mut out = String::new();
    let _ = writeln!(out, "execution = {}", execution.id);
    let _ = writeln!(out, "status = {}", record.status.as_str());
    let _ = writeln!(out, "plan = {}", record.plan);
    let _ = writeln!(out, "object = {}", record.object);
    let _ = writeln!(out, "snapshot = {}", record.snapshot);
    let _ = writeln!(out, "target = {}", record.target);
    let _ = writeln!(
        out,
        "executor = {}/{} ({})",
        record.executor.id, record.executor.version, record.executor.isolation
    );
    let _ = writeln!(out, "environment = {}", record.environment);
    if let Some(provisioning) = record.provisioning {
        let _ = writeln!(
            out,
            "provisioning = {}",
            match provisioning {
                kairo_build::Provisioning::Overlay => "overlay",
                kairo_build::Provisioning::Copy => "copy",
            }
        );
    }
    let _ = writeln!(
        out,
        "host_dependent = {}",
        if record.host_dependent { "yes" } else { "no" }
    );
    if let Some(code) = record.exit_code {
        let _ = writeln!(out, "exit_code = {code}");
    }
    if let Some(artifact) = &record.artifact {
        let _ = writeln!(out, "artifact = {artifact}");
    }
    let _ = writeln!(out, "outputs = {}", record.outputs.len());
    for output in &record.outputs {
        let _ = writeln!(
            out,
            "  {} -> {} ({} files)",
            output.name, output.artifact_path, output.files
        );
    }
    if !record.children.is_empty() {
        let _ = writeln!(out, "children = {}", record.children.len());
        for child in &record.children {
            let _ = writeln!(
                out,
                "  {} object:{} (artifact {})",
                child.name, child.object, child.artifact
            );
        }
    }
    if let Some(tree) = &execution.artifact {
        let _ = writeln!(out, "files = {}", tree.entries.len());
        for entry in &tree.entries {
            let _ = match entry {
                kairo_build::ArtifactEntry::File(file) => writeln!(
                    out,
                    "  {}{} {}",
                    file.path,
                    if file.executable { "*" } else { "" },
                    file.blob
                ),
                kairo_build::ArtifactEntry::Link(link) => {
                    writeln!(out, "  {} -> {}", link.path, link.link)
                }
            };
        }
    }
    let _ = writeln!(out, "stdout = {}", record.logs.stdout);
    let _ = writeln!(out, "stderr = {}", record.logs.stderr);
    for line in &record.diagnostics {
        let _ = writeln!(out, "diagnostic = {line}");
    }
    let _ = writeln!(
        out,
        "record = {}",
        kairo_build::record_path(store, &execution.id).display()
    );
    out
}

pub(crate) fn run_provider_command(
    command: ProviderCommand,
    paths: &StorePaths,
) -> Result<String, CliError> {
    let store = open_store(paths)?;
    let manifests = CacheManifests { paths };
    let index =
        Index::build(&store, &manifests).map_err(|error| CliError::Resolve(Box::new(error)))?;
    match command {
        ProviderCommand::List { json } => Ok(if json {
            let labels: Vec<serde_json::Value> = index
                .all_offers()
                .into_iter()
                .map(|(label, offers)| {
                    serde_json::json!({
                        "provides": label,
                        "offers": offers.iter().map(offer_json).collect::<Vec<_>>(),
                    })
                })
                .collect();
            format!(
                "{}\n",
                serde_json::to_string_pretty(&serde_json::json!({
                    "providers": labels,
                    "skipped": skipped_json(&index),
                }))
                .unwrap_or_default()
            )
        } else {
            let offers = index.all_offers();
            let mut out = String::new();
            let _ = writeln!(out, "providers = {}", offers.len());
            for (label, offers) in &offers {
                let _ = writeln!(out, "  {label}");
                for offer in offers {
                    let _ = writeln!(out, "    {}", describe_offer(offer));
                }
            }
            push_skipped(&mut out, &index);
            out
        }),
        ProviderCommand::Show { object, json } => {
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let candidate = index
                .candidates
                .iter()
                .find(|candidate| candidate.object == object_id)
                .ok_or_else(|| {
                    let reason = index
                        .skipped
                        .iter()
                        .find(|skipped| skipped.object == object_id)
                        .map(|skipped| skipped.reason.clone())
                        .unwrap_or_else(|| "it is not held".to_owned());
                    CliError::ProviderUnavailable {
                        object: object_id.clone(),
                        reason,
                    }
                })?;
            let manifest = &candidate.manifest;
            Ok(if json {
                format!(
                    "{}\n",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "object": candidate.object.to_string(),
                        "snapshot": candidate.snapshot.to_string(),
                        "revision": candidate.revision.as_str(),
                        "provides": manifest.provides().iter().map(|declaration| serde_json::json!({
                            "provides": declaration.provides(),
                            "version": declaration.version(),
                            "target": declaration.artifact().map(|a| a.target()),
                            "output": declaration.artifact().map(|a| a.output()),
                        })).collect::<Vec<_>>(),
                        "build_targets": manifest.build_targets().iter().map(|target| target.name()).collect::<Vec<_>>(),
                    }))
                    .unwrap_or_default()
                )
            } else {
                let mut out = String::new();
                let _ = writeln!(out, "object = {}", candidate.object);
                let _ = writeln!(out, "snapshot = {}", candidate.snapshot);
                let _ = writeln!(out, "revision = {}", candidate.revision.as_str());
                let _ = writeln!(out, "provides = {}", manifest.provides().len());
                for declaration in manifest.provides() {
                    let _ = write!(out, "  {}", declaration.provides());
                    if let Some(version) = declaration.version() {
                        let _ = write!(out, " version {version}");
                    }
                    match declaration.artifact() {
                        Some(artifact) => {
                            let _ = writeln!(
                                out,
                                " from target {} output {}",
                                artifact.target(),
                                artifact.output()
                            );
                        }
                        None => {
                            let _ = writeln!(out, " (content)");
                        }
                    }
                }
                let _ = writeln!(out, "build_targets = {}", manifest.build_targets().len());
                for target in manifest.build_targets() {
                    let _ = writeln!(out, "  {}", target.name());
                }
                out
            })
        }
        ProviderCommand::Resolve {
            provides,
            version,
            json,
        } => {
            let offers = index.offers(&provides);
            let (config, _) = load_config(paths)?;
            let tools = host_tools(&config);
            let mut matching = Vec::new();
            let mut rejected = Vec::new();
            for offer in offers {
                if version_matches(version.as_deref(), offer.version.as_deref()) {
                    matching.push(offer);
                } else {
                    rejected.push(offer);
                }
            }
            let host: Vec<&HostTool> = tools
                .iter()
                .filter(|tool| tool.provides == provides)
                .filter(|tool| version_matches(version.as_deref(), tool.version.as_deref()))
                .collect();
            Ok(if json {
                format!(
                    "{}\n",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "provides": provides,
                        "version": version,
                        "candidates": matching.iter().map(offer_json).collect::<Vec<_>>(),
                        "host_tools": host.iter().map(|tool| serde_json::json!({
                            "path": tool.path.display().to_string(),
                            "version": tool.version,
                        })).collect::<Vec<_>>(),
                        "rejected": rejected.iter().map(offer_json).collect::<Vec<_>>(),
                        "skipped": skipped_json(&index),
                    }))
                    .unwrap_or_default()
                )
            } else {
                let mut out = String::new();
                let _ = write!(out, "provides = {provides}");
                if let Some(version) = &version {
                    let _ = write!(out, " {version}");
                }
                out.push('\n');
                let chosen = matching
                    .first()
                    .map(describe_offer)
                    .or_else(|| {
                        host.first().map(|tool| {
                            format!("host tool {} (development mode)", tool.path.display())
                        })
                    })
                    .unwrap_or_else(|| "nothing held".to_owned());
                let _ = writeln!(out, "chosen = {chosen}");
                let _ = writeln!(out, "candidates = {}", matching.len());
                for offer in &matching {
                    let _ = writeln!(out, "  {}", describe_offer(offer));
                }
                if !host.is_empty() {
                    let _ = writeln!(out, "host_tools = {}", host.len());
                    for tool in &host {
                        let _ = writeln!(out, "  {}", tool.path.display());
                    }
                }
                if !rejected.is_empty() {
                    let _ = writeln!(out, "rejected = {}", rejected.len());
                    for offer in &rejected {
                        let _ = writeln!(
                            out,
                            "  {} (version {})",
                            describe_offer(offer),
                            offer.version.as_deref().unwrap_or("unknown")
                        );
                    }
                }
                push_skipped(&mut out, &index);
                out
            })
        }
    }
}

fn describe_offer(offer: &kairo_resolver::Offer) -> String {
    let mut text = match &offer.artifact {
        Some((target, output)) => {
            format!("object:{} target {target} output {output}", offer.object)
        }
        None => format!("object:{} (content)", offer.object),
    };
    if let Some(version) = &offer.version {
        let _ = write!(text, " version {version}");
    }
    text
}

fn offer_json(offer: &kairo_resolver::Offer) -> serde_json::Value {
    serde_json::json!({
        "object": offer.object.to_string(),
        "snapshot": offer.snapshot.to_string(),
        "version": offer.version,
        "target": offer.artifact.as_ref().map(|(target, _)| target),
        "output": offer.artifact.as_ref().map(|(_, output)| output),
    })
}

fn skipped_json(index: &Index) -> Vec<serde_json::Value> {
    index
        .skipped
        .iter()
        .map(|skipped| serde_json::json!({ "object": skipped.object.to_string(), "reason": skipped.reason }))
        .collect()
}

fn push_skipped(out: &mut String, index: &Index) {
    if index.skipped.is_empty() {
        return;
    }
    let _ = writeln!(out, "skipped:");
    for skipped in &index.skipped {
        let _ = writeln!(out, "  object:{}: {}", skipped.object, skipped.reason);
    }
}
