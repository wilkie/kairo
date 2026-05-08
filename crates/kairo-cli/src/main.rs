mod cli;
mod commands;
mod error;
mod format;
mod store_paths;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;

use cli::{
    ActorCommand, AddAttestationKeyCommand, BranchCommand, CapabilityCommand,
    ChangeAttestationThresholdCommand, Cli, Command, ObjectSubcommand, RecoverKeyCommand,
    RevisionCommand, RevokeAttestationKeyCommand, SnapshotCommand, TagCommand, TrustCommand,
    VerifyCommand,
};
#[cfg(test)]
use cli::{BundleCommand, GitCacheCommand, GitCommand, ManifestCommand};
use error::CliError;
use format::{
    format_actor_resolution, format_signature_status, format_trust, format_verification_report,
    format_verification_report_json,
};
use store_paths::{open_keystore, open_store, StorePaths};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use kairo_core::canonical::CanonicalEncode;
use kairo_core::{ActorId, KairoRef, ObjectId, Timestamp};
use kairo_identity::json::ActorGenesisJson;
use kairo_identity::{
    generate_nonce, ActorGenesisBody, ActorKind, ActorResolver, KeyId, MemoryActorResolver,
    PublicKey, SecretSigningKey,
};
use kairo_keystore::{FilesystemKeystore, Keystore};
use kairo_object::{
    validate_object_revision, validate_revision_manifest, CommitLookup, ContentLayerCheck,
    ManifestBindingCheck, ObjectConsistencyCheck, ObjectManifest, ObjectRevisionValidationReport,
    ParentReferenceCheck, Snapshot,
};
use kairo_statement::json::{
    ActorAttestationKeyAddStatementJson, ActorEmergencyKeyRotationStatementJson,
    ObjectGenesisStatementJson, ObjectRevisionStatementJson,
};
use kairo_statement::verify::{
    verify_envelope_statement, ActorResolution, SignatureStatus, VerificationReport,
};
use kairo_statement::{
    ActorAttestationKeyAddBody, ActorCapabilityGrantBody, ActorCapabilityRevocationBody,
    ActorEmergencyKeyRotationBody, ActorKeyRevocationBody, ActorKeyRotationBody, ActorTrustBody,
    Capability, CapabilityConstraint, CapabilityScope, MultiSignedStatement, ObjectBranchBody,
    ObjectGenesisBody, ObjectGenesisStatement, ObjectKind, ObjectRevisionBody, ObjectVersionTagBody,
    RevisionId, SemverVersion, Signature, SignedStatement, StatementKind, TrustDecision,
    UnsignedStatement,
};
use kairo_store::{
    ActorStore, BlobStore, BranchResolver, CapabilityHead, CapabilityResolver, FilesystemStore,
    ObjectStore, StatementStore, TrustResolver, VersionTagResolver,
};


fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}

/// Resolve the actor's currently active signing key at `now()` and
/// confirm the keystore holds the matching secret. Replaces the
/// previous "match keystore against `actor_body.initial_key()`"
/// pattern, which broke after rotation.
fn require_active_signing_key(
    store: &FilesystemStore,
    keystore: &FilesystemKeystore,
    actor_id: &ActorId,
) -> Result<SecretSigningKey, CliError> {
    let secret = keystore
        .get_signing_key(actor_id)
        .map_err(|error| CliError::ReadKey {
            actor: actor_id.clone(),
            source: error,
        })?;
    let active_key = ActorResolver::active_key_at(store, actor_id, Timestamp::now())
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?
        .ok_or_else(|| CliError::ActorHasNoActiveKey {
            actor: actor_id.clone(),
        })?;
    if secret.public_key() != active_key {
        return Err(CliError::KeyDoesNotMatchActor {
            actor: actor_id.clone(),
        });
    }
    Ok(secret)
}

fn run(cli: Cli) -> Result<String, CliError> {
    let require_daemon = cli.daemon;
    // `--direct` and `--offline` are parsed for forward compat
    // (slice 8 plumbs them into read-command dispatch); accepted
    // here as no-ops in slice 4 except for the clap-enforced
    // mutual-exclusion with `--daemon`.
    let _ = (cli.direct, cli.offline);

    let paths = StorePaths::resolve(cli.store, cli.keys)?;
    match cli.command {
        Some(Command::Actor { command }) => run_actor_command(command, &paths),
        Some(Command::Manifest { command }) => commands::manifest::run_manifest_command(command),
        Some(Command::Object { command }) => run_object_command(command, &paths),
        Some(Command::Revision { command }) => run_revision_command(command, &paths),
        Some(Command::Branch { command }) => run_branch_command(command, &paths),
        Some(Command::Tag { command }) => run_tag_command(command, &paths),
        Some(Command::Trust { command }) => run_trust_command(command, &paths),
        Some(Command::Capability { command }) => run_capability_command(command, &paths),
        Some(Command::Bundle { command }) => commands::bundle::run_bundle_command(command, &paths),
        Some(Command::Snapshot { command }) => run_snapshot_command(command, &paths),
        Some(Command::Verify { command }) => run_verify_command(command, &paths),
        Some(Command::Git { command }) => commands::git::run_git_command(command, &paths),
        Some(Command::Daemon { command }) => {
            commands::daemon::run_daemon_command(command, &paths, require_daemon)
        }
        None => Ok(help_text()),
    }
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
        ActorCommand::Create {
            kind,
            attestation_keys,
            generate_attestation_keys,
            attestation_threshold,
        } => run_actor_create(
            paths,
            kind,
            attestation_keys,
            generate_attestation_keys,
            attestation_threshold,
        ),
        ActorCommand::RotateKey { actor } => run_actor_rotate_key(paths, actor),
        ActorCommand::RevokeKey {
            actor,
            key_id,
            retroactive,
            reason,
            brick_actor,
        } => run_actor_revoke_key(paths, actor, key_id, retroactive, reason, brick_actor),
        ActorCommand::KeyHistory { actor, json } => run_actor_key_history(paths, actor, json),
        ActorCommand::RecoverKey { command } => run_actor_recover_key(paths, command),
        ActorCommand::AddAttestationKey { command } => {
            run_actor_add_attestation_key(paths, command)
        }
        ActorCommand::RevokeAttestationKey { command } => {
            run_actor_revoke_attestation_key(paths, command)
        }
        ActorCommand::ChangeAttestationThreshold { command } => {
            run_actor_change_attestation_threshold(paths, command)
        }
        ActorCommand::CoSign {
            prepared,
            actor,
            attestation_key_seed,
        } => run_actor_cosign(paths, prepared, actor, attestation_key_seed),
    }
}

fn run_actor_create(
    paths: &StorePaths,
    kind: String,
    attestation_keys_hex: Vec<String>,
    generate_attestation_keys: u8,
    attestation_threshold: u8,
) -> Result<String, CliError> {
    if attestation_keys_hex.is_empty() && generate_attestation_keys == 0 {
        return Err(CliError::NoAttestationKeyProvided);
    }

    let store = open_store(paths)?;
    let keystore = open_keystore(paths)?;

    // Operator-presented attestation public keys.
    let mut attestation_publics: Vec<PublicKey> =
        Vec::with_capacity(attestation_keys_hex.len() + usize::from(generate_attestation_keys));
    for hex_str in &attestation_keys_hex {
        attestation_publics.push(parse_attestation_key_hex(hex_str)?);
    }

    // Generate-and-print attestation keys. Each emits a single line of
    // `seed = <base64>  pubkey = <hex>  key_id = <id>` to the returned
    // output (which prints to stdout) plus a stderr warning. Kairo
    // does NOT save the seed; the operator is responsible for moving
    // it to cold storage before continuing.
    let mut generated_block = String::new();
    if generate_attestation_keys > 0 {
        eprintln!(
            "WARNING: {} attestation seed(s) below will not be saved by Kairo. \
             Record them in cold storage now (YubiKey, air-gapped device, encrypted \
             text in a safe). Kairo will never display them again. See ACTORS.md \
             §5.5.2.",
            generate_attestation_keys
        );
        generated_block.push_str(&format!(
            "generated_attestation_keys = {generate_attestation_keys}\n"
        ));
        for index in 0..generate_attestation_keys {
            let secret =
                SecretSigningKey::generate_ed25519().map_err(CliError::GenerateKey)?;
            let public = secret.public_key();
            let seed_b64 = STANDARD.encode(secret.seed_bytes());
            let pubkey_hex = encode_public_key_hex(&public);
            let key_id = public.key_id();
            generated_block.push_str(&format!(
                "  - index = {index}\n    seed = {seed_b64}\n    pubkey = {pubkey_hex}\n    attestation_key_id = {key_id}\n"
            ));
            attestation_publics.push(public);
            // `secret` leaves scope here; the seed will be overwritten
            // on next allocation. A future revision should integrate
            // the `zeroize` crate for explicit wipe.
        }
    }

    let secret = SecretSigningKey::generate_ed25519().map_err(CliError::GenerateKey)?;
    let nonce = generate_nonce().map_err(CliError::GenerateKey)?;

    let body = ActorGenesisBody::new(
        ActorKind::new(kind),
        secret.public_key(),
        attestation_publics.clone(),
        attestation_threshold,
        Timestamp::now(),
        nonce,
    )
    .map_err(CliError::ActorGenesisShape)?;
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

    let mut attestation_summary = String::new();
    attestation_summary.push_str(&format!(
        "attestation_keys = {}\n",
        attestation_publics.len()
    ));
    for key in &attestation_publics {
        attestation_summary.push_str(&format!("  - key_id = {}\n", key.key_id()));
    }

    Ok(format!(
        "created actor\nactor = {actor_id}\nkey_id = {}\nstore = {}\nkeys = {}\n{attestation_summary}{generated_block}",
        secret.public_key().key_id(),
        paths.store.display(),
        paths.keys.display(),
    ))
}

fn parse_attestation_key_hex(hex_str: &str) -> Result<PublicKey, CliError> {
    let bytes = decode_hex_32(hex_str).ok_or_else(|| CliError::InvalidAttestationKeyHex {
        provided: hex_str.to_owned(),
    })?;
    Ok(PublicKey::ed25519(bytes))
}

fn encode_public_key_hex(public: &PublicKey) -> String {
    let bytes = public.bytes();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn decode_hex_32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0_u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(chunk[0])?;
        let low = hex_nibble(chunk[1])?;
        out[index] = (high << 4) | low;
    }
    Some(out)
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn run_actor_recover_key(
    paths: &StorePaths,
    command: RecoverKeyCommand,
) -> Result<String, CliError> {
    match command {
        RecoverKeyCommand::Sign {
            actor,
            attestation_key_seed,
        } => run_actor_recover_key_sign(paths, actor, attestation_key_seed),
        RecoverKeyCommand::Prepare {
            actor,
            new_key,
            output,
        } => run_actor_recover_key_prepare(paths, actor, new_key, output),
        RecoverKeyCommand::Submit {
            prepared,
            signature,
        } => run_actor_recover_key_submit(paths, prepared, signature),
    }
}

fn run_actor_recover_key_sign(
    paths: &StorePaths,
    actor: String,
    attestation_key_seed: PathBuf,
) -> Result<String, CliError> {
    let actor_id = ActorId::new(actor.clone())
        .map_err(|source| CliError::ParseActorId { actor, source })?;

    let store = open_store(paths)?;
    let keystore = open_keystore(paths)?;

    // Confirm the actor exists.
    let _ = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;

    // Read & decode the attestation seed. The seed file leaves
    // process memory once we've built `SecretSigningKey`; future
    // revisions should integrate `zeroize` for explicit wipe.
    let attestation_secret = read_attestation_seed(&attestation_key_seed)?;
    let attestation_public = attestation_secret.public_key();
    let attestation_key_id = attestation_public.key_id();

    // The attestation key must be in the actor's attestation set at
    // `now`. Genesis-declared + later `ActorAttestationKeyAdd` adds.
    let now = Timestamp::now();
    let attestation_set = ActorResolver::attestation_keys_at(&store, &actor_id, now)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?;
    if !attestation_set.contains_key(&attestation_key_id) {
        return Err(CliError::AttestationKeyNotInSet {
            actor: actor_id,
            key_id: attestation_key_id,
        });
    }

    // Auto-chain: emergency rotations participate in the same chain
    // as routine ones. Supersedes the most-recent rotation chain
    // leaf, if any. Genesis-initial is implicit when the chain is
    // empty (`supersedes = None`).
    let supersedes = latest_rotation_supersedes(&store, &actor_id)?;

    // Generate a fresh active signing key.
    let new_secret = SecretSigningKey::generate_ed25519().map_err(CliError::GenerateKey)?;

    // Build & sign the emergency rotation.
    let body = ActorEmergencyKeyRotationBody::new(new_secret.public_key(), supersedes.clone());
    let subject: KairoRef = format!("actor:{actor_id}").parse().map_err(|source| {
        CliError::BuildActorSubjectRef {
            actor: actor_id.clone(),
            source,
        }
    })?;
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, now, body);
    let signature_bytes = attestation_secret.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        actor_id.clone(),
        attestation_key_id.to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    let signed = MultiSignedStatement::single(unsigned, signature);
    let statement_id = signed.statement_id();

    store
        .put_actor_emergency_key_rotation(&signed)
        .map_err(|error| CliError::WriteEmergencyKeyRotation {
            statement: statement_id.clone(),
            source: error,
        })?;

    // Place the new active signing key in the keystore. Use put if
    // there's no prior key (recovery scenario where the operator
    // lost everything), otherwise replace.
    let new_key_id = match keystore.put_signing_key(&actor_id, &new_secret) {
        Ok(id) => id,
        Err(kairo_keystore::KeystoreError::Corrupt {
            reason: kairo_keystore::CorruptReason::AlreadyExists,
            ..
        }) => keystore
            .replace_signing_key(&actor_id, &new_secret)
            .map_err(|error| CliError::WriteKey {
                actor: actor_id.clone(),
                source: error,
            })?,
        Err(error) => {
            return Err(CliError::WriteKey {
                actor: actor_id,
                source: error,
            });
        }
    };

    let supersedes_line = match supersedes {
        Some(id) => format!("supersedes = {id}\n"),
        None => "supersedes = (genesis)\n".to_owned(),
    };
    Ok(format!(
        "recovered active key (emergency rotation)\nstatement = {statement_id}\nactor = {actor_id}\nattestation_key_id = {attestation_key_id}\nnext_key_id = {new_key_id}\n{supersedes_line}"
    ))
}

fn run_actor_recover_key_prepare(
    paths: &StorePaths,
    actor: String,
    new_key_hex: String,
    output: PathBuf,
) -> Result<String, CliError> {
    let actor_id = ActorId::new(actor.clone())
        .map_err(|source| CliError::ParseActorId { actor, source })?;

    let store = open_store(paths)?;
    let _ = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;

    let next_key = parse_attestation_key_hex(&new_key_hex)?;
    let supersedes = latest_rotation_supersedes(&store, &actor_id)?;
    let now = Timestamp::now();
    let body = ActorEmergencyKeyRotationBody::new(next_key.clone(), supersedes.clone());
    let subject: KairoRef = format!("actor:{actor_id}").parse().map_err(|source| {
        CliError::BuildActorSubjectRef {
            actor: actor_id.clone(),
            source,
        }
    })?;
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, now, body);
    let canonical_bytes = unsigned.canonical_bytes();

    // Emit a partial envelope with `signatures: []`. Cosigners append
    // entries via `kairo actor co-sign`; `submit` validates the
    // resulting envelope (non-empty + threshold + per-signature)
    // before persisting.
    let envelope_json = ActorEmergencyKeyRotationStatementJson {
        statement_type: "ActorEmergencyKeyRotation".to_owned(),
        version: 1,
        actor: actor_id.to_string(),
        subject: format!("actor:{actor_id}"),
        created_at: unsigned.created_at().to_string(),
        body: kairo_statement::json::ActorEmergencyKeyRotationBodyJson::from_body(unsigned.body()),
        signatures: Vec::new(),
    };
    let envelope_bytes = serde_json::to_vec_pretty(&envelope_json)
        .map_err(CliError::SerializePreparedEnvelope)?;

    std::fs::write(&output, &envelope_bytes).map_err(|source| {
        CliError::WritePreparedEnvelope {
            path: output.clone(),
            source,
        }
    })?;
    let payload_path = payload_path_for(&output);
    std::fs::write(&payload_path, &canonical_bytes).map_err(|source| {
        CliError::WritePreparedEnvelope {
            path: payload_path.clone(),
            source,
        }
    })?;

    let attestation_set = ActorResolver::attestation_keys_at(&store, &actor_id, now)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?;
    let mut attestation_lines = String::new();
    for key in attestation_set.keys() {
        attestation_lines.push_str(&format!("  - {key}\n"));
    }

    Ok(format!(
        "prepared emergency rotation envelope\nactor = {actor_id}\nnext_key_id = {}\nenvelope = {}\npayload = {}\n\nNext steps:\n  1. Each cosigner signs {} with one of the actor's attestation keys (see list below):\n{attestation_lines}  2. For each signature, run `kairo actor co-sign --prepared {} --actor {actor_id} --attestation-key-seed <path>` to append to the envelope (or pass `--signature <path>` to `submit` for the single-signer flow).\n  3. Run `kairo actor recover-key submit --prepared {}` to finalize and persist.\n",
        next_key.key_id(),
        output.display(),
        payload_path.display(),
        payload_path.display(),
        output.display(),
        output.display(),
    ))
}

