use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::process::ExitCode;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use clap::{Parser, Subcommand};
use kairo_identity::json::ActorGenesisJson;
use kairo_identity::{ActorGenesisBody, MemoryActorResolver, PublicKey};
use kairo_object::{
    validate_revision_manifest, DependencyDeclaration, ObjectDependencySelector, ObjectManifest,
};
use kairo_statement::json::ObjectRevisionStatementJson;
use kairo_statement::verify::{
    verify_envelope_statement, ActorResolution, SignatureStatus, TrustEvaluation,
    VerificationReport,
};
use kairo_statement::ObjectRevisionBody;

#[derive(Debug, Parser)]
#[command(name = "kairo", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Work with actors.
    Actor {
        #[command(subcommand)]
        command: ActorCommand,
    },
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
enum ActorCommand {
    /// Derive an ActorId from an ActorGenesis JSON document.
    Id {
        #[arg(long)]
        genesis: PathBuf,
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
    /// Verify an ObjectRevision JSON statement signature with a raw ed25519 public key.
    VerifySignature {
        #[arg(long)]
        statement: PathBuf,
        #[arg(long, conflicts_with = "public_key_file")]
        public_key: Option<String>,
        #[arg(long, conflicts_with = "public_key")]
        public_key_file: Option<PathBuf>,
    },
    /// Verify an ObjectRevision signature against an ActorGenesis initial key.
    VerifyActorGenesis {
        #[arg(long)]
        statement: PathBuf,
        #[arg(long)]
        actor_genesis: PathBuf,
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
        Some(Command::Actor { command }) => run_actor_command(command),
        Some(Command::Manifest { command }) => run_manifest_command(command),
        Some(Command::Revision { command }) => run_revision_command(command),
        None => Ok(help_text()),
    }
}

fn run_actor_command(command: ActorCommand) -> Result<String, CliError> {
    match command {
        ActorCommand::Id { genesis } => {
            let genesis = read_actor_genesis(genesis)?;
            Ok(format!("{}\n", genesis.actor_id()))
        }
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
        RevisionCommand::VerifySignature {
            statement,
            public_key,
            public_key_file,
        } => {
            let statement = read_object_revision_statement(statement)?;
            let public_key = read_public_key(public_key, public_key_file)?;
            statement
                .verify_signature(&public_key)
                .map_err(CliError::VerifyStatementSignature)?;

            Ok(format_revision_signature_valid(
                statement.unsigned().body(),
                statement.signature(),
            ))
        }
        RevisionCommand::VerifyActorGenesis {
            statement,
            actor_genesis,
        } => {
            let statement = read_object_revision_statement(statement)?;
            let actor_genesis = read_actor_genesis(actor_genesis)?;
            let mut resolver = MemoryActorResolver::new();
            resolver.insert(actor_genesis);
            let report = verify_envelope_statement(&statement, &resolver);

            if report.is_cryptographically_valid() {
                Ok(format_verification_report(
                    statement.unsigned().body(),
                    &report,
                ))
            } else {
                Err(CliError::VerificationFailed(Box::new(report)))
            }
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

fn read_actor_genesis(path: PathBuf) -> Result<ActorGenesisBody, CliError> {
    let input = std::fs::read_to_string(&path).map_err(|source| CliError::ReadActorGenesis {
        path: path.clone(),
        source,
    })?;

    let dto: ActorGenesisJson =
        serde_json::from_str(&input).map_err(CliError::ParseActorGenesisJson)?;
    dto.to_body().map_err(CliError::ParseActorGenesis)
}

fn read_public_key(
    public_key: Option<String>,
    public_key_file: Option<PathBuf>,
) -> Result<PublicKey, CliError> {
    let encoded = match (public_key, public_key_file) {
        (Some(public_key), None) => public_key,
        (None, Some(path)) => {
            std::fs::read_to_string(&path).map_err(|source| CliError::ReadPublicKey {
                path: path.clone(),
                source,
            })?
        }
        (None, None) => return Err(CliError::MissingPublicKey),
        (Some(_), Some(_)) => return Err(CliError::ConflictingPublicKeyInputs),
    };

    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|_| CliError::InvalidPublicKeyBase64)?;
    let bytes =
        <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| CliError::InvalidPublicKeyLength {
            expected: 32,
            actual: bytes.len(),
        })?;

    Ok(PublicKey::ed25519(bytes))
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

fn format_revision_signature_valid(
    revision: &ObjectRevisionBody,
    signature: &kairo_statement::Signature,
) -> String {
    format!(
        "valid revision signature\nobject = {}\nrevision = {}\nactor = {}\nkey_id = {}\nsignature = valid\n",
        revision.object(),
        revision.revision().as_str(),
        signature.actor(),
        signature.key_id()
    )
}

fn format_verification_report(
    revision: &ObjectRevisionBody,
    report: &VerificationReport,
) -> String {
    format!(
        "valid revision actor genesis\n\
         object = {}\n\
         revision = {}\n\
         actor = {}\n\
         statement_id = {}\n\
         signature = {}\n\
         actor_resolution = {}\n\
         trust = {}\n",
        revision.object(),
        revision.revision().as_str(),
        report.envelope_actor,
        report.statement_id,
        format_signature_status(&report.signature),
        format_actor_resolution(&report.actor),
        format_trust(&report.trust),
    )
}

fn format_signature_status(status: &SignatureStatus) -> &'static str {
    match status {
        SignatureStatus::Valid => "valid",
        SignatureStatus::Invalid => "invalid",
        SignatureStatus::UnsupportedAlgorithm(_) => "unsupported-algorithm",
        SignatureStatus::Malformed { .. } => "malformed",
        SignatureStatus::AlgorithmMismatch => "algorithm-mismatch",
        SignatureStatus::NotEvaluated => "not-evaluated",
    }
}

fn format_actor_resolution(resolution: &ActorResolution) -> &'static str {
    match resolution {
        ActorResolution::Resolved => "resolved",
        ActorResolution::NotFound => "not-found",
        ActorResolution::ResolverUnavailable(_) => "resolver-unavailable",
        ActorResolution::SignatureActorMismatch => "signature-actor-mismatch",
    }
}

fn format_trust(trust: &TrustEvaluation) -> &'static str {
    match trust {
        TrustEvaluation::Unevaluated => "unevaluated",
    }
}

