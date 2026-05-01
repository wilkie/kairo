use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use kairo_object::{
    validate_revision_manifest, DependencyDeclaration, ObjectDependencySelector, ObjectManifest,
};
use kairo_statement::json::ObjectRevisionStatementJson;
use kairo_statement::ObjectRevisionBody;

#[derive(Debug, Parser)]
#[command(name = "kairo", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Work with kairo.toml manifests.
    Manifest {
        #[command(subcommand)]
        command: ManifestCommand,
    },
    /// Work with Object revisions.
    Revision {
        #[command(subcommand)]
        command: RevisionCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ManifestCommand {
    /// Print the canonical manifest BlobId.
    Hash {
        #[arg(default_value = "kairo.toml")]
        path: PathBuf,
    },
    /// Print parsed manifest details.
    Inspect {
        #[arg(default_value = "kairo.toml")]
        path: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum RevisionCommand {
    /// Validate an ObjectRevision JSON statement against a kairo.toml manifest.
    ValidateManifest {
        #[arg(long)]
        statement: PathBuf,
        #[arg(long, default_value = "kairo.toml")]
        manifest: PathBuf,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<String, CliError> {
    match cli.command {
        Some(Command::Manifest { command }) => run_manifest_command(command),
        Some(Command::Revision { command }) => run_revision_command(command),
        None => Ok(help_text()),
    }
}

fn run_manifest_command(command: ManifestCommand) -> Result<String, CliError> {
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

fn run_revision_command(command: RevisionCommand) -> Result<String, CliError> {
    match command {
        RevisionCommand::ValidateManifest {
            statement,
            manifest,
        } => {
            let statement = read_object_revision_statement(statement)?;
            let revision = statement.unsigned().body();
            let manifest = read_manifest(manifest)?;
            validate_revision_manifest(revision, &manifest)
                .map_err(CliError::ValidateRevisionManifest)?;

            Ok(format_revision_manifest_valid(revision, &manifest))
        }
    }
}

fn read_manifest(path: PathBuf) -> Result<ObjectManifest, CliError> {
    let input = std::fs::read_to_string(&path).map_err(|source| CliError::ReadManifest {
        path: path.clone(),
        source,
    })?;

    ObjectManifest::parse_toml(&input).map_err(CliError::ParseManifest)
}

fn read_object_revision_statement(
    path: PathBuf,
) -> Result<kairo_statement::SignedStatement<ObjectRevisionBody>, CliError> {
    let input = std::fs::read_to_string(&path).map_err(|source| CliError::ReadStatement {
        path: path.clone(),
        source,
    })?;

    let dto: ObjectRevisionStatementJson =
        serde_json::from_str(&input).map_err(CliError::ParseStatementJson)?;
    dto.to_statement().map_err(CliError::ParseStatement)
}

fn format_manifest_inspection(manifest: &ObjectManifest) -> String {
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
    for dependency in manifest.dependencies() {
        match dependency {
            DependencyDeclaration::Provides(dependency) => {
                output.push_str(&format!("  requires {}\n", dependency.provides()));
            }
            DependencyDeclaration::Object(dependency) => {
                output.push_str(&format!("  object {}\n", dependency.object()));
                match dependency.selector() {
                    ObjectDependencySelector::Version(version) => {
                        output.push_str(&format!("    version = {version}\n"));
                    }
                    ObjectDependencySelector::Snapshot(snapshot) => {
                        output.push_str(&format!("    snapshot = {snapshot}\n"));
                    }
                }
            }
        }
    }

    output
}

fn format_revision_manifest_valid(
    revision: &ObjectRevisionBody,
    manifest: &ObjectManifest,
) -> String {
    format!(
        "valid revision manifest\nobject = {}\nrevision = {}\nmanifest_hash = {}\n",
        revision.object(),
        revision.revision().as_str(),
        manifest.manifest_hash()
    )
}

fn help_text() -> String {
    "kairo\n\nUsage:\n  kairo --help\n  kairo --version\n  kairo manifest hash [path]\n  kairo manifest inspect [path]\n  kairo revision validate-manifest --statement <path> [--manifest <path>]\n".to_owned()
}

#[derive(Debug)]
enum CliError {
    ReadManifest {
        path: PathBuf,
        source: std::io::Error,
    },
    ParseManifest(kairo_object::ManifestError),
    ReadStatement {
        path: PathBuf,
        source: std::io::Error,
    },
    ParseStatementJson(serde_json::Error),
    ParseStatement(kairo_statement::json::StatementJsonError),
    ValidateRevisionManifest(kairo_object::RevisionManifestError),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadManifest { path, source } | Self::ReadStatement { path, source } => {
                write!(f, "failed to read {}: {source}", path.display())
            }
            Self::ParseManifest(error) => write!(f, "{error}"),
            Self::ParseStatementJson(error) => write!(f, "invalid statement JSON: {error}"),
            Self::ParseStatement(error) => write!(f, "{error}"),
            Self::ValidateRevisionManifest(error) => write!(f, "{error}"),
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadManifest { source, .. } | Self::ReadStatement { source, .. } => Some(source),
            Self::ParseManifest(error) => Some(error),
            Self::ParseStatementJson(error) => Some(error),
            Self::ParseStatement(error) => Some(error),
            Self::ValidateRevisionManifest(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_statement::json::{
        ObjectRevisionBodyJson, ObjectRevisionStatementJson, SignatureJson,
    };

    const ACTOR_ID: &str = "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t";
    const OBJECT_ID: &str = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";
    const MANIFEST: &str = r#"
        [kairo]
        schema = 1
        object = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk"
        kind = "software"
        name = "Example"

        [content]
        kind = "tree"

        [[provides]]
        provides = "tool:make"
        version = "3.81"

        [[dependencies]]
        kind = "provides"
        provides = "lib:zlib:static"
    "#;

    #[test]
    fn inspect_output_includes_manifest_details() {
        let manifest = ObjectManifest::parse_toml(MANIFEST);
        let output = manifest.map(|manifest| format_manifest_inspection(&manifest));

        assert!(
            matches!(output, Ok(output) if output.contains("manifest_hash = z")
            && output.contains("kind = software")
            && output.contains("provides tool:make")
            && output.contains("requires lib:zlib:static"))
        );
    }

    #[test]
    fn parses_manifest_hash_command() {
        let cli = Cli::try_parse_from(["kairo", "manifest", "hash", "custom.toml"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                command: Some(Command::Manifest {
                    command: ManifestCommand::Hash { path }
                })
            }) if path.as_os_str() == "custom.toml"
        ));
    }

    #[test]
    fn parses_manifest_inspect_default_path() {
        let cli = Cli::try_parse_from(["kairo", "manifest", "inspect"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                command: Some(Command::Manifest {
                    command: ManifestCommand::Inspect { path }
                })
            }) if path.as_os_str() == "kairo.toml"
        ));
    }

    #[test]
    fn parses_revision_validate_manifest_command() {
        let cli = Cli::try_parse_from([
            "kairo",
            "revision",
            "validate-manifest",
            "--statement",
            "revision.json",
            "--manifest",
            "kairo.toml",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli {
                command: Some(Command::Revision {
                    command: RevisionCommand::ValidateManifest {
                        statement,
                        manifest
                    }
                })
            }) if statement.as_os_str() == "revision.json" && manifest.as_os_str() == "kairo.toml"
        ));
    }

    #[test]
    fn formats_valid_revision_manifest_output() {
        let output = ObjectManifest::parse_toml(MANIFEST)
            .ok()
            .and_then(|manifest| {
                let dto = revision_dto(manifest.manifest_hash().to_string());
                dto.to_statement().ok().map(|statement| {
                    format_revision_manifest_valid(statement.unsigned().body(), &manifest)
                })
            });

        assert!(
            matches!(output, Some(output) if output.contains("valid revision manifest")
            && output.contains("object = z")
            && output.contains("revision = git:sha256:revision")
            && output.contains("manifest_hash = z"))
        );
    }

    fn revision_dto(manifest_hash: String) -> ObjectRevisionStatementJson {
        ObjectRevisionStatementJson {
            statement_type: "ObjectRevision".to_owned(),
            version: 1,
            actor: ACTOR_ID.to_owned(),
            subject: format!("object:{OBJECT_ID}"),
            body: ObjectRevisionBodyJson {
                object: OBJECT_ID.to_owned(),
                revision: "git:sha256:revision".to_owned(),
                parents: Vec::new(),
                manifest_hash,
                attests_reachable_history: true,
            },
            signature: SignatureJson {
                actor: ACTOR_ID.to_owned(),
                key_id: "primary".to_owned(),
                algorithm: "example".to_owned(),
                bytes: "signature".to_owned(),
            },
        }
    }
}