fn run_actor_recover_key_submit(
    paths: &StorePaths,
    prepared: PathBuf,
    signature: Option<PathBuf>,
) -> Result<String, CliError> {
    let store = open_store(paths)?;

    // Read and parse the prepared envelope.
    let envelope_bytes =
        std::fs::read(&prepared).map_err(|source| CliError::ReadPreparedEnvelope {
            path: prepared.clone(),
            source,
        })?;
    let mut envelope_json: ActorEmergencyKeyRotationStatementJson =
        serde_json::from_slice(&envelope_bytes).map_err(CliError::ParseStatementJson)?;

    let actor_id = ActorId::new(envelope_json.actor.clone()).map_err(|source| {
        CliError::ParseActorId {
            actor: envelope_json.actor.clone(),
            source,
        }
    })?;
    let _ = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;

    let body_unsigned = envelope_json
        .body
        .to_body()
        .map_err(CliError::ParseStatement)?;
    let subject: KairoRef = envelope_json.subject.parse().map_err(|source| {
        CliError::BuildActorSubjectRef {
            actor: actor_id.clone(),
            source,
        }
    })?;
    let created_at: Timestamp =
        envelope_json
            .created_at
            .parse()
            .map_err(|source| CliError::ParseTimestamp {
                source,
                value: envelope_json.created_at.clone(),
            })?;
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, created_at, body_unsigned);
    let canonical = unsigned.canonical_bytes();

    // Backward-compat single-signer path: --signature provided, the
    // operator signed externally, and submit auto-detects which
    // attestation key produced it.
    if let Some(sig_path) = signature {
        let sig_bytes = read_signature_bytes(&sig_path)?;
        let attestation_set = ActorResolver::attestation_keys_at(&store, &actor_id, created_at)
            .map_err(|error| CliError::ReadActiveKey {
                actor: actor_id.clone(),
                source: error,
            })?;
        let signature_struct = kairo_identity::SignatureBytes::ed25519(sig_bytes);
        let mut signing_key_id = None;
        for (key_id, public) in &attestation_set {
            if kairo_identity::verify_signature(public, &canonical, &signature_struct).is_ok() {
                signing_key_id = Some(key_id.clone());
                break;
            }
        }
        let signing_key_id =
            signing_key_id.ok_or_else(|| CliError::SignatureNoAttestationMatch {
                actor: actor_id.clone(),
            })?;
        envelope_json
            .signatures
            .push(kairo_statement::json::SignatureJson {
                actor: actor_id.to_string(),
                key_id: signing_key_id.to_string(),
                algorithm: "ed25519".to_owned(),
                bytes: STANDARD.encode(sig_bytes),
            });
    }

    // Construct the multi-sig envelope (validates non-empty + distinct
    // key_ids), then verify threshold + per-signature validity against
    // the resolver. Refuse sub-threshold envelopes.
    let signed = envelope_json
        .to_statement()
        .map_err(CliError::ParseStatement)?;
    let report = kairo_statement::verify::verify_envelope_multi_statement(&signed, &store);
    if !report.is_cryptographically_valid() {
        return Err(CliError::VerificationFailed(Box::new(report)));
    }

    let statement_id = signed.statement_id();
    store
        .put_actor_emergency_key_rotation(&signed)
        .map_err(|error| CliError::WriteEmergencyKeyRotation {
            statement: statement_id.clone(),
            source: error,
        })?;

    let next_key_id = signed.unsigned().body().next_key().key_id();
    let signing_key_id = signed
        .signatures()
        .first()
        .map(|s| s.key_id().to_owned())
        .unwrap_or_default();
    Ok(format!(
        "imported emergency rotation\nstatement = {statement_id}\nactor = {actor_id}\nattestation_key_id = {signing_key_id}\nnext_key_id = {next_key_id}\nNote: the new active signing key is operator-managed (not in the keystore). Sign future statements externally or import the secret separately.\n"
    ))
}

fn run_actor_add_attestation_key(
    paths: &StorePaths,
    command: AddAttestationKeyCommand,
) -> Result<String, CliError> {
    match command {
        AddAttestationKeyCommand::Sign {
            actor,
            signing_attestation_key_seed,
            key,
            generate,
        } => run_actor_add_attestation_key_sign(
            paths,
            actor,
            signing_attestation_key_seed,
            key,
            generate,
        ),
        AddAttestationKeyCommand::Prepare {
            actor,
            new_key,
            output,
        } => run_actor_add_attestation_key_prepare(paths, actor, new_key, output),
        AddAttestationKeyCommand::Submit {
            prepared,
            signature,
        } => run_actor_add_attestation_key_submit(paths, prepared, signature),
    }
}

fn run_actor_add_attestation_key_sign(
    paths: &StorePaths,
    actor: String,
    signing_attestation_key_seed: PathBuf,
    key: Option<String>,
    generate: bool,
) -> Result<String, CliError> {
    if key.is_none() && !generate {
        return Err(CliError::AddAttestationKeyMissingKeySource);
    }

    let actor_id = ActorId::new(actor.clone())
        .map_err(|source| CliError::ParseActorId { actor, source })?;

    let store = open_store(paths)?;

    // Confirm the actor exists.
    let _ = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;

    // Resolve the new attestation public key. Either operator-
    // presented or generated-and-printed; in the generate case the
    // seed leaves scope at the end of this function.
    let mut generated_block = String::new();
    let new_attestation_public = if let Some(hex) = key {
        parse_attestation_key_hex(&hex)?
    } else {
        eprintln!(
            "WARNING: a fresh attestation seed will be printed below and not saved by Kairo. \
             Record it in cold storage now. See ACTORS.md §5.5.2."
        );
        let secret = SecretSigningKey::generate_ed25519().map_err(CliError::GenerateKey)?;
        let public = secret.public_key();
        let seed_b64 = STANDARD.encode(secret.seed_bytes());
        let pubkey_hex = encode_public_key_hex(&public);
        generated_block.push_str(&format!(
            "generated_attestation_seed = {seed_b64}\ngenerated_attestation_pubkey = {pubkey_hex}\n"
        ));
        public
    };

    // Read & decode the signing attestation seed (existing one).
    let signing_secret = read_attestation_seed(&signing_attestation_key_seed)?;
    let signing_public = signing_secret.public_key();
    let signing_key_id = signing_public.key_id();

    let now = Timestamp::now();
    let attestation_set = ActorResolver::attestation_keys_at(&store, &actor_id, now)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?;
    if !attestation_set.contains_key(&signing_key_id) {
        return Err(CliError::AttestationKeyNotInSet {
            actor: actor_id,
            key_id: signing_key_id,
        });
    }

    // Validation (`ACTORS.md` §5.5.2 / canonical spec): new_key must
    // not already be in the attestation set, and must be disjoint
    // from any signing key the actor has held.
    let new_attestation_key_id = new_attestation_public.key_id();
    if attestation_set.contains_key(&new_attestation_key_id) {
        return Err(CliError::AttestationKeyAlreadyInSet {
            actor: actor_id,
            key_id: new_attestation_key_id,
        });
    }
    let actor_body = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;
    if actor_body.initial_key().bytes() == new_attestation_public.bytes() {
        return Err(CliError::AttestationKeySharesSigningKey {
            actor: actor_id,
            key_id: new_attestation_key_id,
        });
    }
    let rotations = ActorResolver::key_rotations(&store, &actor_id).map_err(|error| {
        CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        }
    })?;
    if rotations
        .iter()
        .any(|entry| entry.next_key.bytes() == new_attestation_public.bytes())
    {
        return Err(CliError::AttestationKeySharesSigningKey {
            actor: actor_id,
            key_id: new_attestation_key_id,
        });
    }

    let body = ActorAttestationKeyAddBody::new(new_attestation_public);
    let subject: KairoRef = format!("actor:{actor_id}").parse().map_err(|source| {
        CliError::BuildActorSubjectRef {
            actor: actor_id.clone(),
            source,
        }
    })?;
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, now, body);
    let signature_bytes = signing_secret.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        actor_id.clone(),
        signing_key_id.to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    let signed = MultiSignedStatement::single(unsigned, signature);
    let statement_id = signed.statement_id();

    store
        .put_actor_attestation_key_add(&signed)
        .map_err(|error| CliError::WriteAttestationKeyAdd {
            statement: statement_id.clone(),
            source: error,
        })?;

    Ok(format!(
        "added attestation key\nstatement = {statement_id}\nactor = {actor_id}\nsigning_attestation_key_id = {signing_key_id}\nnew_attestation_key_id = {new_attestation_key_id}\n{generated_block}"
    ))
}

fn run_actor_add_attestation_key_prepare(
    paths: &StorePaths,
    actor: String,
    new_key_hex: String,
    output: PathBuf,
) -> Result<String, CliError> {
    let actor_id = ActorId::new(actor.clone())
        .map_err(|source| CliError::ParseActorId { actor, source })?;

    let store = open_store(paths)?;
    let _ = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;

    let new_key = parse_attestation_key_hex(&new_key_hex)?;
    let now = Timestamp::now();
    let body = ActorAttestationKeyAddBody::new(new_key.clone());
    let subject: KairoRef = format!("actor:{actor_id}").parse().map_err(|source| {
        CliError::BuildActorSubjectRef {
            actor: actor_id.clone(),
            source,
        }
    })?;
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, now, body);
    let canonical_bytes = unsigned.canonical_bytes();

    let envelope_json = ActorAttestationKeyAddStatementJson {
        statement_type: "ActorAttestationKeyAdd".to_owned(),
        version: 1,
        actor: actor_id.to_string(),
        subject: format!("actor:{actor_id}"),
        created_at: unsigned.created_at().to_string(),
        body: kairo_statement::json::ActorAttestationKeyAddBodyJson::from_body(unsigned.body()),
        signatures: Vec::new(),
    };
    let envelope_bytes = serde_json::to_vec_pretty(&envelope_json)
        .map_err(CliError::SerializePreparedEnvelope)?;

    std::fs::write(&output, &envelope_bytes).map_err(|source| {
        CliError::WritePreparedEnvelope {
            path: output.clone(),
            source,
        }
    })?;
    let payload_path = payload_path_for(&output);
    std::fs::write(&payload_path, &canonical_bytes).map_err(|source| {
        CliError::WritePreparedEnvelope {
            path: payload_path.clone(),
            source,
        }
    })?;

    let attestation_set = ActorResolver::attestation_keys_at(&store, &actor_id, now)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?;
    let mut attestation_lines = String::new();
    for key in attestation_set.keys() {
        attestation_lines.push_str(&format!("  - {key}\n"));
    }

    Ok(format!(
        "prepared attestation-key-add envelope\nactor = {actor_id}\nnew_attestation_key_id = {}\nenvelope = {}\npayload = {}\n\nNext steps:\n  1. Each cosigner signs {} with one of the actor's existing attestation keys (see list below):\n{attestation_lines}  2. For each signature, run `kairo actor co-sign --prepared {} --actor {actor_id} --attestation-key-seed <path>` (or pass `--signature <path>` to `submit` for the single-signer flow).\n  3. Run `kairo actor add-attestation-key submit --prepared {}` to finalize.\n",
        new_key.key_id(),
        output.display(),
        payload_path.display(),
        payload_path.display(),
        output.display(),
        output.display(),
    ))
}

fn run_actor_add_attestation_key_submit(
    paths: &StorePaths,
    prepared: PathBuf,
    signature: Option<PathBuf>,
) -> Result<String, CliError> {
    let store = open_store(paths)?;

    let envelope_bytes =
        std::fs::read(&prepared).map_err(|source| CliError::ReadPreparedEnvelope {
            path: prepared.clone(),
            source,
        })?;
    let mut envelope_json: ActorAttestationKeyAddStatementJson =
        serde_json::from_slice(&envelope_bytes).map_err(CliError::ParseStatementJson)?;

    let actor_id = ActorId::new(envelope_json.actor.clone()).map_err(|source| {
        CliError::ParseActorId {
            actor: envelope_json.actor.clone(),
            source,
        }
    })?;
    let _ = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;

    let body_unsigned = envelope_json
        .body
        .to_body()
        .map_err(CliError::ParseStatement)?;
    let subject: KairoRef = envelope_json.subject.parse().map_err(|source| {
        CliError::BuildActorSubjectRef {
            actor: actor_id.clone(),
            source,
        }
    })?;
    let created_at: Timestamp =
        envelope_json
            .created_at
            .parse()
            .map_err(|source| CliError::ParseTimestamp {
                source,
                value: envelope_json.created_at.clone(),
            })?;
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, created_at, body_unsigned);
    let canonical = unsigned.canonical_bytes();

    if let Some(sig_path) = signature {
        let sig_bytes = read_signature_bytes(&sig_path)?;
        let attestation_set = ActorResolver::attestation_keys_at(&store, &actor_id, created_at)
            .map_err(|error| CliError::ReadActiveKey {
                actor: actor_id.clone(),
                source: error,
            })?;
        let signature_struct = kairo_identity::SignatureBytes::ed25519(sig_bytes);
        let mut signing_key_id = None;
        for (key_id, public) in &attestation_set {
            if kairo_identity::verify_signature(public, &canonical, &signature_struct).is_ok() {
                signing_key_id = Some(key_id.clone());
                break;
            }
        }
        let signing_key_id =
            signing_key_id.ok_or_else(|| CliError::SignatureNoAttestationMatch {
                actor: actor_id.clone(),
            })?;
        envelope_json
            .signatures
            .push(kairo_statement::json::SignatureJson {
                actor: actor_id.to_string(),
                key_id: signing_key_id.to_string(),
                algorithm: "ed25519".to_owned(),
                bytes: STANDARD.encode(sig_bytes),
            });
    }

    let signed = envelope_json
        .to_statement()
        .map_err(CliError::ParseStatement)?;
    let report = kairo_statement::verify::verify_envelope_multi_statement(&signed, &store);
    if !report.is_cryptographically_valid() {
        return Err(CliError::VerificationFailed(Box::new(report)));
    }

    let statement_id = signed.statement_id();
    store
        .put_actor_attestation_key_add(&signed)
        .map_err(|error| CliError::WriteAttestationKeyAdd {
            statement: statement_id.clone(),
            source: error,
        })?;

    let new_attestation_key_id = signed.unsigned().body().new_key().key_id();
    let signing_key_id = signed
        .signatures()
        .first()
        .map(|s| s.key_id().to_owned())
        .unwrap_or_default();
    Ok(format!(
        "imported attestation-key-add\nstatement = {statement_id}\nactor = {actor_id}\nsigning_attestation_key_id = {signing_key_id}\nnew_attestation_key_id = {new_attestation_key_id}\n"
    ))
}

fn run_actor_revoke_attestation_key(
    paths: &StorePaths,
    command: RevokeAttestationKeyCommand,
) -> Result<String, CliError> {
    match command {
        RevokeAttestationKeyCommand::Sign {
            actor,
            signing_attestation_key_seed,
            revoke_key,
            reason,
        } => run_actor_revoke_attestation_key_sign(
            paths,
            actor,
            signing_attestation_key_seed,
            revoke_key,
            reason,
        ),
        RevokeAttestationKeyCommand::Prepare {
            actor,
            revoke_key,
            reason,
            output,
        } => run_actor_revoke_attestation_key_prepare(paths, actor, revoke_key, reason, output),
        RevokeAttestationKeyCommand::Submit {
            prepared,
            signature,
        } => run_actor_revoke_attestation_key_submit(paths, prepared, signature),
    }
}