fn describe_verification_failure(report: &VerificationReport) -> String {
    let mut parts = Vec::new();
    match &report.actor {
        ActorResolution::Resolved => {}
        ActorResolution::NotFound => parts.push(format!(
            "actor {} could not be resolved",
            report.envelope_actor
        )),
        ActorResolution::ResolverUnavailable(reason) => {
            parts.push(format!("actor resolver unavailable: {reason}"));
        }
        ActorResolution::SignatureActorMismatch => parts.push(format!(
            "signature actor {} does not match envelope actor {}",
            report.signature_actor, report.envelope_actor
        )),
    }
    match &report.signature {
        SignatureStatus::Valid | SignatureStatus::NotEvaluated => {}
        SignatureStatus::Invalid => parts.push("signature did not verify".to_owned()),
        SignatureStatus::UnsupportedAlgorithm(algorithm) => {
            parts.push(format!("unsupported signature algorithm {algorithm}"));
        }
        SignatureStatus::Malformed {
            expected_len,
            actual_len,
        } => parts.push(format!(
            "malformed signature length {actual_len}; expected {expected_len}"
        )),
        SignatureStatus::AlgorithmMismatch => {
            parts.push("signature algorithm does not match resolved key".to_owned());
        }
    }
    if parts.is_empty() {
        "verification failed".to_owned()
    } else {
        parts.join("; ")
    }
}

