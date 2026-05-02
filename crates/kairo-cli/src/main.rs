use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::process::ExitCode;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use clap::{Parser, Subcommand};
use kairo_core::canonical::CanonicalEncode;
use kairo_core::{ActorId, KairoRef, ObjectId, Timestamp};
use kairo_identity::json::ActorGenesisJson;
use kairo_identity::{
    generate_nonce, ActorGenesisBody, ActorKind, MemoryActorResolver, PublicKey, SecretSigningKey,
};
use kairo_keystore::{FilesystemKeystore, Keystore};
use kairo_object::{
    validate_revision_manifest, DependencyDeclaration, ObjectDependencySelector, ObjectManifest,
    Snapshot, SnapshotError,
};
use kairo_statement::json::{ObjectGenesisStatementJson, ObjectRevisionStatementJson};
use kairo_statement::verify::{
    verify_envelope_statement, ActorResolution, SignatureStatus, TrustEvaluation,
    VerificationReport,
};
use kairo_statement::{
    ObjectBranchBody, ObjectGenesisBody, ObjectGenesisStatement, ObjectKind, ObjectRevisionBody,
    RevisionId, Signature, SignedStatement, UnsignedStatement,
};
use kairo_store::{ActorStore, BranchResolver, FilesystemStore, ObjectStore, StatementStore};

#[derive(Debug, Parser)]
#[command(name = "kairo", version)]
struct Cli {
    /// Override the store root (default ~/.kairo).
    #[arg(long, env = "KAIRO_STORE", global = true)]
    store: Option<PathBuf>,

    /// Override the keystore directory (default <store>/keys).
    #[arg(long, env = "KAIRO_KEYS", global = true)]
    keys: Option<PathBuf>,

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
    /// Work with Objects.
    Object {
        #[command(subcommand)]
        command: ObjectSubcommand,
    },
    /// Work with Object revisions.
    Revision {
        #[command(subcommand)]
        command: RevisionCommand,
    },
    /// Work with named, mutable revision pointers (ObjectBranch).
    Branch {
        #[command(subcommand)]
        command: BranchCommand,
    },
    /// Compute a SnapshotId for an object's effective state.
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
}