/// Single-signer convenience: sign and persist directly. The
/// non-empty-set-versus-threshold guard fires at the store layer if
/// the resulting attestation set would fall below the live threshold;
/// the message tells the operator to add a replacement first. The
/// asymmetric authority rule for the threshold itself is *not*
/// applied to revocations — only to threshold changes — but the live
/// threshold still gates how many distinct sigs the envelope must
/// carry, so this convenience flow is only useful at threshold = 1.
fn run_actor_revoke_attestation_key_sign(
    paths: &StorePaths,
    actor: String,
    signing_attestation_key_seed: PathBuf,
    revoke_key: String,
    reason: Option<String>,
) -> Result<String, CliError> {
    let actor_id = ActorId::new(actor.clone())
        .map_err(|source| CliError::ParseActorId { actor, source })?;
    let store = open_store(paths)?;
    let _ = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;

    let revoked_key_id = kairo_identity::KeyId::new(revoke_key);
    let body = kairo_statement::ActorAttestationKeyRevocationBody::new(
        revoked_key_id.clone(),
        reason,
    );
    let subject: KairoRef = format!("actor:{actor_id}")
        .parse()
        .map_err(|source| CliError::BuildActorSubjectRef {
            actor: actor_id.clone(),
            source,
        })?;
    let now = Timestamp::now();
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, now, body);

    let secret = read_attestation_seed(&signing_attestation_key_seed)?;
    let public = secret.public_key();
    let signing_key_id = public.key_id();

    let attestation_set = ActorResolver::attestation_keys_at(&store, &actor_id, now)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?;
    if !attestation_set.contains_key(&signing_key_id) {
        return Err(CliError::CosignKeyNotInAttestationSet {
            actor: actor_id.clone(),
            key_id: signing_key_id.to_string(),
        });
    }

    let sig_bytes = secret.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        actor_id.clone(),
        signing_key_id.to_string(),
        "ed25519",
        sig_bytes.bytes().to_vec(),
    );
    let signed = MultiSignedStatement::single(unsigned, signature);
    let report = kairo_statement::verify::verify_envelope_multi_statement(&signed, &store);
    if !report.is_cryptographically_valid() {
        return Err(CliError::VerificationFailed(Box::new(report)));
    }
    let statement_id = signed.statement_id();
    store
        .put_actor_attestation_key_revocation(&signed)
        .map_err(|error| CliError::WriteAttestationKeyRevocation {
            statement: statement_id.clone(),
            source: error,
        })?;

    Ok(format!(
        "revoked attestation key\nstatement = {statement_id}\nactor = {actor_id}\nsigning_attestation_key_id = {signing_key_id}\nrevoked_key = {revoked_key_id}\n"
    ))
}

fn run_actor_revoke_attestation_key_prepare(
    paths: &StorePaths,
    actor: String,
    revoke_key: String,
    reason: Option<String>,
    output: PathBuf,
) -> Result<String, CliError> {
    let actor_id = ActorId::new(actor.clone())
        .map_err(|source| CliError::ParseActorId { actor, source })?;
    let store = open_store(paths)?;
    let _ = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;

    let revoked_key_id = kairo_identity::KeyId::new(revoke_key);
    let body = kairo_statement::ActorAttestationKeyRevocationBody::new(
        revoked_key_id.clone(),
        reason,
    );
    let subject: KairoRef = format!("actor:{actor_id}")
        .parse()
        .map_err(|source| CliError::BuildActorSubjectRef {
            actor: actor_id.clone(),
            source,
        })?;
    let now = Timestamp::now();
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, now, body);
    let canonical_bytes = unsigned.canonical_bytes();

    let envelope_json = kairo_statement::json::ActorAttestationKeyRevocationStatementJson {
        statement_type: "ActorAttestationKeyRevocation".to_owned(),
        version: 1,
        actor: actor_id.to_string(),
        subject: format!("actor:{actor_id}"),
        created_at: unsigned.created_at().to_string(),
        body: kairo_statement::json::ActorAttestationKeyRevocationBodyJson::from_body(
            unsigned.body(),
        ),
        signatures: Vec::new(),
    };
    let envelope_bytes = serde_json::to_vec_pretty(&envelope_json)
        .map_err(CliError::SerializePreparedEnvelope)?;
    std::fs::write(&output, &envelope_bytes).map_err(|source| {
        CliError::WritePreparedEnvelope {
            path: output.clone(),
            source,
        }
    })?;
    let payload_path = payload_path_for(&output);
    std::fs::write(&payload_path, &canonical_bytes).map_err(|source| {
        CliError::WritePreparedEnvelope {
            path: payload_path.clone(),
            source,
        }
    })?;

    let attestation_set = ActorResolver::attestation_keys_at(&store, &actor_id, now)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?;
    let mut attestation_lines = String::new();
    for key in attestation_set.keys() {
        attestation_lines.push_str(&format!("  - {key}\n"));
    }
    let need = ActorResolver::attestation_threshold_at(&store, &actor_id, now)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?
        .unwrap_or(1);

    Ok(format!(
        "prepared attestation-key-revocation envelope\nactor = {actor_id}\nrevoked_key = {revoked_key_id}\nrequired_signatures = {need}\nenvelope = {}\npayload = {}\n\nNext steps:\n  1. Each cosigner signs {} with one of the actor's attestation keys (see list below):\n{attestation_lines}  2. For each signature, run `kairo actor co-sign --prepared {} --actor {actor_id} --attestation-key-seed <path>` (or pass `--signature <path>` to `submit` for the single-signer flow).\n  3. Run `kairo actor revoke-attestation-key submit --prepared {}` to finalize.\n",
        output.display(),
        payload_path.display(),
        payload_path.display(),
        output.display(),
        output.display(),
    ))
}

fn run_actor_revoke_attestation_key_submit(
    paths: &StorePaths,
    prepared: PathBuf,
    signature: Option<PathBuf>,
) -> Result<String, CliError> {
    let store = open_store(paths)?;

    let envelope_bytes =
        std::fs::read(&prepared).map_err(|source| CliError::ReadPreparedEnvelope {
            path: prepared.clone(),
            source,
        })?;
    let mut envelope_json: kairo_statement::json::ActorAttestationKeyRevocationStatementJson =
        serde_json::from_slice(&envelope_bytes).map_err(CliError::ParseStatementJson)?;

    let actor_id = ActorId::new(envelope_json.actor.clone()).map_err(|source| {
        CliError::ParseActorId {
            actor: envelope_json.actor.clone(),
            source,
        }
    })?;
    let _ = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;

    let body_unsigned = envelope_json
        .body
        .to_body()
        .map_err(CliError::ParseStatement)?;
    let subject: KairoRef = envelope_json.subject.parse().map_err(|source| {
        CliError::BuildActorSubjectRef {
            actor: actor_id.clone(),
            source,
        }
    })?;
    let created_at: Timestamp =
        envelope_json
            .created_at
            .parse()
            .map_err(|source| CliError::ParseTimestamp {
                source,
                value: envelope_json.created_at.clone(),
            })?;
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, created_at, body_unsigned);
    let canonical = unsigned.canonical_bytes();

    if let Some(sig_path) = signature {
        let sig_bytes = read_signature_bytes(&sig_path)?;
        let attestation_set = ActorResolver::attestation_keys_at(&store, &actor_id, created_at)
            .map_err(|error| CliError::ReadActiveKey {
                actor: actor_id.clone(),
                source: error,
            })?;
        let signature_struct = kairo_identity::SignatureBytes::ed25519(sig_bytes);
        let mut signing_key_id = None;
        for (key_id, public) in &attestation_set {
            if kairo_identity::verify_signature(public, &canonical, &signature_struct).is_ok() {
                signing_key_id = Some(key_id.clone());
                break;
            }
        }
        let signing_key_id =
            signing_key_id.ok_or_else(|| CliError::SignatureNoAttestationMatch {
                actor: actor_id.clone(),
            })?;
        envelope_json
            .signatures
            .push(kairo_statement::json::SignatureJson {
                actor: actor_id.to_string(),
                key_id: signing_key_id.to_string(),
                algorithm: "ed25519".to_owned(),
                bytes: STANDARD.encode(sig_bytes),
            });
    }

    let signed = envelope_json
        .to_statement()
        .map_err(CliError::ParseStatement)?;
    let report = kairo_statement::verify::verify_envelope_multi_statement(&signed, &store);
    if !report.is_cryptographically_valid() {
        return Err(CliError::VerificationFailed(Box::new(report)));
    }
    let revoked_key = signed.unsigned().body().revoked_key().clone();
    let statement_id = signed.statement_id();
    store
        .put_actor_attestation_key_revocation(&signed)
        .map_err(|error| CliError::WriteAttestationKeyRevocation {
            statement: statement_id.clone(),
            source: error,
        })?;

    Ok(format!(
        "revoked attestation key\nstatement = {statement_id}\nactor = {actor_id}\nrevoked_key = {revoked_key}\nsignatures = {}\n",
        signed.signatures().len(),
    ))
}

fn run_actor_change_attestation_threshold(
    paths: &StorePaths,
    command: ChangeAttestationThresholdCommand,
) -> Result<String, CliError> {
    match command {
        ChangeAttestationThresholdCommand::Sign {
            actor,
            attestation_key_seed,
            to,
        } => run_actor_change_attestation_threshold_sign(paths, actor, attestation_key_seed, to),
        ChangeAttestationThresholdCommand::Prepare { actor, to, output } => {
            run_actor_change_attestation_threshold_prepare(paths, actor, to, output)
        }
        ChangeAttestationThresholdCommand::Submit {
            prepared,
            signature,
        } => run_actor_change_attestation_threshold_submit(paths, prepared, signature),
    }
}

/// Single-signer convenience: read an attestation seed, sign + persist
/// a threshold change directly. Only valid when the actor's *current*
/// threshold is 1 — once the actor is at threshold ≥ 2, every change
/// (including lowers back to 1) needs `current` distinct signatures
/// per the asymmetric authority rule, which means cosigning rather
/// than this convenience flow. See `ACTORS.md` §5.5.3.
fn run_actor_change_attestation_threshold_sign(
    paths: &StorePaths,
    actor: String,
    attestation_key_seed: PathBuf,
    to: u8,
) -> Result<String, CliError> {
    let actor_id = ActorId::new(actor.clone())
        .map_err(|source| CliError::ParseActorId { actor, source })?;
    let store = open_store(paths)?;
    let _ = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;

    let now = Timestamp::now();
    let current_threshold = ActorResolver::attestation_threshold_at(&store, &actor_id, now)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?
        .ok_or_else(|| CliError::ReadActor {
            actor: actor_id.clone(),
            source: kairo_store::StoreError::Missing,
        })?;
    let required = if to > current_threshold {
        to
    } else {
        current_threshold
    };
    if required > 1 {
        return Err(CliError::ChangeThresholdSignNeedsCosign {
            actor: actor_id,
            current_threshold,
            required,
        });
    }

    let body = kairo_statement::ActorAttestationThresholdChangeBody::new(to)
        .map_err(CliError::ChangeThresholdShape)?;
    let subject: KairoRef = format!("actor:{actor_id}")
        .parse()
        .map_err(|source| CliError::BuildActorSubjectRef {
            actor: actor_id.clone(),
            source,
        })?;
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, now, body);

    let secret = read_attestation_seed(&attestation_key_seed)?;
    let public = secret.public_key();
    let signing_key_id = public.key_id();

    let attestation_set = ActorResolver::attestation_keys_at(&store, &actor_id, now)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?;
    if !attestation_set.contains_key(&signing_key_id) {
        return Err(CliError::CosignKeyNotInAttestationSet {
            actor: actor_id.clone(),
            key_id: signing_key_id.to_string(),
        });
    }

    let sig_bytes = secret.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        actor_id.clone(),
        signing_key_id.to_string(),
        "ed25519",
        sig_bytes.bytes().to_vec(),
    );
    let signed = MultiSignedStatement::single(unsigned, signature);
    let report = kairo_statement::verify::verify_envelope_multi_statement(&signed, &store);
    if !report.is_cryptographically_valid() {
        return Err(CliError::VerificationFailed(Box::new(report)));
    }
    let statement_id = signed.statement_id();
    store
        .put_actor_attestation_threshold_change(&signed)
        .map_err(|error| CliError::WriteThresholdChange {
            statement: statement_id.clone(),
            source: error,
        })?;

    Ok(format!(
        "changed attestation threshold\nstatement = {statement_id}\nactor = {actor_id}\nattestation_key_id = {signing_key_id}\nold_threshold = {current_threshold}\nnew_threshold = {to}\n"
    ))
}

fn run_actor_change_attestation_threshold_prepare(
    paths: &StorePaths,
    actor: String,
    to: u8,
    output: PathBuf,
) -> Result<String, CliError> {
    let actor_id = ActorId::new(actor.clone())
        .map_err(|source| CliError::ParseActorId { actor, source })?;
    let store = open_store(paths)?;
    let _ = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;

    let body = kairo_statement::ActorAttestationThresholdChangeBody::new(to)
        .map_err(CliError::ChangeThresholdShape)?;
    let subject: KairoRef = format!("actor:{actor_id}")
        .parse()
        .map_err(|source| CliError::BuildActorSubjectRef {
            actor: actor_id.clone(),
            source,
        })?;
    let now = Timestamp::now();
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, now, body);
    let canonical_bytes = unsigned.canonical_bytes();

    let envelope_json = kairo_statement::json::ActorAttestationThresholdChangeStatementJson {
        statement_type: "ActorAttestationThresholdChange".to_owned(),
        version: 1,
        actor: actor_id.to_string(),
        subject: format!("actor:{actor_id}"),
        created_at: unsigned.created_at().to_string(),
        body: kairo_statement::json::ActorAttestationThresholdChangeBodyJson::from_body(
            unsigned.body(),
        ),
        signatures: Vec::new(),
    };
    let envelope_bytes = serde_json::to_vec_pretty(&envelope_json)
        .map_err(CliError::SerializePreparedEnvelope)?;
    std::fs::write(&output, &envelope_bytes).map_err(|source| {
        CliError::WritePreparedEnvelope {
            path: output.clone(),
            source,
        }
    })?;
    let payload_path = payload_path_for(&output);
    std::fs::write(&payload_path, &canonical_bytes).map_err(|source| {
        CliError::WritePreparedEnvelope {
            path: payload_path.clone(),
            source,
        }
    })?;

    let current_threshold = ActorResolver::attestation_threshold_at(&store, &actor_id, now)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?
        .unwrap_or(1);
    let need = if to > current_threshold {
        to
    } else {
        current_threshold
    };
    let attestation_set = ActorResolver::attestation_keys_at(&store, &actor_id, now)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?;
    let mut attestation_lines = String::new();
    for key in attestation_set.keys() {
        attestation_lines.push_str(&format!("  - {key}\n"));
    }

    Ok(format!(
        "prepared attestation-threshold-change envelope\nactor = {actor_id}\nold_threshold = {current_threshold}\nnew_threshold = {to}\nrequired_signatures = {need}\nenvelope = {}\npayload = {}\n\nNext steps:\n  1. Each cosigner signs {} with one of the actor's attestation keys (see list below):\n{attestation_lines}  2. For each signature, run `kairo actor co-sign --prepared {} --actor {actor_id} --attestation-key-seed <path>`.\n  3. Run `kairo actor change-attestation-threshold submit --prepared {}` to finalize.\n",
        output.display(),
        payload_path.display(),
        payload_path.display(),
        output.display(),
        output.display(),
    ))
}

fn run_actor_change_attestation_threshold_submit(
    paths: &StorePaths,
    prepared: PathBuf,
    signature: Option<PathBuf>,
) -> Result<String, CliError> {
    let store = open_store(paths)?;

    let envelope_bytes =
        std::fs::read(&prepared).map_err(|source| CliError::ReadPreparedEnvelope {
            path: prepared.clone(),
            source,
        })?;
    let mut envelope_json: kairo_statement::json::ActorAttestationThresholdChangeStatementJson =
        serde_json::from_slice(&envelope_bytes).map_err(CliError::ParseStatementJson)?;

    let actor_id = ActorId::new(envelope_json.actor.clone()).map_err(|source| {
        CliError::ParseActorId {
            actor: envelope_json.actor.clone(),
            source,
        }
    })?;
    let _ = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;

    let body_unsigned = envelope_json
        .body
        .to_body()
        .map_err(CliError::ParseStatement)?;
    let subject: KairoRef = envelope_json.subject.parse().map_err(|source| {
        CliError::BuildActorSubjectRef {
            actor: actor_id.clone(),
            source,
        }
    })?;
    let created_at: Timestamp =
        envelope_json
            .created_at
            .parse()
            .map_err(|source| CliError::ParseTimestamp {
                source,
                value: envelope_json.created_at.clone(),
            })?;
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, created_at, body_unsigned);
    let canonical = unsigned.canonical_bytes();

    if let Some(sig_path) = signature {
        let sig_bytes = read_signature_bytes(&sig_path)?;
        let attestation_set = ActorResolver::attestation_keys_at(&store, &actor_id, created_at)
            .map_err(|error| CliError::ReadActiveKey {
                actor: actor_id.clone(),
                source: error,
            })?;
        let signature_struct = kairo_identity::SignatureBytes::ed25519(sig_bytes);
        let mut signing_key_id = None;
        for (key_id, public) in &attestation_set {
            if kairo_identity::verify_signature(public, &canonical, &signature_struct).is_ok() {
                signing_key_id = Some(key_id.clone());
                break;
            }
        }
        let signing_key_id =
            signing_key_id.ok_or_else(|| CliError::SignatureNoAttestationMatch {
                actor: actor_id.clone(),
            })?;
        envelope_json
            .signatures
            .push(kairo_statement::json::SignatureJson {
                actor: actor_id.to_string(),
                key_id: signing_key_id.to_string(),
                algorithm: "ed25519".to_owned(),
                bytes: STANDARD.encode(sig_bytes),
            });
    }

    let signed = envelope_json
        .to_statement()
        .map_err(CliError::ParseStatement)?;
    let report = kairo_statement::verify::verify_envelope_multi_statement(&signed, &store);
    if !report.is_cryptographically_valid() {
        return Err(CliError::VerificationFailed(Box::new(report)));
    }
    let new_threshold = signed.unsigned().body().new_threshold();
    let statement_id = signed.statement_id();
    store
        .put_actor_attestation_threshold_change(&signed)
        .map_err(|error| CliError::WriteThresholdChange {
            statement: statement_id.clone(),
            source: error,
        })?;

    Ok(format!(
        "changed attestation threshold\nstatement = {statement_id}\nactor = {actor_id}\nnew_threshold = {new_threshold}\nsignatures = {}\n",
        signed.signatures().len(),
    ))
}