fn help_text() -> String {
    "kairo\n\nUsage:\n  kairo --help\n  kairo --version\n  kairo actor id --genesis <path>\n  kairo manifest hash [path]\n  kairo manifest inspect [path]\n  kairo revision validate-manifest --statement <path> [--manifest <path>]\n  kairo revision verify-signature --statement <path> (--public-key <base64>|--public-key-file <path>)\n  kairo revision verify-actor-genesis --statement <path> --actor-genesis <path>\n".to_owned()
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
    ReadPublicKey {
        path: PathBuf,
        source: std::io::Error,
    },
    ReadActorGenesis {
        path: PathBuf,
        source: std::io::Error,
    },
    ParseActorGenesisJson(serde_json::Error),
    ParseActorGenesis(kairo_identity::json::ActorGenesisJsonError),
    ParseStatementJson(serde_json::Error),
    ParseStatement(kairo_statement::json::StatementJsonError),
    ValidateRevisionManifest(kairo_object::RevisionManifestError),
    VerifyStatementSignature(kairo_statement::StatementSignatureError),
    VerificationFailed(Box<VerificationReport>),
    MissingPublicKey,
    ConflictingPublicKeyInputs,
    InvalidPublicKeyBase64,
    InvalidPublicKeyLength {
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadManifest { path, source }
            | Self::ReadStatement { path, source }
            | Self::ReadPublicKey { path, source }
            | Self::ReadActorGenesis { path, source } => {
                write!(f, "failed to read {}: {source}", path.display())
            }
            Self::ParseManifest(error) => write!(f, "{error}"),
            Self::ParseActorGenesisJson(error) => {
                write!(f, "invalid actor genesis JSON: {error}")
            }
            Self::ParseActorGenesis(error) => write!(f, "{error}"),
            Self::ParseStatementJson(error) => write!(f, "invalid statement JSON: {error}"),
            Self::ParseStatement(error) => write!(f, "{error}"),
            Self::ValidateRevisionManifest(error) => write!(f, "{error}"),
            Self::VerifyStatementSignature(error) => write!(f, "{error}"),
            Self::VerificationFailed(report) => {
                write!(f, "{}", describe_verification_failure(report))
            }
            Self::MissingPublicKey => f.write_str("missing --public-key or --public-key-file"),
            Self::ConflictingPublicKeyInputs => {
                f.write_str("use only one of --public-key or --public-key-file")
            }
            Self::InvalidPublicKeyBase64 => f.write_str("invalid public key base64"),
            Self::InvalidPublicKeyLength { expected, actual } => {
                write!(f, "invalid public key length {actual}; expected {expected}")
            }
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadManifest { source, .. }
            | Self::ReadStatement { source, .. }
            | Self::ReadPublicKey { source, .. }
            | Self::ReadActorGenesis { source, .. } => Some(source),
            Self::ParseManifest(error) => Some(error),
            Self::ParseActorGenesisJson(error) | Self::ParseStatementJson(error) => Some(error),
            Self::ParseActorGenesis(error) => Some(error),
            Self::ParseStatement(error) => Some(error),
            Self::ValidateRevisionManifest(error) => Some(error),
            Self::VerifyStatementSignature(error) => Some(error),
            Self::VerificationFailed(_)
            | Self::MissingPublicKey
            | Self::ConflictingPublicKeyInputs
            | Self::InvalidPublicKeyBase64
            | Self::InvalidPublicKeyLength { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};
    use kairo_core::canonical::CanonicalEncode;
    use kairo_identity::json::{ActorGenesisJson, PublicKeyJson};
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

    #[test]
    fn parses_revision_verify_signature_command() {
        let cli = Cli::try_parse_from([
            "kairo",
            "revision",
            "verify-signature",
            "--statement",
            "revision.json",
            "--public-key",
            "ZmFrZQ==",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli {
                command: Some(Command::Revision {
                    command: RevisionCommand::VerifySignature {
                        statement,
                        public_key: Some(public_key),
                        public_key_file: None
                    }
                })
            }) if statement.as_os_str() == "revision.json" && public_key == "ZmFrZQ=="
        ));
    }

    #[test]
    fn parses_actor_id_command() {
        let cli = Cli::try_parse_from(["kairo", "actor", "id", "--genesis", "actor.json"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                command: Some(Command::Actor {
                    command: ActorCommand::Id { genesis }
                })
            }) if genesis.as_os_str() == "actor.json"
        ));
    }

    #[test]
    fn parses_revision_verify_actor_genesis_command() {
        let cli = Cli::try_parse_from([
            "kairo",
            "revision",
            "verify-actor-genesis",
            "--statement",
            "revision.json",
            "--actor-genesis",
            "actor.json",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli {
                command: Some(Command::Revision {
                    command: RevisionCommand::VerifyActorGenesis {
                        statement,
                        actor_genesis
                    }
                })
            }) if statement.as_os_str() == "revision.json" && actor_genesis.as_os_str() == "actor.json"
        ));
    }

    #[test]
    fn verifies_revision_signature_for_output() {
        let statement = signed_revision_statement();
        let public_key = PublicKey::ed25519(signing_key().verifying_key().to_bytes());
        let output = statement.and_then(|statement| {
            statement.verify_signature(&public_key).ok().map(|_| {
                format_revision_signature_valid(statement.unsigned().body(), statement.signature())
            })
        });

        assert!(
            matches!(output, Some(output) if output.contains("valid revision signature")
            && output.contains("signature = valid")
            && output.contains("key_id = z"))
        );
    }

    #[test]
    fn formats_actor_genesis_verified_revision_output() {
        let actor_genesis = actor_genesis_dto().to_body();
        let output = actor_genesis.ok().and_then(|actor_genesis| {
            let statement =
                signed_revision_statement_for_actor(actor_genesis.actor_id().to_string())?;
            let mut resolver = MemoryActorResolver::new();
            resolver.insert(actor_genesis);
            let report = verify_envelope_statement(&statement, &resolver);
            if report.is_cryptographically_valid() {
                Some(format_verification_report(
                    statement.unsigned().body(),
                    &report,
                ))
            } else {
                None
            }
        });

        assert!(
            matches!(output, Some(output) if output.contains("valid revision actor genesis")
            && output.contains("signature = valid")
            && output.contains("actor_resolution = resolved")
            && output.contains("trust = unevaluated")
            && output.contains("actor = z"))
        );
    }

    #[test]
    fn actor_genesis_id_output_is_actor_id() {
        let output = actor_genesis_dto()
            .to_body()
            .map(|actor_genesis| format!("{}\n", actor_genesis.actor_id()));

        assert!(matches!(output, Ok(output) if output.starts_with('z') && output.ends_with('\n')));
    }

    #[test]
    fn reads_inline_public_key_base64() {
        let encoded = STANDARD.encode(signing_key().verifying_key().to_bytes());
        let public_key = read_public_key(Some(encoded), None);

        assert!(
            matches!(public_key, Ok(public_key) if public_key.bytes() == &signing_key().verifying_key().to_bytes())
        );
    }

    fn revision_dto(manifest_hash: String) -> ObjectRevisionStatementJson {
        revision_dto_for_actor(ACTOR_ID.to_owned(), manifest_hash)
    }

    fn revision_dto_for_actor(
        actor_id: String,
        manifest_hash: String,
    ) -> ObjectRevisionStatementJson {
        ObjectRevisionStatementJson {
            statement_type: "ObjectRevision".to_owned(),
            version: 1,
            actor: actor_id.clone(),
            subject: format!("object:{OBJECT_ID}"),
            created_at: "2026-05-01T14:32:07Z".to_owned(),
            body: ObjectRevisionBodyJson {
                object: OBJECT_ID.to_owned(),
                revision: "git:sha256:revision".to_owned(),
                parents: Vec::new(),
                manifest_hash,
                attests_reachable_history: true,
            },
            signature: SignatureJson {
                actor: actor_id,
                key_id: "primary".to_owned(),
                algorithm: "example".to_owned(),
                bytes: "c2lnbmF0dXJl".to_owned(),
            },
        }
    }

    fn signed_revision_statement() -> Option<kairo_statement::SignedStatement<ObjectRevisionBody>> {
        signed_revision_statement_for_actor(ACTOR_ID.to_owned())
    }

    fn signed_revision_statement_for_actor(
        actor_id: String,
    ) -> Option<kairo_statement::SignedStatement<ObjectRevisionBody>> {
        let manifest = ObjectManifest::parse_toml(MANIFEST).ok()?;
        let mut dto = revision_dto_for_actor(actor_id, manifest.manifest_hash().to_string());
        let unsigned = dto.to_statement().ok()?.unsigned().clone();
        let signature = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
        dto.signature.algorithm = "ed25519".to_owned();
        dto.signature.key_id = PublicKey::ed25519(signing_key().verifying_key().to_bytes())
            .key_id()
            .to_string();
        dto.signature.bytes = STANDARD.encode(signature);
        dto.to_statement().ok()
    }

    fn actor_genesis_dto() -> ActorGenesisJson {
        ActorGenesisJson {
            statement_type: "ActorGenesis".to_owned(),
            version: 1,
            actor_kind: "person".to_owned(),
            initial_key: PublicKeyJson {
                algorithm: "ed25519".to_owned(),
                bytes: STANDARD.encode(signing_key().verifying_key().to_bytes()),
            },
            created_at: "2026-05-01T14:32:07Z".to_owned(),
            nonce: "0909090909090909090909090909090909090909090909090909090909090909".to_owned(),
        }
    }

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }
}
