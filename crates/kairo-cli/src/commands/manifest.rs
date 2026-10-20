//! `kairo manifest hash` and `kairo manifest inspect` command runners.

use std::path::PathBuf;

use kairo_object::{DependencyDeclaration, ObjectDependencySelector, ObjectManifest};

use crate::cli::ManifestCommand;
use crate::error::CliError;

pub(crate) fn run_manifest_command(command: ManifestCommand) -> Result<String, CliError> {
    match command {
        ManifestCommand::Hash { path } => {
            let manifest = read_manifest(path)?;
            Ok(format!("{}\n", manifest.manifest_hash()))
        }
        ManifestCommand::Inspect { path } => {
            let manifest = read_manifest(path)?;
            Ok(format_manifest_inspection(&manifest))
        }
    }
}

pub(crate) fn read_manifest(path: PathBuf) -> Result<ObjectManifest, CliError> {
    let input = std::fs::read_to_string(&path).map_err(|source| CliError::ReadManifest {
        path: path.clone(),
        source,
    })?;

    ObjectManifest::parse_toml(&input).map_err(CliError::ParseManifest)
}

pub(crate) fn format_manifest_inspection(manifest: &ObjectManifest) -> String {
    let mut output = String::new();
    output.push_str(&format!("manifest_hash = {}\n", manifest.manifest_hash()));
    output.push_str(&format!("schema = {}\n", manifest.kairo().schema()));
    output.push_str(&format!("kind = {}\n", manifest.kairo().kind()));
    output.push_str(&format!("name = {}\n", manifest.kairo().name()));

    if let Some(object) = manifest.kairo().object() {
        output.push_str(&format!("object = {object}\n"));
    }

    if let Some(summary) = manifest.kairo().summary() {
        output.push_str(&format!("summary = {summary}\n"));
    }

    if let Some(content) = manifest.content() {
        output.push_str(&format!("content.kind = {}\n", content.kind()));
    }

    output.push_str(&format!("provides = {}\n", manifest.provides().len()));
    for provides in manifest.provides() {
        output.push_str(&format!("  provides {}\n", provides.provides()));
        if let Some(version) = provides.version() {
            output.push_str(&format!("    version = {version}\n"));
        }
    }

    output.push_str(&format!(
        "dependencies = {}\n",
        manifest.dependencies().len()
    ));
    push_dependencies(&mut output, manifest.dependencies(), "  ");

    output.push_str(&format!(
        "build.targets = {}\n",
        manifest.build_targets().len()
    ));
    for target in manifest.build_targets() {
        output.push_str(&format!("  target {}\n", target.name()));
        output.push_str(&format!("    command = {:?}\n", target.command()));
        output.push_str(&format!("    workdir = {}\n", target.workdir()));
        push_dependencies(&mut output, target.dependencies(), "    ");
        for built in target.outputs() {
            output.push_str(&format!(
                "    output {} ({}) {} -> {}\n",
                built.name(),
                built.kind().as_str(),
                built.path(),
                built.artifact_path()
            ));
        }
    }

    output
}

fn push_dependencies(output: &mut String, dependencies: &[DependencyDeclaration], indent: &str) {
    for dependency in dependencies {
        match dependency {
            DependencyDeclaration::Provides(dependency) => {
                output.push_str(&format!("{indent}requires {}\n", dependency.provides()));
                if let Some(version) = dependency.version() {
                    output.push_str(&format!("{indent}  version = {version}\n"));
                }
            }
            DependencyDeclaration::Object(dependency) => {
                output.push_str(&format!("{indent}object {}\n", dependency.object()));
                match dependency.selector() {
                    ObjectDependencySelector::Version(version) => {
                        output.push_str(&format!("{indent}  version = {version}\n"));
                    }
                    ObjectDependencySelector::Snapshot(snapshot) => {
                        output.push_str(&format!("{indent}  snapshot = {snapshot}\n"));
                    }
                }
            }
        }
    }
}