/// Append a single attestation signature to a partial envelope.
///
/// Operates generically across attestation-surface envelope kinds by
/// mutating the JSON `signatures` array directly — body shape doesn't
/// matter to the cosigner since the canonical bytes the seed signs
/// over are taken from the `<prepared>.payload` sidecar emitted by
/// the matching `prepare` flow. The cosigner's seed must be in the
/// actor's attestation set at the envelope's `created_at`.
fn run_actor_cosign(
    paths: &StorePaths,
    prepared: PathBuf,
    actor: String,
    attestation_key_seed: PathBuf,
) -> Result<String, CliError> {
    let actor_id = ActorId::new(actor.clone())
        .map_err(|source| CliError::ParseActorId { actor, source })?;
    let store = open_store(paths)?;

    let envelope_bytes =
        std::fs::read(&prepared).map_err(|source| CliError::ReadPreparedEnvelope {
            path: prepared.clone(),
            source,
        })?;
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&envelope_bytes).map_err(CliError::ParseStatementJson)?;

    let envelope_obj = envelope
        .as_object_mut()
        .ok_or(CliError::CosignEnvelopeShape)?;

    // The envelope must carry the same actor as --actor so the
    // operator can't accidentally cosign a different actor's
    // statement using one of their own attestation keys.
    let envelope_actor = envelope_obj
        .get("actor")
        .and_then(|v| v.as_str())
        .ok_or(CliError::CosignEnvelopeShape)?;
    if envelope_actor != actor_id.as_str() {
        return Err(CliError::CosignActorMismatch {
            expected: actor_id.clone(),
            actual: envelope_actor.to_owned(),
        });
    }

    let created_at_str = envelope_obj
        .get("created_at")
        .and_then(|v| v.as_str())
        .ok_or(CliError::CosignEnvelopeShape)?;
    let created_at: Timestamp =
        created_at_str
            .parse()
            .map_err(|source| CliError::ParseTimestamp {
                source,
                value: created_at_str.to_owned(),
            })?;

    // Canonical bytes come from the sidecar emitted at prepare time.
    // Trusting the sidecar avoids dispatching on the body type here;
    // submit re-derives canonical bytes from the body and refuses
    // mismatches via `MultiSignedStatement::statement_id()`.
    let payload_path = payload_path_for(&prepared);
    let canonical = std::fs::read(&payload_path).map_err(|source| {
        CliError::ReadPreparedEnvelope {
            path: payload_path.clone(),
            source,
        }
    })?;

    let secret = read_attestation_seed(&attestation_key_seed)?;
    let public = secret.public_key();
    let signing_key_id = public.key_id();

    let attestation_set = ActorResolver::attestation_keys_at(&store, &actor_id, created_at)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?;
    if !attestation_set.contains_key(&signing_key_id) {
        return Err(CliError::CosignKeyNotInAttestationSet {
            actor: actor_id.clone(),
            key_id: signing_key_id.to_string(),
        });
    }

    let signatures = envelope_obj
        .get_mut("signatures")
        .ok_or(CliError::CosignEnvelopeShape)?
        .as_array_mut()
        .ok_or(CliError::CosignEnvelopeShape)?;
    for entry in signatures.iter() {
        let existing_key_id = entry
            .get("key_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if existing_key_id == signing_key_id.as_str() {
            return Err(CliError::CosignDuplicateKeyId {
                actor: actor_id.clone(),
                key_id: signing_key_id.to_string(),
            });
        }
    }

    let sig_bytes = secret.sign(&canonical);
    signatures.push(serde_json::json!({
        "actor": actor_id.to_string(),
        "key_id": signing_key_id.to_string(),
        "algorithm": "ed25519",
        "bytes": STANDARD.encode(sig_bytes.bytes()),
    }));

    let have = signatures.len();
    let serialized =
        serde_json::to_vec_pretty(&envelope).map_err(CliError::SerializePreparedEnvelope)?;
    std::fs::write(&prepared, &serialized).map_err(|source| {
        CliError::WritePreparedEnvelope {
            path: prepared.clone(),
            source,
        }
    })?;

    let need = ActorResolver::attestation_threshold_at(&store, &actor_id, created_at)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?
        .unwrap_or(1);

    Ok(format!(
        "co-signed envelope\nactor = {actor_id}\nkey_id = {signing_key_id}\nsignatures = {have}/{need}\nenvelope = {}\n",
        prepared.display(),
    ))
}

/// Read an attestation seed file (single line of base64) and build a
/// `SecretSigningKey` from it. The decoded bytes leave process memory
/// once the secret is constructed.
fn read_attestation_seed(path: &Path) -> Result<SecretSigningKey, CliError> {
    let raw =
        std::fs::read_to_string(path).map_err(|source| CliError::ReadAttestationSeed {
            path: path.to_path_buf(),
            source,
        })?;
    let trimmed = raw.trim();
    let decoded = STANDARD
        .decode(trimmed)
        .map_err(|_| CliError::InvalidAttestationSeedBase64 {
            path: path.to_path_buf(),
        })?;
    let bytes = <[u8; 32]>::try_from(decoded.as_slice()).map_err(|_| {
        CliError::InvalidAttestationSeedLength {
            path: path.to_path_buf(),
            actual: decoded.len(),
        }
    })?;
    Ok(SecretSigningKey::ed25519(bytes))
}

/// Read a base64-encoded ed25519 signature file.
fn read_signature_bytes(path: &Path) -> Result<[u8; 64], CliError> {
    let raw =
        std::fs::read_to_string(path).map_err(|source| CliError::ReadSignatureFile {
            path: path.to_path_buf(),
            source,
        })?;
    let trimmed = raw.trim();
    let decoded = STANDARD
        .decode(trimmed)
        .map_err(|_| CliError::InvalidSignatureBase64Path {
            path: path.to_path_buf(),
        })?;
    <[u8; 64]>::try_from(decoded.as_slice()).map_err(|_| CliError::InvalidSignatureLength {
        path: path.to_path_buf(),
        actual: decoded.len(),
    })
}

fn payload_path_for(envelope_path: &Path) -> PathBuf {
    let mut payload = envelope_path.as_os_str().to_owned();
    payload.push(".payload");
    PathBuf::from(payload)
}

/// Walk the actor's rotation chain and return the chain leaf's
/// `StatementId` to use as `supersedes` for a new rotation. Returns
/// `None` for an actor that has never rotated (genesis-initial is
/// implicit; first rotation has `supersedes = None`).
fn latest_rotation_supersedes(
    store: &FilesystemStore,
    actor_id: &ActorId,
) -> Result<Option<kairo_core::StatementId>, CliError> {
    let rotations = ActorResolver::key_rotations(store, actor_id).map_err(|error| {
        CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        }
    })?;
    let leaf = rotations.into_iter().max_by(|a, b| {
        a.created_at
            .seconds()
            .cmp(&b.created_at.seconds())
            .then_with(|| a.statement_id.cmp(&b.statement_id))
    });
    match leaf {
        None => Ok(None),
        Some(entry) => Ok(Some(
            kairo_core::StatementId::new(entry.statement_id).map_err(|source| {
                CliError::ParseStatementId {
                    statement: "(rotation chain leaf)".to_owned(),
                    source,
                }
            })?,
        )),
    }
}

fn run_actor_rotate_key(paths: &StorePaths, actor: String) -> Result<String, CliError> {
    let actor_id = ActorId::new(actor.clone())
        .map_err(|source| CliError::ParseActorId { actor, source })?;

    let store = open_store(paths)?;
    let keystore = open_keystore(paths)?;

    // Confirm the actor exists and pull the current active key.
    let _ = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;
    let now = Timestamp::now();
    let active_key = ActorResolver::active_key_at(&store, &actor_id, now)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?
        .ok_or_else(|| CliError::ActorHasNoActiveKey {
            actor: actor_id.clone(),
        })?;

    // The keystore must hold the secret matching the current active
    // key — otherwise we can't sign the rotation.
    let prior_secret = keystore
        .get_signing_key(&actor_id)
        .map_err(|error| CliError::ReadKey {
            actor: actor_id.clone(),
            source: error,
        })?;
    if prior_secret.public_key() != active_key {
        return Err(CliError::KeyDoesNotMatchActor { actor: actor_id });
    }

    // Auto-chain: if any prior key event exists for this actor, the
    // new rotation supersedes the most-recent rotation chain leaf.
    // Genesis-initial is implicit — `supersedes = None` for the first
    // rotation.
    let supersedes = ActorResolver::key_rotations(&store, &actor_id)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?
        .into_iter()
        .max_by(|a, b| {
            a.created_at
                .seconds()
                .cmp(&b.created_at.seconds())
                .then_with(|| a.statement_id.cmp(&b.statement_id))
        })
        .map(|entry| {
            kairo_core::StatementId::new(entry.statement_id).map_err(|source| {
                CliError::ParseStatementId {
                    statement: "(rotation chain leaf)".to_owned(),
                    source,
                }
            })
        })
        .transpose()?;

    let new_secret = SecretSigningKey::generate_ed25519().map_err(CliError::GenerateKey)?;
    let body = ActorKeyRotationBody::new(new_secret.public_key(), supersedes.clone());
    let subject: KairoRef = format!("actor:{actor_id}").parse().map_err(|source| {
        CliError::BuildActorSubjectRef {
            actor: actor_id.clone(),
            source,
        }
    })?;
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, now, body);
    let signature_bytes = prior_secret.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        actor_id.clone(),
        prior_secret.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    let signed = SignedStatement::new(unsigned, signature);
    let statement_id = signed.statement_id();

    store
        .put_actor_key_rotation(&signed)
        .map_err(|error| CliError::WriteKeyRotation {
            statement: statement_id.clone(),
            source: error,
        })?;

    // Replace the keystore entry so future signing uses the new key.
    let new_key_id = keystore
        .replace_signing_key(&actor_id, &new_secret)
        .map_err(|error| CliError::WriteKey {
            actor: actor_id.clone(),
            source: error,
        })?;

    let supersedes_line = match supersedes {
        Some(id) => format!("supersedes = {id}\n"),
        None => "supersedes = (genesis)\n".to_owned(),
    };
    Ok(format!(
        "rotated key\nstatement = {statement_id}\nactor = {actor_id}\nprior_key_id = {}\nnext_key_id = {new_key_id}\n{supersedes_line}",
        prior_secret.public_key().key_id()
    ))
}

fn run_actor_revoke_key(
    paths: &StorePaths,
    actor: String,
    key_id: String,
    retroactive: bool,
    reason: Option<String>,
    brick_actor: bool,
) -> Result<String, CliError> {
    let actor_id = ActorId::new(actor.clone())
        .map_err(|source| CliError::ParseActorId { actor, source })?;
    let revoked_key = KeyId::new(key_id);

    let store = open_store(paths)?;
    let keystore = open_keystore(paths)?;

    let _ = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;
    let now = Timestamp::now();
    let active_key = ActorResolver::active_key_at(&store, &actor_id, now)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?
        .ok_or_else(|| CliError::ActorHasNoActiveKey {
            actor: actor_id.clone(),
        })?;
    let active_key_id = active_key.key_id();

    // Bricking guard (`ACTORS.md` §5.5.1): if the operator is
    // revoking the only key they hold, refuse without an explicit
    // opt-in. The "only key" test is "no rotations have happened",
    // i.e. the active key is the genesis-initial key.
    let rotations = ActorResolver::key_rotations(&store, &actor_id).map_err(|error| {
        CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        }
    })?;
    let revoking_active_key = revoked_key == active_key_id;
    let only_active_key = rotations.is_empty();
    if revoking_active_key && only_active_key && !brick_actor {
        return Err(CliError::WouldBrickActor {
            actor: actor_id,
            key_id: revoked_key,
        });
    }

    let signing_secret = keystore
        .get_signing_key(&actor_id)
        .map_err(|error| CliError::ReadKey {
            actor: actor_id.clone(),
            source: error,
        })?;
    if signing_secret.public_key() != active_key {
        return Err(CliError::KeyDoesNotMatchActor { actor: actor_id });
    }

    let body = ActorKeyRevocationBody::new(revoked_key.clone(), retroactive, reason);
    let subject: KairoRef = format!("actor:{actor_id}").parse().map_err(|source| {
        CliError::BuildActorSubjectRef {
            actor: actor_id.clone(),
            source,
        }
    })?;
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, now, body);
    let signature_bytes = signing_secret.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        actor_id.clone(),
        signing_secret.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    let signed = SignedStatement::new(unsigned, signature);
    let statement_id = signed.statement_id();

    store
        .put_actor_key_revocation(&signed)
        .map_err(|error| CliError::WriteKeyRevocation {
            statement: statement_id.clone(),
            source: error,
        })?;

    let reason_line = match signed.unsigned().body().reason() {
        Some(reason) => format!("reason = {reason}\n"),
        None => String::new(),
    };
    Ok(format!(
        "revoked key\nstatement = {statement_id}\nactor = {actor_id}\nrevoked_key = {revoked_key}\nretroactive = {retroactive}\n{reason_line}"
    ))
}