#[derive(Debug, Subcommand)]
enum SnapshotCommand {
    /// Resolve a snapshot for an object and print its SnapshotId.
    ///
    /// By default, follows the creator-actor's "head" branch. Override
    /// resolution with --actor, --name, or pin the frontier directly with
    /// --statement.
    Compute {
        /// Object whose snapshot to compute.
        #[arg(long)]
        object: String,
        /// Pin the frontier to a specific ObjectRevision statement,
        /// bypassing branch resolution. Conflicts with --actor and --name.
        #[arg(long, conflicts_with_all = ["actor", "name"])]
        statement: Option<String>,
        /// Actor whose branch tip to follow. Defaults to ObjectGenesis.created_by.
        #[arg(long)]
        actor: Option<String>,
        /// Branch name (defaults to "head").
        #[arg(long, default_value = "head")]
        name: String,
        /// Emit a stable JSON representation.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum BranchCommand {
    /// Sign a new ObjectBranch statement that points name at revision and
    /// supersedes any earlier branch with the same (actor, object, name).
    Set {
        /// Actor whose key signs the branch update.
        #[arg(long)]
        actor: String,
        /// Object whose lineage the branch belongs to.
        #[arg(long)]
        object: String,
        /// StatementId of the ObjectRevision the branch points at.
        #[arg(long)]
        revision: String,
        /// Branch name (defaults to "head").
        #[arg(long, default_value = "head")]
        name: String,
    },
    /// Resolve and print the current branch tip for (actor, object, name).
    Show {
        #[arg(long)]
        object: String,
        /// Actor whose branch tip to resolve. Defaults to ObjectGenesis.created_by.
        #[arg(long)]
        actor: Option<String>,
        /// Branch name (defaults to "head").
        #[arg(long, default_value = "head")]
        name: String,
        /// Emit a stable JSON representation.
        #[arg(long)]
        json: bool,
    },
    /// List all known (actor, name) branch tips for an object.
    List {
        #[arg(long)]
        object: String,
    },
}

#[derive(Debug, Subcommand)]
enum ActorCommand {
    /// Derive an ActorId from an ActorGenesis JSON document.
    Id {
        #[arg(long)]
        genesis: PathBuf,
    },
    /// Generate a fresh actor (keypair + ActorGenesis) and persist it.
    Create {
        /// Actor kind, e.g. person, project, organization, service.
        #[arg(long)]
        kind: String,
    },
    /// Import an ActorGenesis JSON document into the local store.
    Import {
        #[arg(long)]
        genesis: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum ObjectSubcommand {
    /// Create a new ObjectGenesis statement signed by the given actor.
    Create {
        /// Actor whose key signs the genesis statement.
        #[arg(long)]
        actor: String,
        /// Object kind, e.g. software, dataset, image.
        #[arg(long)]
        kind: String,
        /// Optional initial storage revision (e.g. git:sha256:<commit>).
        #[arg(long)]
        initial_revision: Option<String>,
    },
    /// Import a signed ObjectGenesis statement JSON into the local store.
    Import {
        #[arg(long)]
        statement: PathBuf,
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
        /// Emit a stable JSON representation of the verification report.
        #[arg(long)]
        json: bool,
    },
    /// Create a signed ObjectRevision statement and persist it to the store.
    Create {
        #[arg(long)]
        actor: String,
        #[arg(long)]
        object: String,
        #[arg(long)]
        revision: String,
        #[arg(long, default_value = "kairo.toml")]
        manifest: PathBuf,
        /// Storage parent revision (may be repeated for multi-parent statements).
        #[arg(long = "parent")]
        parents: Vec<String>,
        /// Suppress the default `attests_reachable_history = true` claim.
        #[arg(long)]
        no_attests_reachable_history: bool,
    },
    /// Import a signed ObjectRevision statement JSON into the local store.
    Import {
        #[arg(long)]
        statement: PathBuf,
    },
    /// Print the body fields of a stored ObjectRevision statement.
    Inspect {
        /// StatementId to inspect.
        #[arg(long)]
        statement: String,
        /// Emit a stable JSON representation.
        #[arg(long)]
        json: bool,
    },
    /// List ObjectRevision statements stored locally for an object.
    List {
        /// Filter to revisions whose body.object matches this id.
        #[arg(long)]
        object: String,
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
    let paths = StorePaths::resolve(cli.store, cli.keys)?;
    match cli.command {
        Some(Command::Actor { command }) => run_actor_command(command, &paths),
        Some(Command::Manifest { command }) => run_manifest_command(command),
        Some(Command::Object { command }) => run_object_command(command, &paths),
        Some(Command::Revision { command }) => run_revision_command(command, &paths),
        Some(Command::Branch { command }) => run_branch_command(command, &paths),
        Some(Command::Snapshot { command }) => run_snapshot_command(command, &paths),
        None => Ok(help_text()),
    }
}

#[derive(Debug, Clone)]
struct StorePaths {
    store: PathBuf,
    keys: PathBuf,
}

impl StorePaths {
    fn resolve(store: Option<PathBuf>, keys: Option<PathBuf>) -> Result<Self, CliError> {
        let store = match store {
            Some(path) => path,
            None => default_store_root()?,
        };
        let keys = keys.unwrap_or_else(|| store.join("keys"));
        Ok(Self { store, keys })
    }
}

fn default_store_root() -> Result<PathBuf, CliError> {
    match std::env::var_os("HOME") {
        Some(home) => Ok(PathBuf::from(home).join(".kairo")),
        None => Err(CliError::HomeNotSet),
    }
}

fn open_store(paths: &StorePaths) -> Result<FilesystemStore, CliError> {
    FilesystemStore::open(&paths.store).map_err(|error| CliError::OpenStore {
        path: paths.store.clone(),
        source: error,
    })
}

fn open_keystore(paths: &StorePaths) -> Result<FilesystemKeystore, CliError> {
    FilesystemKeystore::open(&paths.keys).map_err(|error| CliError::OpenKeystore {
        path: paths.keys.clone(),
        source: error,
    })
}

fn run_actor_command(command: ActorCommand, paths: &StorePaths) -> Result<String, CliError> {
    match command {
        ActorCommand::Id { genesis } => {
            let genesis = read_actor_genesis(genesis)?;
            Ok(format!("{}\n", genesis.actor_id()))
        }
        ActorCommand::Import { genesis } => {
            let body = read_actor_genesis(genesis)?;
            let store = open_store(paths)?;
            let actor_id = store
                .put_actor(&body)
                .map_err(|error| CliError::WriteActor {
                    actor: body.actor_id(),
                    source: error,
                })?;
            Ok(format!(
                "imported actor\nactor = {actor_id}\nstore = {}\n",
                paths.store.display()
            ))
        }
        ActorCommand::Create { kind } => {
            let store = open_store(paths)?;
            let keystore = open_keystore(paths)?;

            let secret = SecretSigningKey::generate_ed25519().map_err(CliError::GenerateKey)?;
            let nonce = generate_nonce().map_err(CliError::GenerateKey)?;

            let body = ActorGenesisBody::new(
                ActorKind::new(kind),
                secret.public_key(),
                Timestamp::now(),
                nonce,
            );
            let actor_id = body.actor_id();

            keystore
                .put_signing_key(&actor_id, &secret)
                .map_err(|error| CliError::WriteKey {
                    actor: actor_id.clone(),
                    source: error,
                })?;
            store
                .put_actor(&body)
                .map_err(|error| CliError::WriteActor {
                    actor: actor_id.clone(),
                    source: error,
                })?;

            Ok(format!(
                "created actor\nactor = {actor_id}\nkey_id = {}\nstore = {}\nkeys = {}\n",
                secret.public_key().key_id(),
                paths.store.display(),
                paths.keys.display()
            ))
        }
    }
}

fn run_object_command(command: ObjectSubcommand, paths: &StorePaths) -> Result<String, CliError> {
    match command {
        ObjectSubcommand::Import { statement } => {
            let signed = read_object_genesis_statement(statement)?;
            let store = open_store(paths)?;
            let object_id = store.put_object_genesis(&signed).map_err(|error| {
                CliError::WriteObjectGenesis {
                    object: signed.object_id(),
                    source: error,
                }
            })?;
            Ok(format!(
                "imported object genesis\nobject = {object_id}\ncreated_by = {}\nstore = {}\n",
                signed.body().created_by(),
                paths.store.display()
            ))
        }
        ObjectSubcommand::Create {
            actor,
            kind,
            initial_revision,
        } => {
            let actor_id = ActorId::new(actor.clone())
                .map_err(|source| CliError::ParseActorId { actor, source })?;

            let store = open_store(paths)?;
            let keystore = open_keystore(paths)?;

            let actor_body = store
                .get_actor(&actor_id)
                .map_err(|error| CliError::ReadActor {
                    actor: actor_id.clone(),
                    source: error,
                })?;
            let secret =
                keystore
                    .get_signing_key(&actor_id)
                    .map_err(|error| CliError::ReadKey {
                        actor: actor_id.clone(),
                        source: error,
                    })?;
            if &secret.public_key() != actor_body.initial_key() {
                return Err(CliError::KeyDoesNotMatchActor { actor: actor_id });
            }

            let nonce = generate_nonce().map_err(CliError::GenerateKey)?;
            let body = ObjectGenesisBody::new(
                ObjectKind::new(kind),
                actor_id.clone(),
                Timestamp::now(),
                nonce,
                initial_revision.map(RevisionId::new),
            );
            let object_id = body.object_id();

            let signature_bytes = secret.sign(&body.canonical_bytes());
            let signature = Signature::new(
                actor_id.clone(),
                secret.public_key().key_id().to_string(),
                "ed25519",
                signature_bytes.bytes().to_vec(),
            );
            let statement = ObjectGenesisStatement::new(body, signature);

            store
                .put_object_genesis(&statement)
                .map_err(|error| CliError::WriteObjectGenesis {
                    object: object_id.clone(),
                    source: error,
                })?;

            Ok(format!(
                "created object\nobject = {object_id}\ncreated_by = {actor_id}\nstore = {}\n",
                paths.store.display()
            ))
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

fn run_revision_command(command: RevisionCommand, paths: &StorePaths) -> Result<String, CliError> {
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
            json,
        } => {
            let statement = read_object_revision_statement(statement)?;
            let actor_genesis = read_actor_genesis(actor_genesis)?;
            let mut resolver = MemoryActorResolver::new();
            resolver.insert(actor_genesis);
            let report = verify_envelope_statement(&statement, &resolver);

            if json {
                Ok(format_verification_report_json(
                    statement.unsigned().body(),
                    &report,
                ))
            } else if report.is_cryptographically_valid() {
                Ok(format_verification_report(
                    statement.unsigned().body(),
                    &report,
                ))
            } else {
                Err(CliError::VerificationFailed(Box::new(report)))
            }
        }
        RevisionCommand::Create {
            actor,
            object,
            revision,
            manifest,
            parents,
            no_attests_reachable_history,
        } => {
            let actor_id = ActorId::new(actor.clone())
                .map_err(|source| CliError::ParseActorId { actor, source })?;
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;

            let store = open_store(paths)?;
            let keystore = open_keystore(paths)?;

            let actor_body = store
                .get_actor(&actor_id)
                .map_err(|error| CliError::ReadActor {
                    actor: actor_id.clone(),
                    source: error,
                })?;
            let secret =
                keystore
                    .get_signing_key(&actor_id)
                    .map_err(|error| CliError::ReadKey {
                        actor: actor_id.clone(),
                        source: error,
                    })?;
            if &secret.public_key() != actor_body.initial_key() {
                return Err(CliError::KeyDoesNotMatchActor { actor: actor_id });
            }

            let parsed_manifest = read_manifest(manifest)?;
            let manifest_hash = parsed_manifest.manifest_hash();

            if let Some(declared) = parsed_manifest.kairo().object() {
                if declared != &object_id {
                    return Err(CliError::ManifestObjectMismatch {
                        manifest_object: declared.clone(),
                        cli_object: object_id,
                    });
                }
            }

            let body = ObjectRevisionBody::new(
                object_id.clone(),
                RevisionId::new(revision),
                parents.into_iter().map(RevisionId::new).collect(),
                manifest_hash,
                !no_attests_reachable_history,
            );

            let subject: KairoRef = format!("object:{object_id}").parse().map_err(|source| {
                CliError::BuildSubjectRef {
                    object: object_id.clone(),
                    source,
                }
            })?;

            let unsigned =
                UnsignedStatement::new(actor_id.clone(), subject, Timestamp::now(), body);

            let signature_bytes = secret.sign(&unsigned.canonical_bytes());
            let signature = Signature::new(
                actor_id.clone(),
                secret.public_key().key_id().to_string(),
                "ed25519",
                signature_bytes.bytes().to_vec(),
            );
            let signed = SignedStatement::new(unsigned, signature);
            let statement_id = signed.statement_id();

            store
                .put_object_revision(&signed)
                .map_err(|error| CliError::WriteRevision {
                    statement: statement_id.clone(),
                    source: error,
                })?;

            Ok(format!(
                "created revision\nstatement = {statement_id}\nobject = {object_id}\nactor = {actor_id}\n"
            ))
        }
        RevisionCommand::Import { statement } => {
            let signed = read_object_revision_statement(statement)?;
            let store = open_store(paths)?;
            let statement_id =
                store
                    .put_object_revision(&signed)
                    .map_err(|error| CliError::WriteRevision {
                        statement: signed.statement_id(),
                        source: error,
                    })?;
            let body = signed.unsigned().body();
            Ok(format!(
                "imported revision\nstatement = {statement_id}\nobject = {}\nactor = {}\nstore = {}\n",
                body.object(),
                signed.unsigned().actor(),
                paths.store.display()
            ))
        }
        RevisionCommand::Inspect { statement, json } => {
            let statement_id = kairo_core::StatementId::new(statement.clone())
                .map_err(|source| CliError::ParseStatementId { statement, source })?;
            let store = open_store(paths)?;
            let signed = store.get_object_revision(&statement_id).map_err(|error| {
                CliError::ReadRevision {
                    statement: statement_id.clone(),
                    source: error,
                }
            })?;
            if json {
                Ok(format_revision_inspect_json(&signed))
            } else {
                Ok(format_revision_inspect(&signed))
            }
        }
        RevisionCommand::List { object } => {
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let store = open_store(paths)?;
            let revisions = list_object_revisions(&store, &object_id)?;
            Ok(format_revision_list(&object_id, &revisions))
        }
    }
}

fn run_branch_command(command: BranchCommand, paths: &StorePaths) -> Result<String, CliError> {
    match command {
        BranchCommand::Set {
            actor,
            object,
            revision,
            name,
        } => {
            let actor_id = ActorId::new(actor.clone())
                .map_err(|source| CliError::ParseActorId { actor, source })?;
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let revision_id = kairo_core::StatementId::new(revision.clone()).map_err(|source| {
                CliError::ParseStatementId {
                    statement: revision,
                    source,
                }
            })?;

            let store = open_store(paths)?;
            let keystore = open_keystore(paths)?;

            let actor_body = store
                .get_actor(&actor_id)
                .map_err(|error| CliError::ReadActor {
                    actor: actor_id.clone(),
                    source: error,
                })?;
            let secret =
                keystore
                    .get_signing_key(&actor_id)
                    .map_err(|error| CliError::ReadKey {
                        actor: actor_id.clone(),
                        source: error,
                    })?;
            if &secret.public_key() != actor_body.initial_key() {
                return Err(CliError::KeyDoesNotMatchActor { actor: actor_id });
            }

            // Confirm the revision exists locally and binds to the same
            // object — fail fast rather than leaving a dangling branch.
            let pointed = store.get_object_revision(&revision_id).map_err(|error| {
                CliError::ReadRevision {
                    statement: revision_id.clone(),
                    source: error,
                }
            })?;
            if pointed.unsigned().body().object() != &object_id {
                return Err(CliError::BranchObjectMismatch {
                    branch_object: object_id,
                    revision_object: pointed.unsigned().body().object().clone(),
                });
            }

            let body = ObjectBranchBody::new(object_id.clone(), name.clone(), revision_id.clone());
            let subject: KairoRef = format!("object:{object_id}").parse().map_err(|source| {
                CliError::BuildSubjectRef {
                    object: object_id.clone(),
                    source,
                }
            })?;
            let unsigned =
                UnsignedStatement::new(actor_id.clone(), subject, Timestamp::now(), body);
            let signature_bytes = secret.sign(&unsigned.canonical_bytes());
            let signature = Signature::new(
                actor_id.clone(),
                secret.public_key().key_id().to_string(),
                "ed25519",
                signature_bytes.bytes().to_vec(),
            );
            let signed = SignedStatement::new(unsigned, signature);
            let statement_id = signed.statement_id();

            store
                .put_object_branch(&signed)
                .map_err(|error| CliError::WriteBranch {
                    statement: statement_id.clone(),
                    source: error,
                })?;

            Ok(format!(
                "set branch\nstatement = {statement_id}\nobject = {object_id}\nactor = {actor_id}\nname = {name}\nrevision = {revision_id}\n"
            ))
        }
        BranchCommand::Show {
            object,
            actor,
            name,
            json,
        } => {
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let store = open_store(paths)?;

            let actor_id = match actor {
                Some(actor) => ActorId::new(actor.clone())
                    .map_err(|source| CliError::ParseActorId { actor, source })?,
                None => {
                    let genesis = store.get_object_genesis(&object_id).map_err(|error| {
                        CliError::ReadObjectGenesis {
                            object: object_id.clone(),
                            source: error,
                        }
                    })?;
                    genesis.body().created_by().clone()
                }
            };

            let resolved = store
                .latest_branch(&actor_id, &object_id, &name)
                .map_err(CliError::ReadBranch)?;

            match resolved {
                Some(signed) => {
                    if json {
                        Ok(format_branch_show_json(&signed))
                    } else {
                        Ok(format_branch_show(&signed))
                    }
                }
                None => Err(CliError::BranchNotFound {
                    actor: actor_id,
                    object: object_id,
                    name,
                }),
            }
        }
        BranchCommand::List { object } => {
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let store = open_store(paths)?;
            let tips = store
                .list_branches(&object_id)
                .map_err(CliError::ReadBranch)?;
            Ok(format_branch_list(&object_id, &tips))
        }
    }
}

fn format_branch_show(signed: &SignedStatement<ObjectBranchBody>) -> String {
    let body = signed.unsigned().body();
    format!(
        "statement = {}\nobject = {}\nactor = {}\nname = {}\nrevision = {}\ncreated_at = {}\n",
        signed.statement_id(),
        body.object(),
        signed.unsigned().actor(),
        body.name(),
        body.revision(),
        signed.unsigned().created_at()
    )
}

fn format_branch_show_json(signed: &SignedStatement<ObjectBranchBody>) -> String {
    let body = signed.unsigned().body();
    let value = serde_json::json!({
        "statement_id": signed.statement_id().to_string(),
        "actor": signed.unsigned().actor().to_string(),
        "object": body.object().to_string(),
        "name": body.name(),
        "revision": body.revision().to_string(),
        "created_at": signed.unsigned().created_at().to_string(),
    });
    let mut output = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    output.push('\n');
    output
}

fn format_branch_list(object: &ObjectId, tips: &[kairo_store::BranchTip]) -> String {
    let mut output = String::new();
    output.push_str(&format!("object = {object}\n"));
    output.push_str(&format!("branches = {}\n", tips.len()));
    for tip in tips {
        output.push_str(&format!(
            "  actor={} name={} statement={} created_at={}\n",
            tip.actor, tip.name, tip.statement_id, tip.created_at,
        ));
    }
    output
}

fn run_snapshot_command(command: SnapshotCommand, paths: &StorePaths) -> Result<String, CliError> {
    match command {
        SnapshotCommand::Compute {
            object,
            statement,
            actor,
            name,
            json,
        } => {
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let store = open_store(paths)?;

            let revision_statement = match statement {
                Some(statement) => {
                    let statement_id = kairo_core::StatementId::new(statement.clone())
                        .map_err(|source| CliError::ParseStatementId { statement, source })?;
                    store.get_object_revision(&statement_id).map_err(|error| {
                        CliError::ReadRevision {
                            statement: statement_id,
                            source: error,
                        }
                    })?
                }
                None => {
                    let actor_id = match actor {
                        Some(actor) => ActorId::new(actor.clone())
                            .map_err(|source| CliError::ParseActorId { actor, source })?,
                        None => {
                            let genesis =
                                store.get_object_genesis(&object_id).map_err(|error| {
                                    CliError::ReadObjectGenesis {
                                        object: object_id.clone(),
                                        source: error,
                                    }
                                })?;
                            genesis.body().created_by().clone()
                        }
                    };

                    let branch = store
                        .latest_branch(&actor_id, &object_id, &name)
                        .map_err(CliError::ReadBranch)?
                        .ok_or_else(|| CliError::BranchNotFound {
                            actor: actor_id.clone(),
                            object: object_id.clone(),
                            name: name.clone(),
                        })?;

                    let revision_statement_id = branch.unsigned().body().revision().clone();
                    store
                        .get_object_revision(&revision_statement_id)
                        .map_err(|error| CliError::ReadRevision {
                            statement: revision_statement_id,
                            source: error,
                        })?
                }
            };

            let snapshot = Snapshot::from_object_revision(&object_id, &revision_statement)
                .map_err(CliError::ComputeSnapshot)?;

            if json {
                Ok(format_snapshot_json(&snapshot))
            } else {
                Ok(format_snapshot(&snapshot))
            }
        }
    }
}

fn format_snapshot(snapshot: &Snapshot) -> String {
    let mut output = String::new();
    output.push_str(&format!("snapshot = {}\n", snapshot.snapshot_id()));
    output.push_str(&format!("object = {}\n", snapshot.object()));
    output.push_str(&format!("revision = {}\n", snapshot.revision().as_str()));
    output.push_str(&format!("manifest_hash = {}\n", snapshot.manifest_hash()));
    output.push_str(&format!("frontier = {}\n", snapshot.frontier().len()));
    for statement_id in snapshot.frontier() {
        output.push_str(&format!("  {statement_id}\n"));
    }
    output
}

fn format_snapshot_json(snapshot: &Snapshot) -> String {
    let value = serde_json::json!({
        "snapshot_id": snapshot.snapshot_id().to_string(),
        "object": snapshot.object().to_string(),
        "revision": snapshot.revision().as_str(),
        "manifest_hash": snapshot.manifest_hash().to_string(),
        "frontier": snapshot
            .frontier()
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>(),
    });
    let mut output = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    output.push('\n');
    output
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

fn read_object_genesis_statement(path: PathBuf) -> Result<ObjectGenesisStatement, CliError> {
    let input = std::fs::read_to_string(&path).map_err(|source| CliError::ReadStatement {
        path: path.clone(),
        source,
    })?;

    let dto: ObjectGenesisStatementJson =
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

fn list_object_revisions(
    store: &FilesystemStore,
    object: &ObjectId,
) -> Result<Vec<SignedStatement<ObjectRevisionBody>>, CliError> {
    let statements_dir = store.root().join("statements");
    let mut found = Vec::new();
    let level1 = match std::fs::read_dir(&statements_dir) {
        Ok(level1) => level1,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(error) => {
            return Err(CliError::ScanStatements {
                path: statements_dir,
                source: error,
            });
        }
    };
    for shard1 in level1 {
        let shard1 = shard1.map_err(|source| CliError::ScanStatements {
            path: statements_dir.clone(),
            source,
        })?;
        if !shard1.path().is_dir() {
            continue;
        }
        for shard2 in
            std::fs::read_dir(shard1.path()).map_err(|source| CliError::ScanStatements {
                path: shard1.path(),
                source,
            })?
        {
            let shard2 = shard2.map_err(|source| CliError::ScanStatements {
                path: shard1.path(),
                source,
            })?;
            if !shard2.path().is_dir() {
                continue;
            }
            for entry in
                std::fs::read_dir(shard2.path()).map_err(|source| CliError::ScanStatements {
                    path: shard2.path(),
                    source,
                })?
            {
                let entry = entry.map_err(|source| CliError::ScanStatements {
                    path: shard2.path(),
                    source,
                })?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                let bytes = std::fs::read(&path).map_err(|source| CliError::ScanStatements {
                    path: path.clone(),
                    source,
                })?;
                let dto: ObjectRevisionStatementJson =
                    serde_json::from_slice(&bytes).map_err(CliError::ParseStatementJson)?;
                let signed = dto.to_statement().map_err(CliError::ParseStatement)?;
                if signed.unsigned().body().object() == object {
                    found.push(signed);
                }
            }
        }
    }
    Ok(found)
}

fn format_revision_inspect(signed: &SignedStatement<ObjectRevisionBody>) -> String {
    let body = signed.unsigned().body();
    let mut output = String::new();
    output.push_str(&format!("statement = {}\n", signed.statement_id()));
    output.push_str(&format!("actor = {}\n", signed.unsigned().actor()));
    output.push_str(&format!(
        "created_at = {}\n",
        signed.unsigned().created_at()
    ));
    output.push_str(&format!("object = {}\n", body.object()));
    output.push_str(&format!("revision = {}\n", body.revision().as_str()));
    output.push_str(&format!("manifest_hash = {}\n", body.manifest_hash()));
    output.push_str(&format!(
        "attests_reachable_history = {}\n",
        body.attests_reachable_history()
    ));
    output.push_str(&format!("parents = {}\n", body.parents().len()));
    for parent in body.parents() {
        output.push_str(&format!("  parent {}\n", parent.as_str()));
    }
    output.push_str(&format!(
        "signature.key_id = {}\n",
        signed.signature().key_id()
    ));
    output.push_str(&format!(
        "signature.algorithm = {}\n",
        signed.signature().algorithm()
    ));
    output
}

fn format_revision_inspect_json(signed: &SignedStatement<ObjectRevisionBody>) -> String {
    let body = signed.unsigned().body();
    let value = serde_json::json!({
        "statement_id": signed.statement_id().to_string(),
        "actor": signed.unsigned().actor().to_string(),
        "created_at": signed.unsigned().created_at().to_string(),
        "object": body.object().to_string(),
        "revision": body.revision().as_str(),
        "parents": body.parents().iter().map(|p| p.as_str()).collect::<Vec<_>>(),
        "manifest_hash": body.manifest_hash().to_string(),
        "attests_reachable_history": body.attests_reachable_history(),
        "signature": {
            "actor": signed.signature().actor().to_string(),
            "key_id": signed.signature().key_id(),
            "algorithm": signed.signature().algorithm(),
        }
    });
    let mut output = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    output.push('\n');
    output
}

fn format_revision_list(
    object: &ObjectId,
    revisions: &[SignedStatement<ObjectRevisionBody>],
) -> String {
    let mut output = String::new();
    output.push_str(&format!("object = {object}\n"));
    output.push_str(&format!("revisions = {}\n", revisions.len()));
    for signed in revisions {
        let body = signed.unsigned().body();
        output.push_str(&format!(
            "  {} revision={} actor={}\n",
            signed.statement_id(),
            body.revision().as_str(),
            signed.unsigned().actor()
        ));
    }
    output
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

fn format_verification_report_json(
    revision: &ObjectRevisionBody,
    report: &VerificationReport,
) -> String {
    let mut signature = serde_json::Map::new();
    signature.insert(
        "status".to_owned(),
        serde_json::Value::String(format_signature_status(&report.signature).to_owned()),
    );
    match &report.signature {
        SignatureStatus::UnsupportedAlgorithm(algorithm) => {
            signature.insert(
                "algorithm".to_owned(),
                serde_json::Value::String(algorithm.clone()),
            );
        }
        SignatureStatus::Malformed {
            expected_len,
            actual_len,
        } => {
            signature.insert(
                "expected_len".to_owned(),
                serde_json::Value::Number((*expected_len).into()),
            );
            signature.insert(
                "actual_len".to_owned(),
                serde_json::Value::Number((*actual_len).into()),
            );
        }
        _ => {}
    }

    let mut actor = serde_json::Map::new();
    actor.insert(
        "status".to_owned(),
        serde_json::Value::String(format_actor_resolution(&report.actor).to_owned()),
    );
    if let ActorResolution::ResolverUnavailable(reason) = &report.actor {
        actor.insert(
            "reason".to_owned(),
            serde_json::Value::String(reason.clone()),
        );
    }

    let value = serde_json::json!({
        "statement_id": report.statement_id.to_string(),
        "envelope_actor": report.envelope_actor.to_string(),
        "signature_actor": report.signature_actor.to_string(),
        "object": revision.object().to_string(),
        "revision": revision.revision().as_str(),
        "signature": serde_json::Value::Object(signature),
        "actor": serde_json::Value::Object(actor),
        "trust": format_trust(&report.trust),
        "cryptographically_valid": report.is_cryptographically_valid(),
    });
    let mut output = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    output.push('\n');
    output
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
    "kairo\n\nUsage:\n  kairo [--store <path>] [--keys <path>] <command>\n\nCommands:\n  kairo actor id --genesis <path>\n  kairo actor create --kind <kind>\n  kairo actor import --genesis <path>\n  kairo manifest hash [path]\n  kairo manifest inspect [path]\n  kairo object create --actor <id> --kind <kind> [--initial-revision <ref>]\n  kairo object import --statement <path>\n  kairo revision create --actor <id> --object <id> --revision <ref> [--manifest <path>] [--parent <ref>]... [--no-attests-reachable-history]\n  kairo revision import --statement <path>\n  kairo revision inspect --statement <id> [--json]\n  kairo revision list --object <id>\n  kairo revision validate-manifest --statement <path> [--manifest <path>]\n  kairo revision verify-signature --statement <path> (--public-key <base64>|--public-key-file <path>)\n  kairo revision verify-actor-genesis --statement <path> --actor-genesis <path> [--json]\n  kairo branch set --actor <id> --object <id> --revision <statement-id> [--name <name>]\n  kairo branch show --object <id> [--actor <id>] [--name <name>] [--json]\n  kairo branch list --object <id>\n  kairo snapshot compute --object <id> [--actor <id>] [--name <name>] [--statement <id>] [--json]\n".to_owned()
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
    HomeNotSet,
    OpenStore {
        path: PathBuf,
        source: kairo_store::StoreError,
    },
    OpenKeystore {
        path: PathBuf,
        source: kairo_keystore::KeystoreError,
    },
    GenerateKey(kairo_identity::KeyGenerationError),
    WriteKey {
        actor: ActorId,
        source: kairo_keystore::KeystoreError,
    },
    WriteActor {
        actor: ActorId,
        source: kairo_store::StoreError,
    },
    ReadActor {
        actor: ActorId,
        source: kairo_store::StoreError,
    },
    ReadKey {
        actor: ActorId,
        source: kairo_keystore::KeystoreError,
    },
    KeyDoesNotMatchActor {
        actor: ActorId,
    },
    ParseActorId {
        actor: String,
        source: kairo_core::IdError,
    },
    ParseObjectId {
        object: String,
        source: kairo_core::IdError,
    },
    WriteObjectGenesis {
        object: ObjectId,
        source: kairo_store::StoreError,
    },
    WriteRevision {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    ReadRevision {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    ParseStatementId {
        statement: String,
        source: kairo_core::IdError,
    },
    ScanStatements {
        path: PathBuf,
        source: std::io::Error,
    },
    WriteBranch {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    ReadBranch(kairo_store::StoreError),
    ReadObjectGenesis {
        object: ObjectId,
        source: kairo_store::StoreError,
    },
    BranchObjectMismatch {
        branch_object: ObjectId,
        revision_object: ObjectId,
    },
    BranchNotFound {
        actor: ActorId,
        object: ObjectId,
        name: String,
    },
    ComputeSnapshot(SnapshotError),
    ManifestObjectMismatch {
        manifest_object: ObjectId,
        cli_object: ObjectId,
    },
    BuildSubjectRef {
        object: ObjectId,
        source: kairo_core::IdError,
    },
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
            Self::HomeNotSet => f.write_str(
                "HOME environment variable is not set; use --store to specify a store root",
            ),
            Self::OpenStore { path, source } => {
                write!(f, "failed to open store at {}: {source}", path.display())
            }
            Self::OpenKeystore { path, source } => {
                write!(f, "failed to open keystore at {}: {source}", path.display())
            }
            Self::GenerateKey(error) => write!(f, "{error}"),
            Self::WriteKey { actor, source } => {
                write!(f, "failed to write key for actor {actor}: {source}")
            }
            Self::WriteActor { actor, source } => {
                write!(f, "failed to write actor {actor}: {source}")
            }
            Self::ReadActor { actor, source } => {
                write!(f, "failed to read actor {actor}: {source}")
            }
            Self::ReadKey { actor, source } => {
                write!(f, "failed to read key for actor {actor}: {source}")
            }
            Self::KeyDoesNotMatchActor { actor } => write!(
                f,
                "stored key for actor {actor} does not match the actor's initial public key"
            ),
            Self::ParseActorId { actor, source } => {
                write!(f, "invalid actor id {actor}: {source}")
            }
            Self::ParseObjectId { object, source } => {
                write!(f, "invalid object id {object}: {source}")
            }
            Self::WriteObjectGenesis { object, source } => {
                write!(f, "failed to write object genesis {object}: {source}")
            }
            Self::WriteRevision { statement, source } => {
                write!(
                    f,
                    "failed to write revision statement {statement}: {source}"
                )
            }
            Self::ReadRevision { statement, source } => {
                write!(f, "failed to read revision statement {statement}: {source}")
            }
            Self::ParseStatementId { statement, source } => {
                write!(f, "invalid statement id {statement}: {source}")
            }
            Self::ScanStatements { path, source } => {
                write!(
                    f,
                    "failed to scan statements at {}: {source}",
                    path.display()
                )
            }
            Self::WriteBranch { statement, source } => {
                write!(f, "failed to write branch statement {statement}: {source}")
            }
            Self::ReadBranch(error) => write!(f, "failed to read branch: {error}"),
            Self::ReadObjectGenesis { object, source } => {
                write!(f, "failed to read object genesis {object}: {source}")
            }
            Self::BranchObjectMismatch {
                branch_object,
                revision_object,
            } => write!(
                f,
                "branch declares object {branch_object} but the pointed-at revision binds to {revision_object}"
            ),
            Self::BranchNotFound { actor, object, name } => write!(
                f,
                "no branch named {name} for actor {actor} on object {object}"
            ),
            Self::ComputeSnapshot(error) => write!(f, "{error}"),
            Self::ManifestObjectMismatch {
                manifest_object,
                cli_object,
            } => write!(
                f,
                "manifest declares object {manifest_object} but --object is {cli_object}"
            ),
            Self::BuildSubjectRef { object, source } => {
                write!(
                    f,
                    "could not build subject reference for object {object}: {source}"
                )
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
            | Self::ReadActorGenesis { source, .. }
            | Self::ScanStatements { source, .. } => Some(source),
            Self::ParseManifest(error) => Some(error),
            Self::ParseActorGenesisJson(error) | Self::ParseStatementJson(error) => Some(error),
            Self::ParseActorGenesis(error) => Some(error),
            Self::ParseStatement(error) => Some(error),
            Self::ValidateRevisionManifest(error) => Some(error),
            Self::VerifyStatementSignature(error) => Some(error),
            Self::OpenStore { source, .. }
            | Self::WriteActor { source, .. }
            | Self::ReadActor { source, .. }
            | Self::WriteObjectGenesis { source, .. }
            | Self::WriteRevision { source, .. }
            | Self::ReadRevision { source, .. }
            | Self::WriteBranch { source, .. }
            | Self::ReadObjectGenesis { source, .. } => Some(source),
            Self::ReadBranch(error) => Some(error),
            Self::ComputeSnapshot(error) => Some(error),
            Self::OpenKeystore { source, .. }
            | Self::WriteKey { source, .. }
            | Self::ReadKey { source, .. } => Some(source),
            Self::ParseActorId { source, .. }
            | Self::ParseObjectId { source, .. }
            | Self::ParseStatementId { source, .. }
            | Self::BuildSubjectRef { source, .. } => Some(source),
            Self::GenerateKey(error) => Some(error),
            Self::VerificationFailed(_)
            | Self::HomeNotSet
            | Self::KeyDoesNotMatchActor { .. }
            | Self::ManifestObjectMismatch { .. }
            | Self::BranchObjectMismatch { .. }
            | Self::BranchNotFound { .. }
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
            Ok(Cli { store: None, keys: None,
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
            Ok(Cli { store: None, keys: None,
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
            Ok(Cli { store: None, keys: None,
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
            Ok(Cli { store: None, keys: None,
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
            Ok(Cli { store: None, keys: None,
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
            Ok(Cli { store: None, keys: None,
                command: Some(Command::Revision {
                    command: RevisionCommand::VerifyActorGenesis {
                        statement,
                        actor_genesis,
                        json: false,
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

    #[test]
    fn end_to_end_actor_object_revision_against_tempdir() -> Result<(), Box<dyn std::error::Error>>
    {
        let store_dir = tempfile::TempDir::new()?;
        let manifest_dir = tempfile::TempDir::new()?;
        let manifest_path = manifest_dir.path().join("kairo.toml");
        let bare_manifest = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [content]
            kind = "tree"
        "#;
        std::fs::write(&manifest_path, bare_manifest)?;

        // 1. Create a fresh actor.
        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;

        // 2. Create an object lineage.
        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id.clone(),
                    kind: "software".to_owned(),
                    initial_revision: Some("git:sha256:abc".to_owned()),
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;

        // 3. Create a signed revision pointing at that object.
        let revision_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: "git:sha256:def".to_owned(),
                    manifest: manifest_path.clone(),
                    parents: vec!["git:sha256:abc".to_owned()],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        assert!(revision_output.contains("created revision"));
        assert!(revision_output.contains(&actor_id));
        assert!(revision_output.contains(&object_id));

        // 4. Read back from the store and verify the signature against the
        //    persisted ActorGenesis through the generic verifier.
        use kairo_statement::verify::ActorResolution;
        let store = open_store(&StorePaths {
            store: store_dir.path().to_path_buf(),
            keys: store_dir.path().join("keys"),
        })?;
        let actor_id_typed = ActorId::new(actor_id)?;
        let _genesis = store.get_actor(&actor_id_typed)?;

        // The revision should be readable by its statement id (we don't have
        // direct access to it here, but the parse_field above pinned the
        // round-trip to a successful write).
        let signed =
            first_statement_on_disk(store_dir.path())?.ok_or("no revision statement on disk")?;
        let report = kairo_statement::verify::verify_envelope_statement(&signed, &store);
        assert_eq!(report.actor, ActorResolution::Resolved);
        assert!(report.is_cryptographically_valid());

        Ok(())
    }

    fn first_statement_on_disk(
        store_root: &std::path::Path,
    ) -> Result<Option<SignedStatement<ObjectRevisionBody>>, Box<dyn std::error::Error>> {
        let statements_dir = store_root.join("statements");
        for level1 in std::fs::read_dir(&statements_dir)? {
            let level1 = level1?;
            for level2 in std::fs::read_dir(level1.path())? {
                let level2 = level2?;
                if let Some(entry) = std::fs::read_dir(level2.path())?.next() {
                    let path = entry?.path();
                    let json: ObjectRevisionStatementJson =
                        serde_json::from_slice(&std::fs::read(&path)?)?;
                    let signed = json.to_statement().map_err(|error| error.to_string())?;
                    return Ok(Some(signed));
                }
            }
        }
        Ok(None)
    }

    fn parse_field(text: &str, prefix: &str) -> Result<String, Box<dyn std::error::Error>> {
        text.lines()
            .find_map(|line| line.strip_prefix(prefix).map(str::to_owned))
            .ok_or_else(|| format!("missing field {prefix:?} in {text:?}").into())
    }

    #[test]
    fn parses_actor_import_command() {
        let cli = Cli::try_parse_from(["kairo", "actor", "import", "--genesis", "actor.json"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Import { genesis }
                })
            }) if genesis.as_os_str() == "actor.json"
        ));
    }

    #[test]
    fn parses_object_import_command() {
        let cli = Cli::try_parse_from(["kairo", "object", "import", "--statement", "obj.json"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Object {
                    command: ObjectSubcommand::Import { statement }
                })
            }) if statement.as_os_str() == "obj.json"
        ));
    }

    #[test]
    fn parses_revision_import_command() {
        let cli = Cli::try_parse_from(["kairo", "revision", "import", "--statement", "r.json"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Revision {
                    command: RevisionCommand::Import { statement }
                })
            }) if statement.as_os_str() == "r.json"
        ));
    }

    #[test]
    fn parses_revision_inspect_command() {
        let cli = Cli::try_parse_from([
            "kairo",
            "revision",
            "inspect",
            "--statement",
            "zQmStatement",
            "--json",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Revision {
                    command: RevisionCommand::Inspect {
                        statement,
                        json: true,
                    }
                })
            }) if statement == "zQmStatement"
        ));
    }

    #[test]
    fn parses_revision_list_command() {
        let cli = Cli::try_parse_from(["kairo", "revision", "list", "--object", "zQmObject"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Revision {
                    command: RevisionCommand::List { object }
                })
            }) if object == "zQmObject"
        ));
    }

    #[test]
    fn parses_revision_verify_actor_genesis_with_json() {
        let cli = Cli::try_parse_from([
            "kairo",
            "revision",
            "verify-actor-genesis",
            "--statement",
            "r.json",
            "--actor-genesis",
            "a.json",
            "--json",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Revision {
                    command: RevisionCommand::VerifyActorGenesis {
                        statement: _,
                        actor_genesis: _,
                        json: true,
                    }
                })
            })
        ));
    }

    #[test]
    fn end_to_end_import_inspect_list() -> Result<(), Box<dyn std::error::Error>> {
        // Drive the create flow into a temp store, then use the import / inspect /
        // list commands to round-trip.
        let store_dir = tempfile::TempDir::new()?;
        let other_dir = tempfile::TempDir::new()?;
        let manifest_dir = tempfile::TempDir::new()?;
        let manifest_path = manifest_dir.path().join("kairo.toml");
        let bare_manifest = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [content]
            kind = "tree"
        "#;
        std::fs::write(&manifest_path, bare_manifest)?;

        // 1. Create actor + object + revision in store_dir.
        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;
        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id.clone(),
                    kind: "software".to_owned(),
                    initial_revision: None,
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;
        let revision_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: "git:sha256:def".to_owned(),
                    manifest: manifest_path.clone(),
                    parents: vec![],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let statement_id = parse_field(&revision_output, "statement = ")?;

        // 2. Re-find the on-disk JSONs in store_dir's actors/objects/statements
        //    directories so we can re-import them into a fresh store.
        let actor_json = find_one(&store_dir.path().join("actors"), "json")?;
        let object_json = find_one(&store_dir.path().join("objects"), "json")?;
        let statement_json = find_one(&store_dir.path().join("statements"), "json")?;

        // 3. Import them all into a fresh store via the CLI.
        let imported_actor = run(Cli {
            store: Some(other_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Import {
                    genesis: actor_json,
                },
            }),
        })?;
        assert!(imported_actor.contains("imported actor"));
        assert!(imported_actor.contains(&actor_id));

        let imported_object = run(Cli {
            store: Some(other_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Import {
                    statement: object_json,
                },
            }),
        })?;
        assert!(imported_object.contains("imported object genesis"));
        assert!(imported_object.contains(&object_id));

        let imported_revision = run(Cli {
            store: Some(other_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Import {
                    statement: statement_json,
                },
            }),
        })?;
        assert!(imported_revision.contains("imported revision"));
        assert!(imported_revision.contains(&statement_id));

        // 4. Inspect the revision in the new store.
        let inspect_text = run(Cli {
            store: Some(other_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Inspect {
                    statement: statement_id.clone(),
                    json: false,
                },
            }),
        })?;
        assert!(inspect_text.contains(&statement_id));
        assert!(inspect_text.contains(&object_id));
        assert!(inspect_text.contains("revision = git:sha256:def"));

        let inspect_json = run(Cli {
            store: Some(other_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Inspect {
                    statement: statement_id.clone(),
                    json: true,
                },
            }),
        })?;
        let parsed: serde_json::Value = serde_json::from_str(&inspect_json)?;
        assert_eq!(parsed["statement_id"], statement_id);
        assert_eq!(parsed["object"], object_id);
        assert_eq!(parsed["revision"], "git:sha256:def");

        // 5. List revisions for that object.
        let list_text = run(Cli {
            store: Some(other_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::List {
                    object: object_id.clone(),
                },
            }),
        })?;
        assert!(list_text.contains("revisions = 1"));
        assert!(list_text.contains(&statement_id));

        Ok(())
    }

    fn find_one(
        root: &std::path::Path,
        extension: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        for level1 in std::fs::read_dir(root)? {
            let level1 = level1?;
            if !level1.path().is_dir() {
                continue;
            }
            for level2 in std::fs::read_dir(level1.path())? {
                let level2 = level2?;
                if !level2.path().is_dir() {
                    continue;
                }
                if let Some(entry) = std::fs::read_dir(level2.path())?.next() {
                    let path = entry?.path();
                    if path.extension().and_then(|s| s.to_str()) == Some(extension) {
                        return Ok(path);
                    }
                }
            }
        }
        Err(format!("no {extension} file found under {}", root.display()).into())
    }

    #[test]
    fn parses_branch_set_default_name() {
        let cli = Cli::try_parse_from([
            "kairo",
            "branch",
            "set",
            "--actor",
            "zActor",
            "--object",
            "zObject",
            "--revision",
            "zRev",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Branch {
                    command: BranchCommand::Set { name, .. }
                })
            }) if name == "head"
        ));
    }

    #[test]
    fn parses_branch_set_with_explicit_name() {
        let cli = Cli::try_parse_from([
            "kairo",
            "branch",
            "set",
            "--actor",
            "zActor",
            "--object",
            "zObject",
            "--revision",
            "zRev",
            "--name",
            "release",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Branch {
                    command: BranchCommand::Set { name, .. }
                })
            }) if name == "release"
        ));
    }

    #[test]
    fn parses_branch_show_defaults_to_head() {
        let cli = Cli::try_parse_from(["kairo", "branch", "show", "--object", "zObject"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Branch {
                    command: BranchCommand::Show {
                        actor: None,
                        name,
                        json: false,
                        ..
                    }
                })
            }) if name == "head"
        ));
    }

    #[test]
    fn parses_branch_list_command() {
        let cli = Cli::try_parse_from(["kairo", "branch", "list", "--object", "zObject"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Branch {
                    command: BranchCommand::List { object }
                })
            }) if object == "zObject"
        ));
    }

    #[test]
    fn end_to_end_branch_set_show_list() -> Result<(), Box<dyn std::error::Error>> {
        // Drive create + revision into a temp store, then exercise branch
        // set / show / list and prove that supersession moves the index.
        let store_dir = tempfile::TempDir::new()?;
        let manifest_dir = tempfile::TempDir::new()?;
        let manifest_path = manifest_dir.path().join("kairo.toml");
        std::fs::write(
            &manifest_path,
            r#"[kairo]
schema = 1
kind = "software"
name = "Example"

[content]
kind = "tree"
"#,
        )?;

        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;
        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id.clone(),
                    kind: "software".to_owned(),
                    initial_revision: None,
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;

        // Two revisions on the same object so we can supersede a branch tip.
        let r1 = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: "git:sha256:r1".to_owned(),
                    manifest: manifest_path.clone(),
                    parents: vec![],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let r1_statement = parse_field(&r1, "statement = ")?;

        // Force a strictly greater created_at by pausing briefly. Timestamp
        // resolution is whole seconds, so wait one full second to guarantee
        // strict supersession.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let r2 = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: "git:sha256:r2".to_owned(),
                    manifest: manifest_path.clone(),
                    parents: vec!["git:sha256:r1".to_owned()],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let r2_statement = parse_field(&r2, "statement = ")?;
        assert_ne!(r1_statement, r2_statement);

        // Set head to r1.
        let set_r1 = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: r1_statement.clone(),
                    name: "head".to_owned(),
                },
            }),
        })?;
        assert!(set_r1.contains("set branch"));
        assert!(set_r1.contains(&r1_statement));

        // Show should currently return r1.
        let show_r1 = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Show {
                    object: object_id.clone(),
                    actor: None,
                    name: "head".to_owned(),
                    json: false,
                },
            }),
        })?;
        assert!(show_r1.contains(&r1_statement));

        // Pause again so the supersession ordering is unambiguous at
        // whole-second granularity.
        std::thread::sleep(std::time::Duration::from_millis(1100));

        // Advance head to r2.
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: r2_statement.clone(),
                    name: "head".to_owned(),
                },
            }),
        })?;

        // Show should now return r2.
        let show_r2 = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Show {
                    object: object_id.clone(),
                    actor: None,
                    name: "head".to_owned(),
                    json: true,
                },
            }),
        })?;
        let parsed: serde_json::Value = serde_json::from_str(&show_r2)?;
        assert_eq!(parsed["revision"], r2_statement);

        // List should report exactly one branch tip for the object.
        let list_text = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::List {
                    object: object_id.clone(),
                },
            }),
        })?;
        assert!(list_text.contains("branches = 1"));
        assert!(list_text.contains("name=head"));

        Ok(())
    }

    #[test]
    fn branch_set_rejects_revision_for_wrong_object() -> Result<(), Box<dyn std::error::Error>> {
        // Set up two distinct objects and try to point object A's branch at
        // a revision that binds to object B. The branch set command must
        // fail rather than create a dangling pointer.
        let store_dir = tempfile::TempDir::new()?;
        let manifest_dir = tempfile::TempDir::new()?;
        let manifest_path = manifest_dir.path().join("kairo.toml");
        std::fs::write(
            &manifest_path,
            r#"[kairo]
schema = 1
kind = "software"
name = "Example"

[content]
kind = "tree"
"#,
        )?;

        let actor_id = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Create {
                        kind: "person".to_owned(),
                    },
                }),
            })?,
            "actor = ",
        )?;

        let object_a = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Object {
                    command: ObjectSubcommand::Create {
                        actor: actor_id.clone(),
                        kind: "software".to_owned(),
                        initial_revision: None,
                    },
                }),
            })?,
            "object = ",
        )?;
        let object_b = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Object {
                    command: ObjectSubcommand::Create {
                        actor: actor_id.clone(),
                        kind: "software".to_owned(),
                        initial_revision: Some("git:sha256:bootstrap".to_owned()),
                    },
                }),
            })?,
            "object = ",
        )?;
        assert_ne!(object_a, object_b);

        let r_b = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_b.clone(),
                    revision: "git:sha256:rb".to_owned(),
                    manifest: manifest_path.clone(),
                    parents: vec![],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let r_b_statement = parse_field(&r_b, "statement = ")?;

        let result = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id,
                    object: object_a,
                    revision: r_b_statement,
                    name: "head".to_owned(),
                },
            }),
        });

        assert!(matches!(result, Err(CliError::BranchObjectMismatch { .. })));
        Ok(())
    }

    #[test]
    fn parses_snapshot_compute_defaults() {
        let cli = Cli::try_parse_from(["kairo", "snapshot", "compute", "--object", "zObject"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Snapshot {
                    command: SnapshotCommand::Compute {
                        statement: None,
                        actor: None,
                        name,
                        json: false,
                        ..
                    }
                })
            }) if name == "head"
        ));
    }

    #[test]
    fn parses_snapshot_compute_with_pinned_statement() {
        let cli = Cli::try_parse_from([
            "kairo",
            "snapshot",
            "compute",
            "--object",
            "zObject",
            "--statement",
            "zStatement",
            "--json",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Snapshot {
                    command: SnapshotCommand::Compute {
                        statement: Some(stmt),
                        json: true,
                        ..
                    }
                })
            }) if stmt == "zStatement"
        ));
    }

    #[test]
    fn snapshot_compute_with_pinned_statement_and_actor_conflicts() {
        // --statement conflicts with --actor and --name (which would
        // otherwise route through branch resolution).
        let cli = Cli::try_parse_from([
            "kairo",
            "snapshot",
            "compute",
            "--object",
            "zObject",
            "--statement",
            "zStatement",
            "--actor",
            "zActor",
        ]);

        assert!(cli.is_err());
    }

    #[test]
    fn end_to_end_snapshot_via_branch() -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let manifest_dir = tempfile::TempDir::new()?;
        let manifest_path = manifest_dir.path().join("kairo.toml");
        std::fs::write(
            &manifest_path,
            r#"[kairo]
schema = 1
kind = "software"
name = "Example"

[content]
kind = "tree"
"#,
        )?;

        let actor_id = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Create {
                        kind: "person".to_owned(),
                    },
                }),
            })?,
            "actor = ",
        )?;
        let object_id = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Object {
                    command: ObjectSubcommand::Create {
                        actor: actor_id.clone(),
                        kind: "software".to_owned(),
                        initial_revision: None,
                    },
                }),
            })?,
            "object = ",
        )?;
        let revision_statement = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Revision {
                    command: RevisionCommand::Create {
                        actor: actor_id.clone(),
                        object: object_id.clone(),
                        revision: "git:sha256:def".to_owned(),
                        manifest: manifest_path.clone(),
                        parents: vec![],
                        no_attests_reachable_history: false,
                    },
                }),
            })?,
            "statement = ",
        )?;

        // No branch set yet — snapshot must fail with BranchNotFound.
        let no_branch = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Snapshot {
                command: SnapshotCommand::Compute {
                    object: object_id.clone(),
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    json: false,
                },
            }),
        });
        assert!(matches!(no_branch, Err(CliError::BranchNotFound { .. })));

        // Pinning the statement directly should work without a branch.
        let pinned = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Snapshot {
                command: SnapshotCommand::Compute {
                    object: object_id.clone(),
                    statement: Some(revision_statement.clone()),
                    actor: None,
                    name: "head".to_owned(),
                    json: true,
                },
            }),
        })?;
        let pinned_json: serde_json::Value = serde_json::from_str(&pinned)?;
        assert_eq!(pinned_json["object"], object_id);
        assert_eq!(pinned_json["revision"], "git:sha256:def");
        let pinned_id = pinned_json["snapshot_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        assert!(pinned_id.starts_with('z'));

        // Set head to point at the revision so default-resolution works too.
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: revision_statement.clone(),
                    name: "head".to_owned(),
                },
            }),
        })?;

        // Default-resolved snapshot should produce the same id as pinning.
        let resolved = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Snapshot {
                command: SnapshotCommand::Compute {
                    object: object_id.clone(),
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    json: true,
                },
            }),
        })?;
        let resolved_json: serde_json::Value = serde_json::from_str(&resolved)?;
        assert_eq!(
            resolved_json["snapshot_id"].as_str(),
            Some(pinned_id.as_str())
        );

        // Human-readable form contains the snapshot id and frontier.
        let human = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Snapshot {
                command: SnapshotCommand::Compute {
                    object: object_id,
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    json: false,
                },
            }),
        })?;
        assert!(human.contains(&pinned_id));
        assert!(human.contains("revision = git:sha256:def"));
        assert!(human.contains("frontier = 1"));
        assert!(human.contains(&revision_statement));

        Ok(())
    }
}
