//! `kairo build --plan-only` and `kairo provider ...` runners
//! (`specs/DECISIONS.md` §33 sub-decisions 2–3, `specs/CLI.md` §17).
//!
//! Planning reads each candidate's `kairo.toml` from the managed Git
//! cache at its revision's commit, so an object whose tree isn't held
//! is reported as skipped rather than considered. Build results don't
//! exist yet, so every artifact provider is a build the plan needs
//! first. Running the plan is the next slice.

use std::fmt::Write as _;

use kairo_core::{KairoRef, ObjectId};
use kairo_object::ObjectManifest;
use kairo_resolver::{
    describe_resolved, version_matches, Host, HostTool, Index, ManifestSource, NoArtifacts,
    Resolution, Resolver, SnapshotSelector, SourceError,
};
use kairo_statement::RevisionId;
use kairo_store::BlobStore;

use crate::cli::ProviderCommand;
use crate::commands::config::load_config;
use crate::error::CliError;
use crate::store_paths::{open_store, StorePaths};

pub(crate) struct BuildArgs<'a> {
    pub snapshot_ref: &'a str,
    pub target: &'a str,
    pub plan_only: bool,
    pub json: bool,
}

/// Manifests from the managed Git cache, at the revision's commit.
struct CacheManifests<'a> {
    paths: &'a StorePaths,
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

fn host_tools(paths: &StorePaths) -> Result<Vec<HostTool>, CliError> {
    let (config, _) = load_config(paths)?;
    Ok(config
        .runtime
        .host_tools
        .into_iter()
        .map(|tool| HostTool {
            provides: tool.provides,
            version: tool.version,
            path: tool.path,
        })
        .collect())
}

pub(crate) fn run_build(args: &BuildArgs<'_>, paths: &StorePaths) -> Result<String, CliError> {
    if !args.plan_only {
        return Err(CliError::BuildNotYetSupported);
    }
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
    let manifests = CacheManifests { paths };
    let host = Host::detect();
    let tools = host_tools(paths)?;
    let resolution = Resolver::new(&store, &manifests, &NoArtifacts, &host, &tools)
        .resolve(&object, &selector, args.target)
        .map_err(|error| CliError::Resolve(Box::new(error)))?;

    // The plan and what it needs first are stored, so a later `kairo
    // build` and `kairo reproduce` can cite them by hash.
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

    Ok(if args.json {
        format_resolution_json(&resolution)
    } else {
        format_resolution(&resolution)
    })
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
            let tools = host_tools(paths)?;
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