fn run_actor_key_history(paths: &StorePaths, actor: String, json: bool) -> Result<String, CliError> {
    let actor_id = ActorId::new(actor.clone())
        .map_err(|source| CliError::ParseActorId { actor, source })?;

    let store = open_store(paths)?;
    let actor_body = store
        .get_actor(&actor_id)
        .map_err(|error| CliError::ReadActor {
            actor: actor_id.clone(),
            source: error,
        })?;
    let rotations = ActorResolver::key_rotations(&store, &actor_id).map_err(|error| {
        CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        }
    })?;
    let revocations = ActorResolver::key_revocations(&store, &actor_id).map_err(|error| {
        CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        }
    })?;
    let attestation_adds = ActorResolver::attestation_key_adds(&store, &actor_id).map_err(
        |error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        },
    )?;
    let attestation_revocations =
        ActorResolver::attestation_key_revocations(&store, &actor_id).map_err(|error| {
            CliError::ReadActiveKey {
                actor: actor_id.clone(),
                source: error,
            }
        })?;
    let mut threshold_changes = ActorResolver::attestation_threshold_changes(&store, &actor_id)
        .map_err(|error| CliError::ReadActiveKey {
            actor: actor_id.clone(),
            source: error,
        })?;
    threshold_changes.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.statement_id.cmp(&b.statement_id))
    });

    // Walk threshold_changes in causal order, recomputing the
    // "quorum at this event" — the count of distinct attestation
    // signatures that *would* have been required to authorize the
    // event under the asymmetric rule (max(current, new) for raises;
    // current for lowers/equal). Emitted alongside each change so
    // operators can audit the path from the genesis threshold
    // forward without re-deriving it themselves.
    let genesis_threshold = actor_body.attestation_threshold();
    let mut current_threshold = genesis_threshold;
    let trajectory: Vec<(
        &kairo_identity::AttestationThresholdChangeEntry,
        u8,
        u8,
    )> = threshold_changes
        .iter()
        .map(|entry| {
            let from = current_threshold;
            let quorum = if entry.new_threshold > from {
                entry.new_threshold
            } else {
                from
            };
            current_threshold = entry.new_threshold;
            (entry, from, quorum)
        })
        .collect();

    if json {
        let value = serde_json::json!({
            "actor": actor_id.to_string(),
            "genesis_key_id": actor_body.initial_key().key_id().to_string(),
            "genesis_attestation_keys": actor_body
                .attestation_keys()
                .iter()
                .map(|key| key.key_id().to_string())
                .collect::<Vec<_>>(),
            "genesis_attestation_threshold": genesis_threshold,
            "current_attestation_threshold": current_threshold,
            "rotations": rotations
                .iter()
                .map(|entry| serde_json::json!({
                    "statement_id": entry.statement_id,
                    "next_key_id": entry.next_key.key_id().to_string(),
                    "created_at": entry.created_at.to_string(),
                    "supersedes": entry.supersedes,
                    "surface": surface_str(entry.surface),
                }))
                .collect::<Vec<_>>(),
            "revocations": revocations
                .iter()
                .map(|entry| serde_json::json!({
                    "statement_id": entry.statement_id,
                    "revoked_key": entry.revoked_key.to_string(),
                    "retroactive": entry.retroactive,
                    "created_at": entry.created_at.to_string(),
                    "surface": surface_str(entry.surface),
                }))
                .collect::<Vec<_>>(),
            "attestation_adds": attestation_adds
                .iter()
                .map(|entry| serde_json::json!({
                    "statement_id": entry.statement_id,
                    "new_attestation_key_id": entry.new_key.key_id().to_string(),
                    "created_at": entry.created_at.to_string(),
                }))
                .collect::<Vec<_>>(),
            "attestation_revocations": attestation_revocations
                .iter()
                .map(|entry| serde_json::json!({
                    "statement_id": entry.statement_id,
                    "revoked_key": entry.revoked_key.to_string(),
                    "created_at": entry.created_at.to_string(),
                }))
                .collect::<Vec<_>>(),
            "attestation_threshold_changes": trajectory
                .iter()
                .map(|(entry, from, quorum)| serde_json::json!({
                    "statement_id": entry.statement_id,
                    "from": from,
                    "to": entry.new_threshold,
                    "created_at": entry.created_at.to_string(),
                    "quorum_at_event": quorum,
                }))
                .collect::<Vec<_>>(),
        });
        let mut output = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
        output.push('\n');
        return Ok(output);
    }

    let mut out = String::new();
    out.push_str(&format!("actor = {actor_id}\n"));
    out.push_str(&format!(
        "genesis_key_id = {}\n",
        actor_body.initial_key().key_id()
    ));
    out.push_str(&format!(
        "genesis_attestation_keys = {}\n",
        actor_body.attestation_keys().len()
    ));
    for key in actor_body.attestation_keys() {
        out.push_str(&format!("  - {}\n", key.key_id()));
    }
    out.push_str(&format!(
        "genesis_attestation_threshold = {genesis_threshold}\n"
    ));
    out.push_str(&format!(
        "current_attestation_threshold = {current_threshold}\n"
    ));
    out.push_str(&format!("rotations = {}\n", rotations.len()));
    for entry in &rotations {
        out.push_str(&format!(
            "  - statement = {}\n    next_key_id = {}\n    created_at = {}\n    supersedes = {}\n    surface = {}\n",
            entry.statement_id,
            entry.next_key.key_id(),
            entry.created_at,
            entry.supersedes.as_deref().unwrap_or("(genesis)"),
            surface_str(entry.surface),
        ));
    }
    out.push_str(&format!("revocations = {}\n", revocations.len()));
    for entry in &revocations {
        out.push_str(&format!(
            "  - statement = {}\n    revoked_key = {}\n    retroactive = {}\n    created_at = {}\n    surface = {}\n",
            entry.statement_id,
            entry.revoked_key,
            entry.retroactive,
            entry.created_at,
            surface_str(entry.surface),
        ));
    }
    out.push_str(&format!("attestation_adds = {}\n", attestation_adds.len()));
    for entry in &attestation_adds {
        out.push_str(&format!(
            "  - statement = {}\n    new_attestation_key_id = {}\n    created_at = {}\n",
            entry.statement_id,
            entry.new_key.key_id(),
            entry.created_at,
        ));
    }
    out.push_str(&format!(
        "attestation_revocations = {}\n",
        attestation_revocations.len()
    ));
    for entry in &attestation_revocations {
        out.push_str(&format!(
            "  - statement = {}\n    revoked_key = {}\n    created_at = {}\n",
            entry.statement_id, entry.revoked_key, entry.created_at,
        ));
    }
    out.push_str(&format!(
        "attestation_threshold_changes = {}\n",
        trajectory.len()
    ));
    for (entry, from, quorum) in &trajectory {
        out.push_str(&format!(
            "  - statement = {}\n    from = {}\n    to = {}\n    created_at = {}\n    quorum_at_event = {}\n",
            entry.statement_id, from, entry.new_threshold, entry.created_at, quorum,
        ));
    }
    Ok(out)
}

fn surface_str(surface: kairo_identity::KeySurface) -> &'static str {
    match surface {
        kairo_identity::KeySurface::Operational => "operational",
        kairo_identity::KeySurface::Attestation => "attestation",
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

            store
                .get_actor(&actor_id)
                .map_err(|error| CliError::ReadActor {
                    actor: actor_id.clone(),
                    source: error,
                })?;
            let secret = require_active_signing_key(&store, &keystore, &actor_id)?;

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


fn run_revision_command(command: RevisionCommand, paths: &StorePaths) -> Result<String, CliError> {
    match command {
        RevisionCommand::ValidateManifest {
            statement,
            manifest,
        } => {
            let statement = read_object_revision_statement(statement)?;
            let revision = statement.unsigned().body();
            let manifest = commands::manifest::read_manifest(manifest)?;
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

            store
                .get_actor(&actor_id)
                .map_err(|error| CliError::ReadActor {
                    actor: actor_id.clone(),
                    source: error,
                })?;
            let secret = require_active_signing_key(&store, &keystore, &actor_id)?;

            let parsed_manifest = commands::manifest::read_manifest(manifest)?;
            let manifest_hash = parsed_manifest.manifest_hash();

            if let Some(declared) = parsed_manifest.kairo().object() {
                if declared != &object_id {
                    return Err(CliError::ManifestObjectMismatch {
                        manifest_object: declared.clone(),
                        cli_object: object_id,
                    });
                }
            }

            // Persist the manifest blob alongside the revision so the
            // store carries everything signed-into the revision (and
            // bundle export can ship it). Idempotent: re-writing the
            // same canonical bytes under the same BlobId is a no-op
            // at the byte level.
            let manifest_canonical_bytes = parsed_manifest.canonical_bytes();
            store
                .put_blob(&manifest_hash, &manifest_canonical_bytes)
                .map_err(|error| CliError::WriteBlob {
                    blob: manifest_hash.clone(),
                    source: error,
                })?;

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

            store
                .get_actor(&actor_id)
                .map_err(|error| CliError::ReadActor {
                    actor: actor_id.clone(),
                    source: error,
                })?;
            let secret = require_active_signing_key(&store, &keystore, &actor_id)?;

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

            // Auto-chain: if the actor already has a head for this branch
            // name, supersede it; otherwise this is the genesis advance.
            let supersedes = store
                .latest_branch(&actor_id, &object_id, &name)
                .map_err(CliError::ReadBranch)?
                .map(|signed| signed.statement_id());

            let body = ObjectBranchBody::new(
                object_id.clone(),
                name.clone(),
                revision_id.clone(),
                supersedes.clone(),
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
                .put_object_branch(&signed)
                .map_err(|error| CliError::WriteBranch {
                    statement: statement_id.clone(),
                    source: error,
                })?;

            let supersedes_line = match supersedes {
                Some(id) => format!("supersedes = {id}\n"),
                None => "supersedes = (genesis)\n".to_owned(),
            };
            Ok(format!(
                "set branch\nstatement = {statement_id}\nobject = {object_id}\nactor = {actor_id}\nname = {name}\nrevision = {revision_id}\n{supersedes_line}"
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

fn run_tag_command(command: TagCommand, paths: &StorePaths) -> Result<String, CliError> {
    match command {
        TagCommand::Bind {
            actor,
            object,
            version,
            revision,
        } => {
            let actor_id = ActorId::new(actor.clone())
                .map_err(|source| CliError::ParseActorId { actor, source })?;
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let semver = SemverVersion::parse(&version).map_err(CliError::ParseSemver)?;
            let revision_id = kairo_core::StatementId::new(revision.clone()).map_err(|source| {
                CliError::ParseStatementId {
                    statement: revision,
                    source,
                }
            })?;

            let store = open_store(paths)?;
            let keystore = open_keystore(paths)?;

            store
                .get_actor(&actor_id)
                .map_err(|error| CliError::ReadActor {
                    actor: actor_id.clone(),
                    source: error,
                })?;
            let secret = require_active_signing_key(&store, &keystore, &actor_id)?;

            // Confirm the revision exists locally and binds to the same
            // object — fail fast rather than leaving a dangling tag.
            let pointed = store.get_object_revision(&revision_id).map_err(|error| {
                CliError::ReadRevision {
                    statement: revision_id.clone(),
                    source: error,
                }
            })?;
            if pointed.unsigned().body().object() != &object_id {
                return Err(CliError::TagObjectMismatch {
                    tag_object: object_id,
                    revision_object: pointed.unsigned().body().object().clone(),
                });
            }

            // Auto-chain: if the actor already has a head for this version,
            // supersede it; otherwise this is the genesis tag.
            let supersedes = store
                .latest_version_tag(&actor_id, &object_id, semver.as_str())
                .map_err(CliError::ReadVersionTag)?
                .map(|signed| signed.statement_id());

            let body = ObjectVersionTagBody::new(
                object_id.clone(),
                semver.clone(),
                Some(revision_id.clone()),
                supersedes.clone(),
            )
            .map_err(CliError::TagShape)?;
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
                .put_object_version_tag(&signed)
                .map_err(|error| CliError::WriteVersionTag {
                    statement: statement_id.clone(),
                    source: error,
                })?;

            let supersedes_line = match supersedes {
                Some(id) => format!("supersedes = {id}\n"),
                None => "supersedes = (genesis)\n".to_owned(),
            };
            Ok(format!(
                "bind tag\nstatement = {statement_id}\nobject = {object_id}\nactor = {actor_id}\nversion = {}\ntarget = {revision_id}\n{supersedes_line}",
                semver.as_str()
            ))
        }
        TagCommand::Revoke {
            actor,
            object,
            version,
        } => {
            let actor_id = ActorId::new(actor.clone())
                .map_err(|source| CliError::ParseActorId { actor, source })?;
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let semver = SemverVersion::parse(&version).map_err(CliError::ParseSemver)?;

            let store = open_store(paths)?;
            let keystore = open_keystore(paths)?;

            store
                .get_actor(&actor_id)
                .map_err(|error| CliError::ReadActor {
                    actor: actor_id.clone(),
                    source: error,
                })?;
            let secret = require_active_signing_key(&store, &keystore, &actor_id)?;

            // Revocation requires a prior tag to chain off of.
            let prior = store
                .latest_version_tag(&actor_id, &object_id, semver.as_str())
                .map_err(CliError::ReadVersionTag)?
                .ok_or_else(|| CliError::RevokeWithoutPriorTag {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    version: semver.as_str().to_owned(),
                })?;
            let supersedes_id = prior.statement_id();

            let body = ObjectVersionTagBody::new(
                object_id.clone(),
                semver.clone(),
                None,
                Some(supersedes_id.clone()),
            )
            .map_err(CliError::TagShape)?;
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
                .put_object_version_tag(&signed)
                .map_err(|error| CliError::WriteVersionTag {
                    statement: statement_id.clone(),
                    source: error,
                })?;

            Ok(format!(
                "revoke tag\nstatement = {statement_id}\nobject = {object_id}\nactor = {actor_id}\nversion = {}\ntarget = (revoked)\nsupersedes = {supersedes_id}\n",
                semver.as_str()
            ))
        }
        TagCommand::Show {
            object,
            actor,
            version,
            json,
        } => {
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let semver = SemverVersion::parse(&version).map_err(CliError::ParseSemver)?;
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
                .latest_version_tag(&actor_id, &object_id, semver.as_str())
                .map_err(CliError::ReadVersionTag)?;

            match resolved {
                Some(signed) => {
                    if json {
                        Ok(format_tag_show_json(&signed))
                    } else {
                        Ok(format_tag_show(&signed))
                    }
                }
                None => Err(CliError::TagNotFound {
                    actor: actor_id,
                    object: object_id,
                    version: semver.as_str().to_owned(),
                }),
            }
        }
        TagCommand::List { object } => {
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let store = open_store(paths)?;
            let heads = store
                .list_version_tags(&object_id)
                .map_err(CliError::ReadVersionTag)?;
            Ok(format_tag_list(&object_id, &heads))
        }
        TagCommand::History {
            object,
            actor,
            version,
            json,
        } => {
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let semver = SemverVersion::parse(&version).map_err(CliError::ParseSemver)?;
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

            let head = store
                .latest_version_tag(&actor_id, &object_id, semver.as_str())
                .map_err(CliError::ReadVersionTag)?
                .ok_or_else(|| CliError::TagNotFound {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    version: semver.as_str().to_owned(),
                })?;

            let chain = walk_tag_chain(&store, head)?;
            if json {
                Ok(format_tag_history_json(&actor_id, &object_id, semver.as_str(), &chain))
            } else {
                Ok(format_tag_history(&actor_id, &object_id, semver.as_str(), &chain))
            }
        }
    }
}

/// One link in a version tag history walk. `Indeterminate` marks the
/// point where the chain leaves the local store.
#[derive(Debug)]
enum TagChainLink {
    Statement(Box<SignedStatement<ObjectVersionTagBody>>),
    Indeterminate { missing: kairo_core::StatementId },
}

fn walk_tag_chain(
    store: &FilesystemStore,
    head: SignedStatement<ObjectVersionTagBody>,
) -> Result<Vec<TagChainLink>, CliError> {
    let mut chain = Vec::new();
    let mut next = Some(head);
    while let Some(signed) = next {
        let supersedes = signed.unsigned().body().supersedes().cloned();
        chain.push(TagChainLink::Statement(Box::new(signed)));
        match supersedes {
            Some(prior_id) => match store.get_object_version_tag(&prior_id) {
                Ok(prior) => next = Some(prior),
                Err(kairo_store::StoreError::Missing) => {
                    chain.push(TagChainLink::Indeterminate { missing: prior_id });
                    next = None;
                }
                Err(error) => return Err(CliError::ReadVersionTag(error)),
            },
            None => next = None,
        }
    }
    Ok(chain)
}

fn format_tag_show(signed: &SignedStatement<ObjectVersionTagBody>) -> String {
    let body = signed.unsigned().body();
    let target = match body.target() {
        Some(id) => id.to_string(),
        None => "(revoked)".to_owned(),
    };
    let supersedes = match body.supersedes() {
        Some(id) => id.to_string(),
        None => "(genesis)".to_owned(),
    };
    format!(
        "statement = {}\nobject = {}\nactor = {}\nversion = {}\ntarget = {target}\nsupersedes = {supersedes}\ncreated_at = {}\n",
        signed.statement_id(),
        body.object(),
        signed.unsigned().actor(),
        body.version().as_str(),
        signed.unsigned().created_at(),
    )
}

fn format_tag_show_json(signed: &SignedStatement<ObjectVersionTagBody>) -> String {
    let body = signed.unsigned().body();
    let value = serde_json::json!({
        "statement_id": signed.statement_id().to_string(),
        "actor": signed.unsigned().actor().to_string(),
        "object": body.object().to_string(),
        "version": body.version().as_str(),
        "target": body.target().map(|id| id.to_string()),
        "supersedes": body.supersedes().map(|id| id.to_string()),
        "is_revocation": body.is_revocation(),
        "created_at": signed.unsigned().created_at().to_string(),
    });
    let mut output = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    output.push('\n');
    output
}

fn format_tag_list(object: &ObjectId, heads: &[kairo_store::VersionTagHead]) -> String {
    let mut output = String::new();
    output.push_str(&format!("object = {object}\n"));
    output.push_str(&format!("tags = {}\n", heads.len()));
    for head in heads {
        output.push_str(&format!(
            "  actor={} version={} statement={} created_at={}\n",
            head.actor, head.version, head.statement_id, head.created_at,
        ));
    }
    output
}

fn format_tag_history(
    actor: &ActorId,
    object: &ObjectId,
    version: &str,
    chain: &[TagChainLink],
) -> String {
    let mut output = String::new();
    output.push_str(&format!("object = {object}\n"));
    output.push_str(&format!("actor = {actor}\n"));
    output.push_str(&format!("version = {version}\n"));
    output.push_str("history (newest -> oldest):\n");
    for (idx, link) in chain.iter().enumerate() {
        let n = idx + 1;
        match link {
            TagChainLink::Statement(signed) => {
                let body = signed.unsigned().body();
                let kind = if body.is_revocation() { "revoke" } else { "bind" };
                let target = match body.target() {
                    Some(id) => format!(" target={id}"),
                    None => String::new(),
                };
                let supersedes = match body.supersedes() {
                    Some(id) => format!(" supersedes={id}"),
                    None => " (genesis)".to_owned(),
                };
                output.push_str(&format!(
                    "  {n}. statement={} created_at={} kind={kind}{target}{supersedes}\n",
                    signed.statement_id(),
                    signed.unsigned().created_at(),
                ));
            }
            TagChainLink::Indeterminate { missing } => {
                output.push_str(&format!(
                    "  {n}. (missing) statement={missing} — chain truncated; import the predecessor to continue\n"
                ));
            }
        }
    }
    output
}

fn format_tag_history_json(
    actor: &ActorId,
    object: &ObjectId,
    version: &str,
    chain: &[TagChainLink],
) -> String {
    let entries: Vec<_> = chain
        .iter()
        .map(|link| match link {
            TagChainLink::Statement(signed) => {
                let body = signed.unsigned().body();
                serde_json::json!({
                    "kind": if body.is_revocation() { "revoke" } else { "bind" },
                    "statement_id": signed.statement_id().to_string(),
                    "target": body.target().map(|id| id.to_string()),
                    "supersedes": body.supersedes().map(|id| id.to_string()),
                    "created_at": signed.unsigned().created_at().to_string(),
                })
            }
            TagChainLink::Indeterminate { missing } => serde_json::json!({
                "kind": "indeterminate",
                "missing_statement_id": missing.to_string(),
            }),
        })
        .collect();
    let value = serde_json::json!({
        "actor": actor.to_string(),
        "object": object.to_string(),
        "version": version,
        "history": entries,
    });
    let mut output = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    output.push('\n');
    output
}

fn run_trust_command(command: TrustCommand, paths: &StorePaths) -> Result<String, CliError> {
    match command {
        TrustCommand::Grant { by, of, reason } => {
            run_trust_decide(paths, by, of, reason, Some(TrustDecision::Trusted), "grant")
        }
        TrustCommand::Block { by, of, reason } => {
            run_trust_decide(paths, by, of, reason, Some(TrustDecision::Untrusted), "block")
        }
        TrustCommand::Withdraw { by, of, reason } => {
            run_trust_decide(paths, by, of, reason, None, "withdraw")
        }
        TrustCommand::Show { by, of, json } => {
            let by_actor = ActorId::new(by.clone())
                .map_err(|source| CliError::ParseActorId { actor: by, source })?;
            let trusted_actor = ActorId::new(of.clone())
                .map_err(|source| CliError::ParseActorId { actor: of, source })?;
            let store = open_store(paths)?;
            let resolved = store
                .latest_trust(&by_actor, &trusted_actor)
                .map_err(CliError::ReadActorTrust)?;
            if json {
                Ok(format_trust_show_json(&by_actor, &trusted_actor, resolved.as_ref()))
            } else {
                Ok(format_trust_show(&by_actor, &trusted_actor, resolved.as_ref()))
            }
        }
        TrustCommand::List { by } => {
            let by_actor = ActorId::new(by.clone())
                .map_err(|source| CliError::ParseActorId { actor: by, source })?;
            let store = open_store(paths)?;
            let heads = store
                .list_trust(&by_actor)
                .map_err(CliError::ReadActorTrust)?;
            Ok(format_trust_list(&by_actor, &heads))
        }
        TrustCommand::History { by, of, json } => {
            let by_actor = ActorId::new(by.clone())
                .map_err(|source| CliError::ParseActorId { actor: by, source })?;
            let trusted_actor = ActorId::new(of.clone())
                .map_err(|source| CliError::ParseActorId { actor: of, source })?;
            let store = open_store(paths)?;
            let head = store
                .latest_trust(&by_actor, &trusted_actor)
                .map_err(CliError::ReadActorTrust)?;
            let chain = match head {
                Some(signed) => walk_trust_chain(&store, signed)?,
                None => Vec::new(),
            };
            if json {
                Ok(format_trust_history_json(&by_actor, &trusted_actor, &chain))
            } else {
                Ok(format_trust_history(&by_actor, &trusted_actor, &chain))
            }
        }
    }
}

fn run_trust_decide(
    paths: &StorePaths,
    by: String,
    of: String,
    reason: Option<String>,
    decision: Option<TrustDecision>,
    label: &str,
) -> Result<String, CliError> {
    let by_actor = ActorId::new(by.clone())
        .map_err(|source| CliError::ParseActorId { actor: by, source })?;
    let trusted_actor = ActorId::new(of.clone())
        .map_err(|source| CliError::ParseActorId { actor: of, source })?;

    let store = open_store(paths)?;
    let keystore = open_keystore(paths)?;

    store
        .get_actor(&by_actor)
        .map_err(|error| CliError::ReadActor {
            actor: by_actor.clone(),
            source: error,
        })?;
    let secret = require_active_signing_key(&store, &keystore, &by_actor)?;

    // Auto-chain: if the truster already has a head about this trusted
    // actor, supersede it; otherwise this is the genesis opinion.
    // Withdrawal additionally requires a prior head.
    let prior = store
        .latest_trust(&by_actor, &trusted_actor)
        .map_err(CliError::ReadActorTrust)?;
    let supersedes = prior.as_ref().map(|signed| signed.statement_id());
    if decision.is_none() && supersedes.is_none() {
        return Err(CliError::WithdrawWithoutPriorTrust {
            by_actor,
            trusted_actor,
        });
    }

    let body = ActorTrustBody::new(trusted_actor.clone(), decision, reason, supersedes.clone())
        .map_err(CliError::TrustShape)?;
    let subject: KairoRef = format!("actor:{trusted_actor}").parse().map_err(|source| {
        CliError::BuildActorSubjectRef {
            actor: trusted_actor.clone(),
            source,
        }
    })?;
    let unsigned = UnsignedStatement::new(by_actor.clone(), subject, Timestamp::now(), body);
    let signature_bytes = secret.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        by_actor.clone(),
        secret.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    let signed = SignedStatement::new(unsigned, signature);
    let statement_id = signed.statement_id();

    store
        .put_actor_trust(&signed)
        .map_err(|error| CliError::WriteActorTrust {
            statement: statement_id.clone(),
            source: error,
        })?;

    let supersedes_line = match supersedes {
        Some(id) => format!("supersedes = {id}\n"),
        None => "supersedes = (genesis)\n".to_owned(),
    };
    let decision_line = match signed.unsigned().body().decision() {
        Some(d) => d.as_str(),
        None => "(withdrawn)",
    };
    Ok(format!(
        "{label} trust\nstatement = {statement_id}\nby_actor = {by_actor}\ntrusted_actor = {trusted_actor}\ndecision = {decision_line}\n{supersedes_line}",
    ))
}

/// One link in a trust history walk. `Indeterminate` marks the point
/// where the chain leaves the local store.
#[derive(Debug)]
enum TrustChainLink {
    Statement(Box<SignedStatement<ActorTrustBody>>),
    Indeterminate { missing: kairo_core::StatementId },
}

fn walk_trust_chain(
    store: &FilesystemStore,
    head: SignedStatement<ActorTrustBody>,
) -> Result<Vec<TrustChainLink>, CliError> {
    let mut chain = Vec::new();
    let mut next = Some(head);
    while let Some(signed) = next {
        let supersedes = signed.unsigned().body().supersedes().cloned();
        chain.push(TrustChainLink::Statement(Box::new(signed)));
        match supersedes {
            Some(prior_id) => match store.get_actor_trust(&prior_id) {
                Ok(prior) => next = Some(prior),
                Err(kairo_store::StoreError::Missing) => {
                    chain.push(TrustChainLink::Indeterminate { missing: prior_id });
                    next = None;
                }
                Err(error) => return Err(CliError::ReadActorTrust(error)),
            },
            None => next = None,
        }
    }
    Ok(chain)
}

fn format_trust_show(
    by_actor: &ActorId,
    trusted_actor: &ActorId,
    resolved: Option<&SignedStatement<ActorTrustBody>>,
) -> String {
    match resolved {
        None => format!(
            "by_actor = {by_actor}\ntrusted_actor = {trusted_actor}\ndecision = unknown\n"
        ),
        Some(signed) => {
            let body = signed.unsigned().body();
            let decision = match body.decision() {
                Some(d) => d.as_str(),
                None => "unknown",
            };
            let supersedes = match body.supersedes() {
                Some(id) => id.to_string(),
                None => "(genesis)".to_owned(),
            };
            let reason = match body.reason() {
                Some(r) => format!("reason = {r}\n"),
                None => String::new(),
            };
            format!(
                "statement = {}\nby_actor = {by_actor}\ntrusted_actor = {trusted_actor}\ndecision = {decision}\nsupersedes = {supersedes}\ncreated_at = {}\n{reason}",
                signed.statement_id(),
                signed.unsigned().created_at(),
            )
        }
    }
}

fn format_trust_show_json(
    by_actor: &ActorId,
    trusted_actor: &ActorId,
    resolved: Option<&SignedStatement<ActorTrustBody>>,
) -> String {
    let value = match resolved {
        None => serde_json::json!({
            "by_actor": by_actor.to_string(),
            "trusted_actor": trusted_actor.to_string(),
            "decision": "unknown",
            "statement_id": null,
        }),
        Some(signed) => {
            let body = signed.unsigned().body();
            serde_json::json!({
                "statement_id": signed.statement_id().to_string(),
                "by_actor": by_actor.to_string(),
                "trusted_actor": trusted_actor.to_string(),
                "decision": body.decision().map(|d| d.as_str()),
                "supersedes": body.supersedes().map(|id| id.to_string()),
                "reason": body.reason(),
                "is_withdrawal": body.is_withdrawal(),
                "created_at": signed.unsigned().created_at().to_string(),
            })
        }
    };
    let mut output = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    output.push('\n');
    output
}

fn format_trust_list(by_actor: &ActorId, heads: &[kairo_store::TrustHead]) -> String {
    let mut output = String::new();
    output.push_str(&format!("by_actor = {by_actor}\n"));
    output.push_str(&format!("opinions = {}\n", heads.len()));
    for head in heads {
        let decision = head.decision.as_deref().unwrap_or("unknown");
        output.push_str(&format!(
            "  trusted_actor={} decision={decision} statement={} created_at={}\n",
            head.trusted_actor, head.statement_id, head.created_at,
        ));
    }
    output
}

fn format_trust_history(
    by_actor: &ActorId,
    trusted_actor: &ActorId,
    chain: &[TrustChainLink],
) -> String {
    let mut output = String::new();
    output.push_str(&format!("by_actor = {by_actor}\n"));
    output.push_str(&format!("trusted_actor = {trusted_actor}\n"));
    output.push_str(&format!("history (newest -> oldest, {} entries):\n", chain.len()));
    for (idx, link) in chain.iter().enumerate() {
        let n = idx + 1;
        match link {
            TrustChainLink::Statement(signed) => {
                let body = signed.unsigned().body();
                let kind = match body.decision() {
                    Some(TrustDecision::Trusted) => "grant",
                    Some(TrustDecision::Untrusted) => "block",
                    None => "withdraw",
                };
                let supersedes = match body.supersedes() {
                    Some(id) => format!(" supersedes={id}"),
                    None => " (genesis)".to_owned(),
                };
                output.push_str(&format!(
                    "  {n}. statement={} created_at={} kind={kind}{supersedes}\n",
                    signed.statement_id(),
                    signed.unsigned().created_at(),
                ));
            }
            TrustChainLink::Indeterminate { missing } => {
                output.push_str(&format!(
                    "  {n}. (missing) statement={missing} — chain truncated; import the predecessor to continue\n"
                ));
            }
        }
    }
    output
}

fn format_trust_history_json(
    by_actor: &ActorId,
    trusted_actor: &ActorId,
    chain: &[TrustChainLink],
) -> String {
    let entries: Vec<_> = chain
        .iter()
        .map(|link| match link {
            TrustChainLink::Statement(signed) => {
                let body = signed.unsigned().body();
                let kind = match body.decision() {
                    Some(TrustDecision::Trusted) => "grant",
                    Some(TrustDecision::Untrusted) => "block",
                    None => "withdraw",
                };
                serde_json::json!({
                    "kind": kind,
                    "statement_id": signed.statement_id().to_string(),
                    "decision": body.decision().map(|d| d.as_str()),
                    "supersedes": body.supersedes().map(|id| id.to_string()),
                    "reason": body.reason(),
                    "created_at": signed.unsigned().created_at().to_string(),
                })
            }
            TrustChainLink::Indeterminate { missing } => serde_json::json!({
                "kind": "indeterminate",
                "missing_statement_id": missing.to_string(),
            }),
        })
        .collect();
    let value = serde_json::json!({
        "by_actor": by_actor.to_string(),
        "trusted_actor": trusted_actor.to_string(),
        "history": entries,
    });
    let mut output = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    output.push('\n');
    output
}

fn run_capability_command(
    command: CapabilityCommand,
    paths: &StorePaths,
) -> Result<String, CliError> {
    match command {
        CapabilityCommand::Grant {
            grantor,
            grantee,
            object,
            kinds,
            delegable,
            expires_at,
            max_delegation_depth,
            key_pinned,
        } => run_capability_grant(
            paths,
            grantor,
            grantee,
            object,
            kinds,
            delegable,
            expires_at,
            max_delegation_depth,
            key_pinned,
        ),
        CapabilityCommand::Revoke {
            grantor,
            grant,
            retroactive,
            reason,
        } => run_capability_revoke(paths, grantor, grant, retroactive, reason),
        CapabilityCommand::List { grantor, object } => {
            run_capability_list(paths, grantor, object)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_capability_grant(
    paths: &StorePaths,
    grantor: String,
    grantee: String,
    object: String,
    kinds: Vec<String>,
    delegable: bool,
    expires_at: Option<String>,
    max_delegation_depth: Option<u8>,
    key_pinned: Option<String>,
) -> Result<String, CliError> {
    let grantor_id = ActorId::new(grantor.clone())
        .map_err(|source| CliError::ParseActorId { actor: grantor, source })?;
    let grantee_id = ActorId::new(grantee.clone())
        .map_err(|source| CliError::ParseActorId { actor: grantee, source })?;
    let object_id = ObjectId::new(object.clone())
        .map_err(|source| CliError::ParseObjectId { object, source })?;

    if kinds.is_empty() {
        return Err(CliError::CapabilityKindsRequired);
    }
    let mut parsed_kinds: Vec<StatementKind> = Vec::with_capacity(kinds.len());
    for kind in &kinds {
        let parsed = StatementKind::parse(kind)
            .map_err(|source| CliError::ParseStatementKind { kind: kind.clone(), source })?;
        parsed_kinds.push(parsed);
    }

    let mut constraints: Vec<CapabilityConstraint> = Vec::new();
    if let Some(expires_at) = expires_at {
        let ts: Timestamp = expires_at
            .parse()
            .map_err(|source| CliError::ParseTimestamp { value: expires_at, source })?;
        constraints.push(CapabilityConstraint::ExpiresAt(ts));
    }
    if let Some(depth) = max_delegation_depth {
        constraints.push(CapabilityConstraint::MaxDelegationDepth(depth));
    }
    if let Some(key_id) = key_pinned {
        constraints.push(CapabilityConstraint::KeyPinned(kairo_identity::KeyId::new(
            key_id,
        )));
    }

    let scope = CapabilityScope::Object(object_id.clone());
    let capability = Capability::new(scope.clone(), parsed_kinds, delegable, constraints)
        .map_err(CliError::CapabilityShape)?;

    let store = open_store(paths)?;
    let keystore = open_keystore(paths)?;

    store
        .get_actor(&grantor_id)
        .map_err(|error| CliError::ReadActor {
            actor: grantor_id.clone(),
            source: error,
        })?;
    let secret = require_active_signing_key(&store, &keystore, &grantor_id)?;

    // Auto-chain: supersede the existing chain leaf for (grantor,
    // grantee, scope) if any; otherwise this is the genesis grant.
    let prior = store
        .latest_capability(&grantor_id, &grantee_id, &scope)
        .map_err(CliError::ReadCapability)?;
    let supersedes = prior.as_ref().map(|signed| signed.statement_id());

    let body = ActorCapabilityGrantBody::new(grantee_id.clone(), capability, supersedes.clone());
    let subject: KairoRef = format!("actor:{grantee_id}")
        .parse()
        .map_err(|source| CliError::BuildActorSubjectRef {
            actor: grantee_id.clone(),
            source,
        })?;
    let unsigned = UnsignedStatement::new(grantor_id.clone(), subject, Timestamp::now(), body);
    let signature_bytes = secret.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        grantor_id.clone(),
        secret.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    let signed = SignedStatement::new(unsigned, signature);
    let statement_id = signed.statement_id();

    store
        .put_actor_capability_grant(&signed)
        .map_err(|error| CliError::WriteCapabilityGrant {
            statement: statement_id.clone(),
            source: error,
        })?;

    let supersedes_line = match supersedes {
        Some(id) => format!("supersedes = {id}\n"),
        None => "supersedes = (genesis)\n".to_owned(),
    };
    let body = signed.unsigned().body();
    let cap = body.capability();
    let kinds_line = cap
        .statement_kinds()
        .iter()
        .map(StatementKind::as_str)
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "grant capability\nstatement = {statement_id}\ngrantor = {grantor_id}\ngrantee = {grantee_id}\nobject = {object_id}\nkinds = [{kinds_line}]\ndelegable = {}\n{supersedes_line}",
        cap.delegable()
    ))
}

fn run_capability_revoke(
    paths: &StorePaths,
    grantor: String,
    grant: String,
    retroactive: bool,
    reason: Option<String>,
) -> Result<String, CliError> {
    let grantor_id = ActorId::new(grantor.clone())
        .map_err(|source| CliError::ParseActorId { actor: grantor, source })?;
    let grant_id = kairo_core::StatementId::new(grant.clone())
        .map_err(|source| CliError::ParseStatementId { statement: grant, source })?;

    let store = open_store(paths)?;
    let keystore = open_keystore(paths)?;

    // The grant must exist locally and have been signed by --grantor
    // (cross-grantor revocation is invalid in v1).
    let prior = store
        .get_actor_capability_grant(&grant_id)
        .map_err(|error| CliError::ReadGrant {
            statement: grant_id.clone(),
            source: error,
        })?;
    if prior.unsigned().actor() != &grantor_id {
        return Err(CliError::RevokeWrongGrantor {
            grant: grant_id,
            expected: prior.unsigned().actor().clone(),
            got: grantor_id,
        });
    }

    store
        .get_actor(&grantor_id)
        .map_err(|error| CliError::ReadActor {
            actor: grantor_id.clone(),
            source: error,
        })?;
    let secret = require_active_signing_key(&store, &keystore, &grantor_id)?;

    let body = ActorCapabilityRevocationBody::new(grant_id.clone(), retroactive, reason);
    let subject: KairoRef = format!("statement:{grant_id}")
        .parse()
        .map_err(|source| CliError::BuildStatementSubjectRef {
            statement: grant_id.clone(),
            source,
        })?;
    let unsigned = UnsignedStatement::new(grantor_id.clone(), subject, Timestamp::now(), body);
    let signature_bytes = secret.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        grantor_id.clone(),
        secret.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    let signed = SignedStatement::new(unsigned, signature);
    let statement_id = signed.statement_id();

    store
        .put_actor_capability_revocation(&signed)
        .map_err(|error| CliError::WriteCapabilityRevocation {
            statement: statement_id.clone(),
            source: error,
        })?;

    Ok(format!(
        "revoke capability\nstatement = {statement_id}\ngrantor = {grantor_id}\nrevoked_grant = {grant_id}\nretroactive = {retroactive}\n",
    ))
}

fn run_capability_list(
    paths: &StorePaths,
    grantor: Option<String>,
    object: Option<String>,
) -> Result<String, CliError> {
    match (grantor, object) {
        (Some(_), Some(_)) | (None, None) => Err(CliError::CapabilityListExclusive),
        (Some(grantor), None) => {
            let grantor_id = ActorId::new(grantor.clone())
                .map_err(|source| CliError::ParseActorId { actor: grantor, source })?;
            let store = open_store(paths)?;
            let heads = store
                .list_capabilities_from(&grantor_id)
                .map_err(CliError::ReadCapability)?;
            Ok(format_capability_list_by_grantor(&grantor_id, &heads))
        }
        (None, Some(object)) => {
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let store = open_store(paths)?;
            let heads = store
                .list_capabilities_for_object(&object_id)
                .map_err(CliError::ReadCapability)?;
            Ok(format_capability_list_by_object(&object_id, &heads))
        }
    }
}

fn format_capability_list_by_grantor(grantor: &ActorId, heads: &[CapabilityHead]) -> String {
    let mut output = format!("grantor = {grantor}\nheads = {}\n", heads.len());
    for (idx, head) in heads.iter().enumerate() {
        let scope_line = match &head.scope {
            CapabilityScope::Object(id) => format!("object = {id}"),
            CapabilityScope::Actor(id) => format!("actor = {id}"),
        };
        output.push_str(&format!(
            "\n[{}] grantee = {}\n    {scope_line}\n    statement = {}\n    created_at = {}\n",
            idx + 1,
            head.grantee,
            head.statement_id,
            head.created_at
        ));
    }
    output
}

fn format_capability_list_by_object(
    object: &ObjectId,
    heads: &[kairo_store::CapabilityByObjectHead],
) -> String {
    let mut output = format!("object = {object}\nheads = {}\n", heads.len());
    for (idx, head) in heads.iter().enumerate() {
        output.push_str(&format!(
            "\n[{}] grantor = {}\n    grantee = {}\n    statement = {}\n    created_at = {}\n",
            idx + 1,
            head.grantor,
            head.grantee,
            head.statement_id,
            head.created_at
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

/// Aggregated end-to-end verification result for an object, produced
/// by `kairo verify object`.
#[derive(Debug)]
struct ObjectVerificationReport {
    object: ObjectId,
    genesis: GenesisCheck,
    frontier: FrontierResolution,
    revision: RevisionChecks,
    overall: OverallStatus,
    /// Where the storage commit was looked up. Diagnostic only —
    /// does not contribute to `overall`. Surfaced as a single line
    /// in text output; `--json` shape unchanged for v1.
    repo_source: RepoSource,
}

/// Source of the Git repository consulted for the content-layer
/// check. Reported back to the user so they can tell whether
/// verify-object hit the managed cache, fell back to cwd, used an
/// explicit `--repo`, or skipped Git lookup entirely.
#[derive(Debug, Clone)]
enum RepoSource {
    /// Per-object bare repo in the managed Git cache contained the
    /// commit OID; verify ran against `<cache>/<XX>/<YY>/<id>/`.
    Cache { object: ObjectId },
    /// Either a cwd-discovered repo or an explicit `--repo <path>`.
    /// Both produce a `Repository` from a filesystem path, so the
    /// diagnostic only needs the path; the user's flags imply how
    /// it was found.
    Filesystem { path: PathBuf },
    /// No Git lookup performed: `--no-repo`, or cache miss with
    /// `--no-cwd-repo` set.
    Skipped,
}

#[derive(Debug)]
struct GenesisCheck {
    derived_object: ObjectId,
}

#[derive(Debug)]
enum FrontierResolution {
    BranchTip {
        actor: ActorId,
        name: String,
        statement: kairo_core::StatementId,
    },
    PinnedStatement {
        statement: kairo_core::StatementId,
    },
}

#[derive(Debug)]
struct RevisionChecks {
    statement_id: kairo_core::StatementId,
    revision: RevisionId,
    revision_object: ObjectId,
    signature: VerificationReport,
    validation: ObjectRevisionValidationReport,
    /// Where the manifest came from for the binding check. Either a
    /// filesystem path (explicit `--manifest`) or a synthetic
    /// `git:sha256:<oid>/kairo.toml` descriptor for tree-derived
    /// manifests. `None` when no manifest could be resolved.
    manifest_source: Option<String>,
    /// Truster used for the trust evaluation, if any. `None` when
    /// trust was skipped (`--no-as`) or when no local actor could be
    /// auto-picked from the keystore.
    truster: Option<ActorId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverallStatus {
    Valid,
    Indeterminate,
    Invalid,
}

impl OverallStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Valid => "VALID",
            Self::Indeterminate => "INDETERMINATE",
            Self::Invalid => "INVALID",
        }
    }
}

fn run_verify_command(command: VerifyCommand, paths: &StorePaths) -> Result<String, CliError> {
    match command {
        VerifyCommand::Object {
            object,
            statement,
            actor,
            name,
            r#as,
            no_as,
            repo,
            no_repo,
            no_cache,
            no_cwd_repo,
            manifest,
            json,
        } => {
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let store = open_store(paths)?;

            let genesis_statement =
                store
                    .get_object_genesis(&object_id)
                    .map_err(|error| CliError::ReadObjectGenesis {
                        object: object_id.clone(),
                        source: error,
                    })?;
            let genesis = GenesisCheck {
                derived_object: genesis_statement.object_id(),
            };

            // Resolve the chosen ObjectRevision and how we got there.
            let (revision_statement, frontier) = match statement {
                Some(statement) => {
                    let statement_id = kairo_core::StatementId::new(statement.clone())
                        .map_err(|source| CliError::ParseStatementId { statement, source })?;
                    let revision = store.get_object_revision(&statement_id).map_err(|error| {
                        CliError::ReadRevision {
                            statement: statement_id.clone(),
                            source: error,
                        }
                    })?;
                    (
                        revision,
                        FrontierResolution::PinnedStatement {
                            statement: statement_id,
                        },
                    )
                }
                None => {
                    let actor_id = match actor {
                        Some(actor) => ActorId::new(actor.clone())
                            .map_err(|source| CliError::ParseActorId { actor, source })?,
                        None => genesis_statement.body().created_by().clone(),
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
                    let revision = store.get_object_revision(&revision_statement_id).map_err(
                        |error| CliError::ReadRevision {
                            statement: revision_statement_id.clone(),
                            source: error,
                        },
                    )?;
                    (
                        revision,
                        FrontierResolution::BranchTip {
                            actor: actor_id,
                            name,
                            statement: revision_statement_id,
                        },
                    )
                }
            };

            let revision_body = revision_statement.unsigned().body();

            // Resolve the Git source. Precedence (per
            // `specs/DECISIONS.md` §9 and PHASE_2 §1):
            //   1. `--repo <path>`: that path, no fallbacks.
            //   2. `--no-repo`: skip everything.
            //   3. Default: cache first (if the per-object cache
            //      repo exists and contains the commit), then cwd
            //      discovery.
            let (git_repo, repo_source) = resolve_repo_for_verify(
                paths,
                &object_id,
                revision_body.revision(),
                repo.as_deref(),
                no_repo,
                no_cache,
                no_cwd_repo,
            )?;

            // Look up the storage commit. None = no repo or non-git
            // revision scheme. Some(NotFound) = repo present, commit
            // missing. Some(Found{...}) = commit details for the
            // content-layer check.
            let commit_lookup = match git_repo.as_ref() {
                Some(repo) => Some(lookup_commit_for_revision(repo, revision_body.revision())?),
                None => None,
            };

            // Resolve the manifest. Order of preference: explicit
            // --manifest override, then kairo.toml read from the
            // commit's tree, then nothing.
            let (manifest_value, manifest_source) = resolve_manifest(
                manifest.as_deref(),
                git_repo.as_ref(),
                revision_body.revision(),
                &commit_lookup,
            )?;

            let validation = validate_object_revision(
                &revision_statement,
                Some(&genesis_statement),
                manifest_value.as_ref(),
                commit_lookup.as_ref(),
            );

            let mut signature = verify_envelope_statement(&revision_statement, &store);

            // Resolve the truster for trust evaluation. Trust is
            // first-person so it is always parameterized by *who* is
            // asking. `--no-as` skips evaluation; `--as <id>` is
            // explicit; otherwise the keystore must have exactly one
            // entry to be unambiguous.
            let truster = if no_as {
                None
            } else {
                resolve_verify_truster(paths, r#as)?
            };
            if let Some(by_actor) = truster.as_ref() {
                signature.trust = match kairo_statement::verify::evaluate_trust(
                    by_actor,
                    &signature.signature_actor,
                    &store,
                ) {
                    Ok(eval) => eval,
                    Err(error) => return Err(CliError::ReadActorTrust(error)),
                };
            }

            let revision_checks = RevisionChecks {
                statement_id: revision_statement.statement_id(),
                revision: revision_body.revision().clone(),
                revision_object: revision_body.object().clone(),
                signature,
                validation,
                manifest_source: manifest_source.map(|p| p.display().to_string()),
                truster,
            };

            let overall = aggregate_overall_status(&genesis, &revision_checks, &object_id);

            let report = ObjectVerificationReport {
                object: object_id,
                genesis,
                frontier,
                revision: revision_checks,
                overall,
                repo_source,
            };

            if matches!(overall, OverallStatus::Invalid) {
                return Err(CliError::ObjectVerificationFailed(if json {
                    format_object_verification_json(&report)
                } else {
                    format_object_verification(&report)
                }));
            }

            if json {
                Ok(format_object_verification_json(&report))
            } else {
                Ok(format_object_verification(&report))
            }
        }
    }
}

/// Resolve the truster used for trust evaluation in `verify object`.
///
/// `--as <id>` is authoritative. Otherwise: if the keystore has
/// exactly one actor, auto-pick it; if zero, return `None` (trust
/// stays `Unevaluated`); if more than one, return an error so the
/// user explicitly chooses with `--as`.
fn resolve_verify_truster(
    paths: &StorePaths,
    explicit: Option<String>,
) -> Result<Option<ActorId>, CliError> {
    if let Some(actor) = explicit {
        let by_actor = ActorId::new(actor.clone())
            .map_err(|source| CliError::ParseActorId { actor, source })?;
        return Ok(Some(by_actor));
    }
    let keystore = open_keystore(paths)?;
    let mut actors = keystore.list_actors().map_err(CliError::ListKeystore)?;
    match actors.len() {
        0 => Ok(None),
        1 => Ok(Some(actors.remove(0))),
        _ => {
            actors.sort_by(|a, b| a.as_str().cmp(b.as_str()));
            Err(CliError::AmbiguousLocalActor { candidates: actors })
        }
    }
}

/// Resolve the Git source for `verify object` per the precedence
/// in `specs/DECISIONS.md` §9 / `specs/PHASE_2.md` §1:
///
/// 1. `--repo <path>` is authoritative — that path, no fallbacks.
/// 2. `--no-repo` skips everything; content layer goes Indeterminate.
/// 3. Default: try the managed cache first (per-object cache repo
///    exists *and* contains the commit OID), then fall back to
///    cwd-upward discovery.
///
/// Cache probe is cheap: `kairo_git::object_repo_path` is just
/// sharding (no I/O beyond an existence check), and the per-object
/// repo's gix open is local-only. We deliberately do not call
/// `GitCache::open` here — that would `git init --bare` the pool
/// for first-time users who only have a cwd repo, requiring git
/// on PATH for a code path that doesn't otherwise need it.
///
/// Cache miss + `--no-cwd-repo` returns `(None, Skipped)` instead
/// of erroring — the user explicitly opted out of cwd discovery,
/// so absence of a result is expected (content layer Indeterminate).
/// Cache miss + cwd-discovery failure preserves today's behavior:
/// errors with `GitRepoNotDiscovered`.
fn resolve_repo_for_verify(
    paths: &StorePaths,
    object_id: &ObjectId,
    revision: &RevisionId,
    explicit_repo: Option<&Path>,
    no_repo: bool,
    no_cache: bool,
    no_cwd_repo: bool,
) -> Result<(Option<kairo_git::Repository>, RepoSource), CliError> {
    if no_repo {
        return Ok((None, RepoSource::Skipped));
    }

    if let Some(path) = explicit_repo {
        let repo = kairo_git::discover(path).map_err(|source| CliError::OpenGitRepo {
            path: path.to_path_buf(),
            source,
        })?;
        return Ok((Some(repo), RepoSource::Filesystem { path: path.to_path_buf() }));
    }

    if !no_cache {
        if let Some(repo) = try_cache_repo_for_revision(paths, object_id.as_str(), revision)? {
            return Ok((Some(repo), RepoSource::Cache { object: object_id.clone() }));
        }
    }

    if no_cwd_repo {
        return Ok((None, RepoSource::Skipped));
    }

    let cwd = std::env::current_dir().map_err(|source| CliError::CwdUnavailable { source })?;
    match kairo_git::discover(&cwd) {
        Ok(repo) => {
            let path = repo.git_dir().to_path_buf();
            Ok((Some(repo), RepoSource::Filesystem { path }))
        }
        Err(_) => Err(CliError::GitRepoNotDiscovered { searched_from: cwd }),
    }
}

/// Probe the managed Git cache for `object_id`'s per-object bare
/// repo. Returns `Some(repo)` only if the per-object dir exists
/// *and* its object DB (transparently including the shared pool
/// via alternates) reaches the commit named in `revision`. A
/// non-git revision scheme returns `None` immediately — there's
/// nothing for the cache to resolve.
fn try_cache_repo_for_revision(
    paths: &StorePaths,
    object_id: &str,
    revision: &RevisionId,
) -> Result<Option<kairo_git::Repository>, CliError> {
    let Some(oid) = revision.as_str().strip_prefix("git:sha256:") else {
        return Ok(None);
    };
    let git_root = paths.git_root();
    let repo_path = match kairo_git::object_repo_path(&git_root, object_id) {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    if !repo_path.exists() {
        return Ok(None);
    }
    let repo = match kairo_git::open(&repo_path) {
        Ok(repo) => repo,
        Err(_) => return Ok(None),
    };
    match repo.find_commit(oid) {
        Ok(Some(_)) => Ok(Some(repo)),
        Ok(None) => Ok(None),
        Err(error) => Err(CliError::GitOperation { source: error }),
    }
}

/// Strip the `git:sha256:` prefix from a `RevisionId` and look up the
/// commit. Non-git revisions return `Ok(None)` — the caller treats
/// that as "content layer is Indeterminate."
fn lookup_commit_for_revision(
    repo: &kairo_git::Repository,
    revision: &RevisionId,
) -> Result<CommitLookup, CliError> {
    let oid = match revision.as_str().strip_prefix("git:sha256:") {
        Some(oid) => oid,
        None => return Ok(CommitLookup::NotFound),
    };
    match repo.find_commit(oid) {
        Ok(Some(info)) => Ok(CommitLookup::Found {
            parent_oids: info.parent_ids,
        }),
        Ok(None) => Ok(CommitLookup::NotFound),
        Err(error) => Err(CliError::GitOperation { source: error }),
    }
}

/// Resolve the manifest used for the binding check. Returns the
/// parsed manifest plus a "source path" string for the report.
fn resolve_manifest(
    explicit_manifest: Option<&Path>,
    git_repo: Option<&kairo_git::Repository>,
    revision: &RevisionId,
    commit_lookup: &Option<CommitLookup>,
) -> Result<(Option<ObjectManifest>, Option<PathBuf>), CliError> {
    if let Some(path) = explicit_manifest {
        let manifest = commands::manifest::read_manifest(path.to_path_buf())?;
        return Ok((Some(manifest), Some(path.to_path_buf())));
    }
    let (Some(repo), Some(CommitLookup::Found { .. })) = (git_repo, commit_lookup.as_ref()) else {
        return Ok((None, None));
    };
    let oid = match revision.as_str().strip_prefix("git:sha256:") {
        Some(oid) => oid,
        None => return Ok((None, None)),
    };
    let bytes = repo
        .read_blob_at_path(oid, "kairo.toml")
        .map_err(|source| CliError::GitOperation { source })?;
    let Some(bytes) = bytes else {
        return Ok((None, None));
    };
    let text = String::from_utf8(bytes).map_err(|_| CliError::ManifestNotUtf8)?;
    let manifest = ObjectManifest::parse_toml(&text).map_err(CliError::ParseManifest)?;
    Ok((
        Some(manifest),
        Some(PathBuf::from(format!("git:sha256:{oid}/kairo.toml"))),
    ))
}

fn aggregate_overall_status(
    genesis: &GenesisCheck,
    revision: &RevisionChecks,
    requested_object: &ObjectId,
) -> OverallStatus {
    // Genesis: a successful store read already proved the derived
    // ObjectId matches; a mismatch here would only arise if the CLI
    // and store somehow disagreed on the requested id.
    let genesis_status = if &genesis.derived_object == requested_object {
        OverallStatus::Valid
    } else {
        OverallStatus::Invalid
    };

    let signature_status = match revision.signature.signature {
        SignatureStatus::Valid => OverallStatus::Valid,
        SignatureStatus::NotEvaluated => OverallStatus::Indeterminate,
        _ => OverallStatus::Invalid,
    };
    let actor_status = match revision.signature.actor {
        ActorResolution::Resolved => OverallStatus::Valid,
        ActorResolution::NotFound | ActorResolution::SignatureActorMismatch => {
            OverallStatus::Invalid
        }
        ActorResolution::ResolverUnavailable(_) => OverallStatus::Indeterminate,
    };
    let object_consistency_status = match revision.validation.object_consistency {
        ObjectConsistencyCheck::Consistent => OverallStatus::Valid,
        ObjectConsistencyCheck::Mismatch { .. } => OverallStatus::Invalid,
        ObjectConsistencyCheck::GenesisNotProvided => OverallStatus::Indeterminate,
    };
    let manifest_binding_status = match revision.validation.manifest_binding {
        ManifestBindingCheck::Bound => OverallStatus::Valid,
        ManifestBindingCheck::HashMismatch { .. }
        | ManifestBindingCheck::DeclaredObjectMismatch { .. } => OverallStatus::Invalid,
        ManifestBindingCheck::ManifestNotProvided => OverallStatus::Indeterminate,
    };
    let content_status = match revision.validation.content {
        ContentLayerCheck::Verified => OverallStatus::Valid,
        ContentLayerCheck::ParentMismatch { .. } | ContentLayerCheck::CommitNotFound => {
            OverallStatus::Invalid
        }
        ContentLayerCheck::Indeterminate => OverallStatus::Indeterminate,
    };

    fold_status(&[
        genesis_status,
        signature_status,
        actor_status,
        object_consistency_status,
        manifest_binding_status,
        content_status,
    ])
}

/// Worst-of fold: any Invalid wins; otherwise any Indeterminate wins;
/// otherwise Valid.
fn fold_status(items: &[OverallStatus]) -> OverallStatus {
    if items.iter().any(|s| matches!(s, OverallStatus::Invalid)) {
        OverallStatus::Invalid
    } else if items.iter().any(|s| matches!(s, OverallStatus::Indeterminate)) {
        OverallStatus::Indeterminate
    } else {
        OverallStatus::Valid
    }
}

fn format_object_verification(report: &ObjectVerificationReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("verify object: {}\n", report.overall.label()));
    out.push_str(&format!("object = {}\n", report.object));
    out.push_str(&format!("commit lookup: {}\n", format_repo_source(&report.repo_source)));
    out.push_str(&format!(
        "genesis: derived_object = {}\n",
        report.genesis.derived_object
    ));
    match &report.frontier {
        FrontierResolution::BranchTip {
            actor,
            name,
            statement,
        } => {
            out.push_str(&format!(
                "frontier: branch actor={actor} name={name} statement={statement}\n"
            ));
        }
        FrontierResolution::PinnedStatement { statement } => {
            out.push_str(&format!("frontier: pinned statement={statement}\n"));
        }
    }
    out.push_str(&format!(
        "revision: statement = {}\n",
        report.revision.statement_id
    ));
    out.push_str(&format!(
        "  revision = {}\n",
        report.revision.revision.as_str()
    ));
    out.push_str(&format!(
        "  object = {}\n",
        report.revision.revision_object
    ));
    out.push_str(&format!(
        "  signature = {}\n",
        format_signature_status(&report.revision.signature.signature)
    ));
    out.push_str(&format!(
        "  actor = {}\n",
        format_actor_resolution(&report.revision.signature.actor)
    ));
    let trust_truster = match &report.revision.truster {
        Some(actor) => format!(" (as {actor})"),
        None => String::new(),
    };
    out.push_str(&format!(
        "  trust = {}{trust_truster}\n",
        format_trust(&report.revision.signature.trust)
    ));
    out.push_str(&format!(
        "  object_consistency = {}\n",
        format_object_consistency(&report.revision.validation.object_consistency)
    ));
    out.push_str(&format!(
        "  manifest_binding = {}\n",
        format_manifest_binding(&report.revision.validation.manifest_binding)
    ));
    if let Some(source) = &report.revision.manifest_source {
        out.push_str(&format!("  manifest_source = {source}\n"));
    }
    out.push_str(&format!(
        "  parents = {}\n",
        format_parents(&report.revision.validation.parents)
    ));
    out.push_str(&format!(
        "  content = {}\n",
        format_content_layer(&report.revision.validation.content)
    ));
    out
}

fn format_repo_source(source: &RepoSource) -> String {
    match source {
        RepoSource::Cache { object } => format!("cache (object {object})"),
        RepoSource::Filesystem { path } => format!("repo at {}", path.display()),
        RepoSource::Skipped => "skipped".to_owned(),
    }
}

fn format_content_layer(check: &ContentLayerCheck) -> String {
    match check {
        ContentLayerCheck::Verified => "VALID (commit found, parents agree)".to_owned(),
        ContentLayerCheck::ParentMismatch { expected, actual } => format!(
            "INVALID (parent mismatch; expected {expected:?}, actual {actual:?})"
        ),
        ContentLayerCheck::CommitNotFound => "INVALID (commit not in repo)".to_owned(),
        ContentLayerCheck::Indeterminate => {
            "INDETERMINATE (no Git lookup performed)".to_owned()
        }
    }
}

fn format_object_verification_json(report: &ObjectVerificationReport) -> String {
    let frontier = match &report.frontier {
        FrontierResolution::BranchTip {
            actor,
            name,
            statement,
        } => serde_json::json!({
            "kind": "branch",
            "actor": actor.to_string(),
            "name": name,
            "statement": statement.to_string(),
        }),
        FrontierResolution::PinnedStatement { statement } => serde_json::json!({
            "kind": "pinned",
            "statement": statement.to_string(),
        }),
    };

    let manifest_binding_value = match &report.revision.validation.manifest_binding {
        ManifestBindingCheck::Bound => serde_json::json!({ "status": "bound" }),
        ManifestBindingCheck::HashMismatch { expected, actual } => serde_json::json!({
            "status": "hash-mismatch",
            "expected": expected.to_string(),
            "actual": actual.to_string(),
        }),
        ManifestBindingCheck::DeclaredObjectMismatch { expected, actual } => serde_json::json!({
            "status": "declared-object-mismatch",
            "expected": expected.to_string(),
            "actual": actual.to_string(),
        }),
        ManifestBindingCheck::ManifestNotProvided => {
            serde_json::json!({ "status": "manifest-not-provided" })
        }
    };

    let object_consistency_value = match &report.revision.validation.object_consistency {
        ObjectConsistencyCheck::Consistent => serde_json::json!({ "status": "consistent" }),
        ObjectConsistencyCheck::Mismatch { expected, actual } => serde_json::json!({
            "status": "mismatch",
            "expected": expected.to_string(),
            "actual": actual.to_string(),
        }),
        ObjectConsistencyCheck::GenesisNotProvided => {
            serde_json::json!({ "status": "genesis-not-provided" })
        }
    };

    let parents_value = match &report.revision.validation.parents {
        ParentReferenceCheck::NoParents => serde_json::json!({ "status": "none" }),
        ParentReferenceCheck::Declared { count } => serde_json::json!({
            "status": "declared",
            "count": count,
        }),
    };

    let value = serde_json::json!({
        "overall": report.overall.label(),
        "object": report.object.to_string(),
        "genesis": {
            "derived_object": report.genesis.derived_object.to_string(),
        },
        "frontier": frontier,
        "revision": {
            "statement_id": report.revision.statement_id.to_string(),
            "revision": report.revision.revision.as_str(),
            "object": report.revision.revision_object.to_string(),
            "signature": format_signature_status(&report.revision.signature.signature),
            "actor": format_actor_resolution(&report.revision.signature.actor),
            "trust": {
                "status": format_trust(&report.revision.signature.trust),
                "by_actor": report.revision.truster.as_ref().map(|a| a.to_string()),
            },
            "object_consistency": object_consistency_value,
            "manifest_binding": manifest_binding_value,
            "manifest_source": report.revision.manifest_source.clone(),
            "parents": parents_value,
            "content": content_layer_json(&report.revision.validation.content),
        },
    });
    let mut output = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    output.push('\n');
    output
}

fn format_object_consistency(check: &ObjectConsistencyCheck) -> &'static str {
    match check {
        ObjectConsistencyCheck::Consistent => "VALID",
        ObjectConsistencyCheck::Mismatch { .. } => "INVALID (mismatch)",
        ObjectConsistencyCheck::GenesisNotProvided => "INDETERMINATE (genesis not provided)",
    }
}

fn format_manifest_binding(check: &ManifestBindingCheck) -> &'static str {
    match check {
        ManifestBindingCheck::Bound => "VALID (bound)",
        ManifestBindingCheck::HashMismatch { .. } => "INVALID (hash mismatch)",
        ManifestBindingCheck::DeclaredObjectMismatch { .. } => {
            "INVALID (declared object mismatch)"
        }
        ManifestBindingCheck::ManifestNotProvided => "INDETERMINATE (no manifest provided)",
    }
}

fn format_parents(check: &ParentReferenceCheck) -> String {
    match check {
        ParentReferenceCheck::NoParents => "0 (initial revision)".to_owned(),
        ParentReferenceCheck::Declared { count } => format!("{count} declared"),
    }
}

fn content_layer_json(check: &ContentLayerCheck) -> serde_json::Value {
    match check {
        ContentLayerCheck::Verified => serde_json::json!({ "status": "verified" }),
        ContentLayerCheck::ParentMismatch { expected, actual } => serde_json::json!({
            "status": "parent-mismatch",
            "expected": expected,
            "actual": actual,
        }),
        ContentLayerCheck::CommitNotFound => serde_json::json!({ "status": "commit-not-found" }),
        ContentLayerCheck::Indeterminate => serde_json::json!({ "status": "indeterminate" }),
    }
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



fn help_text() -> String {
    "kairo\n\nUsage:\n  kairo [--store <path>] [--keys <path>] <command>\n\nCommands:\n  kairo actor id --genesis <path>\n  kairo actor create --kind <kind> (--attestation-key <hex> | --generate-attestation-key)...\n  kairo actor import --genesis <path>\n  kairo actor rotate-key --actor <id>\n  kairo actor revoke-key --actor <id> --key <key-id> [--retroactive] [--reason <text>] [--brick-actor]\n  kairo actor key-history --actor <id> [--json]\n  kairo actor recover-key sign --actor <id> --attestation-key-seed <path>\n  kairo actor recover-key prepare --actor <id> --new-key <hex> --output <path>\n  kairo actor recover-key submit --prepared <path> [--signature <path>]\n  kairo actor add-attestation-key sign --actor <id> --signing-attestation-key-seed <path> (--key <hex> | --generate)\n  kairo actor add-attestation-key prepare --actor <id> --new-key <hex> --output <path>\n  kairo actor add-attestation-key submit --prepared <path> [--signature <path>]\n  kairo actor revoke-attestation-key sign --actor <id> --signing-attestation-key-seed <path> --revoke-key <key-id> [--reason <text>]\n  kairo actor revoke-attestation-key prepare --actor <id> --revoke-key <key-id> [--reason <text>] --output <path>\n  kairo actor revoke-attestation-key submit --prepared <path> [--signature <path>]\n  kairo actor change-attestation-threshold sign --actor <id> --attestation-key-seed <path> --to <N>\n  kairo actor change-attestation-threshold prepare --actor <id> --to <N> --output <path>\n  kairo actor change-attestation-threshold submit --prepared <path> [--signature <path>]\n  kairo actor co-sign --prepared <path> --actor <id> --attestation-key-seed <path>\n  kairo manifest hash [path]\n  kairo manifest inspect [path]\n  kairo object create --actor <id> --kind <kind> [--initial-revision <ref>]\n  kairo object import --statement <path>\n  kairo revision create --actor <id> --object <id> --revision <ref> [--manifest <path>] [--parent <ref>]... [--no-attests-reachable-history]\n  kairo revision import --statement <path>\n  kairo revision inspect --statement <id> [--json]\n  kairo revision list --object <id>\n  kairo revision validate-manifest --statement <path> [--manifest <path>]\n  kairo revision verify-signature --statement <path> (--public-key <base64>|--public-key-file <path>)\n  kairo revision verify-actor-genesis --statement <path> --actor-genesis <path> [--json]\n  kairo branch set --actor <id> --object <id> --revision <statement-id> [--name <name>]\n  kairo branch show --object <id> [--actor <id>] [--name <name>] [--json]\n  kairo branch list --object <id>\n  kairo tag bind --actor <id> --object <id> --version <semver> --revision <statement-id>\n  kairo tag revoke --actor <id> --object <id> --version <semver>\n  kairo tag show --object <id> [--actor <id>] --version <semver> [--json]\n  kairo tag list --object <id>\n  kairo tag history --object <id> [--actor <id>] --version <semver> [--json]\n  kairo trust grant --by <id> --of <id> [--reason <text>]\n  kairo trust block --by <id> --of <id> [--reason <text>]\n  kairo trust withdraw --by <id> --of <id> [--reason <text>]\n  kairo trust show --by <id> --of <id> [--json]\n  kairo trust list --by <id>\n  kairo trust history --by <id> --of <id> [--json]\n  kairo capability grant --grantor <id> --grantee <id> --object <id> --kind <kind>... [--delegable] [--expires-at <RFC3339>] [--max-delegation-depth <N>] [--key-pinned <keyid>]\n  kairo capability revoke --grantor <id> --grant <statement-id> [--retroactive] [--reason <text>]\n  kairo capability list (--grantor <id> | --object <id>)\n  kairo bundle export --object <id> --output <dir> [--include-git]\n  kairo bundle import --input <dir>\n  kairo snapshot compute --object <id> [--actor <id>] [--name <name>] [--statement <id>] [--json]\n  kairo verify object --object <id> [--actor <id>] [--name <name>] [--statement <id>] [--as <id>|--no-as] [--repo <path>|--no-repo] [--no-cache] [--no-cwd-repo] [--manifest <path>] [--json]\n  kairo git fetch --object <id> --remote <url> [--branch <name>]\n  kairo git cache status\n".to_owned()
}



#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests;
