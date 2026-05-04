//! Integration tests for the bundle round-trip.
//!
//! These tests build a store with an actor, an object, a revision,
//! a branch tip, and a version tag — then export to a tempdir bundle
//! and import into a fresh store, asserting every id round-trips
//! byte-for-byte.

#![cfg(test)]
#![allow(clippy::expect_used, clippy::panic)]

use std::fs;

use ed25519_dalek::{Signer, SigningKey};
use kairo_core::canonical::CanonicalEncode;
use kairo_core::{ActorId, BlobId, KairoRef, ObjectId, StatementId, Timestamp};
use kairo_identity::{ActorGenesisBody, ActorKind, PublicKey};
use kairo_object::{ObjectManifest, OBJECT_MANIFEST_DOMAIN};
use kairo_statement::{
    ObjectBranchBody, ObjectGenesisBody, ObjectGenesisStatement, ObjectKind, ObjectRevisionBody,
    ObjectVersionTagBody, RevisionId, SemverVersion, Signature, SignedStatement, UnsignedStatement,
};
use kairo_store::{
    ActorStore, BlobStore, BranchResolver, FilesystemStore, ObjectStore, StatementStore,
    VersionTagResolver,
};
use tempfile::TempDir;

use crate::error::BundleError;
use crate::{import_bundle, write_bundle, BUNDLE_SCHEMA, MANIFEST_FILENAME};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const COMMIT_OID: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}

fn public_key() -> PublicKey {
    PublicKey::ed25519(signing_key().verifying_key().to_bytes())
}

fn timestamp() -> Timestamp {
    Timestamp::from_seconds(1_700_000_000)
}

fn actor_genesis() -> ActorGenesisBody {
    ActorGenesisBody::new(ActorKind::person(), public_key(), timestamp(), [9; 32])
}

fn manifest_blob() -> (BlobId, Vec<u8>, ObjectManifest) {
    let toml = r#"[kairo]
schema = 1
kind = "software"
name = "bundle-fixture"

[content]
kind = "tree"
"#;
    let manifest = ObjectManifest::parse_toml(toml).expect("parse manifest");
    let bytes = manifest.canonical_bytes();
    let id = BlobId::from_bytes(OBJECT_MANIFEST_DOMAIN, &bytes);
    (id, bytes, manifest)
}

fn signed_object_genesis(actor: &ActorId) -> ObjectGenesisStatement {
    let body = ObjectGenesisBody::new(
        ObjectKind::software(),
        actor.clone(),
        timestamp(),
        [42; 32],
        Some(RevisionId::new(format!("git:sha256:{COMMIT_OID}"))),
    );
    let signature_bytes = signing_key().sign(&body.canonical_bytes()).to_bytes();
    let signature = Signature::new(
        actor.clone(),
        public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.to_vec(),
    );
    ObjectGenesisStatement::new(body, signature)
}

fn signed_revision(
    actor: &ActorId,
    object: &ObjectId,
    manifest_blob_id: BlobId,
) -> Result<SignedStatement<ObjectRevisionBody>, Box<dyn std::error::Error>> {
    let body = ObjectRevisionBody::new(
        object.clone(),
        RevisionId::new(format!("git:sha256:{COMMIT_OID}")),
        vec![],
        manifest_blob_id,
        true,
    );
    let subject: KairoRef = format!("object:{object}").parse()?;
    let unsigned = UnsignedStatement::new(actor.clone(), subject, timestamp(), body);
    let bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
    let signature = Signature::new(
        actor.clone(),
        public_key().key_id().to_string(),
        "ed25519",
        bytes.to_vec(),
    );
    Ok(SignedStatement::new(unsigned, signature))
}

fn signed_branch(
    actor: &ActorId,
    object: &ObjectId,
    revision: StatementId,
) -> Result<SignedStatement<ObjectBranchBody>, Box<dyn std::error::Error>> {
    let body = ObjectBranchBody::new(object.clone(), "head", revision, None);
    let subject: KairoRef = format!("object:{object}").parse()?;
    let unsigned = UnsignedStatement::new(actor.clone(), subject, timestamp(), body);
    let bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
    let signature = Signature::new(
        actor.clone(),
        public_key().key_id().to_string(),
        "ed25519",
        bytes.to_vec(),
    );
    Ok(SignedStatement::new(unsigned, signature))
}

fn signed_version_tag(
    actor: &ActorId,
    object: &ObjectId,
    revision: StatementId,
) -> Result<SignedStatement<ObjectVersionTagBody>, Box<dyn std::error::Error>> {
    let body = ObjectVersionTagBody::new(
        object.clone(),
        SemverVersion::parse("1.2.3")?,
        Some(revision),
        None,
    )?;
    let subject: KairoRef = format!("object:{object}").parse()?;
    let unsigned = UnsignedStatement::new(actor.clone(), subject, timestamp(), body);
    let bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
    let signature = Signature::new(
        actor.clone(),
        public_key().key_id().to_string(),
        "ed25519",
        bytes.to_vec(),
    );
    Ok(SignedStatement::new(unsigned, signature))
}

/// Build a fully-populated store with one actor, one object, one
/// revision, one branch tip, one version tag, and the manifest blob.
/// Returns the open store dir, a handle, and every identity needed
/// for assertions.
struct Fixture {
    _dir: TempDir,
    store: FilesystemStore,
    actor_id: ActorId,
    object_id: ObjectId,
    revision_statement_id: StatementId,
    branch_statement_id: StatementId,
    version_tag_statement_id: StatementId,
    blob_id: BlobId,
}

fn build_fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let store = FilesystemStore::open(dir.path())?;
    let genesis = actor_genesis();
    let actor_id = store.put_actor(&genesis)?;

    let (blob_id, blob_bytes, _) = manifest_blob();
    store.put_blob(&blob_id, &blob_bytes)?;

    let signed_object = signed_object_genesis(&actor_id);
    let object_id = store.put_object_genesis(&signed_object)?;

    let signed_rev = signed_revision(&actor_id, &object_id, blob_id.clone())?;
    let revision_statement_id = store.put_object_revision(&signed_rev)?;

    let signed_br = signed_branch(&actor_id, &object_id, revision_statement_id.clone())?;
    let branch_statement_id = store.put_object_branch(&signed_br)?;

    let signed_tag = signed_version_tag(&actor_id, &object_id, revision_statement_id.clone())?;
    let version_tag_statement_id = store.put_object_version_tag(&signed_tag)?;

    Ok(Fixture {
        _dir: dir,
        store,
        actor_id,
        object_id,
        revision_statement_id,
        branch_statement_id,
        version_tag_statement_id,
        blob_id,
    })
}

#[test]
fn round_trips_through_export_and_import() -> TestResult {
    let src = build_fixture()?;
    let bundle_dir = TempDir::new()?;

    let manifest = write_bundle(
        &src.store,
        &src.object_id,
        bundle_dir.path(),
        "2026-05-03T12:00:00Z",
        "0.1.0",
    )?;

    assert_eq!(manifest.schema, BUNDLE_SCHEMA);
    assert_eq!(manifest.roots.objects, vec![src.object_id.to_string()]);
    assert_eq!(manifest.contents.actors, vec![src.actor_id.to_string()]);
    assert_eq!(manifest.contents.objects, vec![src.object_id.to_string()]);
    assert!(manifest
        .contents
        .statements
        .contains(&src.revision_statement_id.to_string()));
    assert!(manifest
        .contents
        .statements
        .contains(&src.branch_statement_id.to_string()));
    assert!(manifest
        .contents
        .statements
        .contains(&src.version_tag_statement_id.to_string()));
    assert_eq!(manifest.contents.blobs, vec![src.blob_id.to_string()]);
    assert!(!manifest.git_history.included);
    assert_eq!(manifest.git_history.expected_commits, vec![COMMIT_OID]);

    // Sanity: every advertised file is on disk.
    for actor in &manifest.contents.actors {
        assert!(bundle_dir.path().join("actors").join(format!("{actor}.json")).exists());
    }
    for object in &manifest.contents.objects {
        assert!(bundle_dir.path().join("objects").join(format!("{object}.json")).exists());
    }
    for stmt in &manifest.contents.statements {
        assert!(bundle_dir.path().join("statements").join(format!("{stmt}.json")).exists());
    }
    for blob in &manifest.contents.blobs {
        assert!(bundle_dir.path().join("blobs").join(blob).exists());
    }
    assert!(bundle_dir.path().join(MANIFEST_FILENAME).exists());

    // Import into a fresh store.
    let dest_dir = TempDir::new()?;
    let dest_store = FilesystemStore::open(dest_dir.path())?;
    let summary = import_bundle(bundle_dir.path(), &dest_store)?;
    assert_eq!(summary.actors, 1);
    assert_eq!(summary.objects, 1);
    assert_eq!(summary.statements, 3);
    assert_eq!(summary.blobs, 1);

    // Each round-tripped record exists with matching id at the
    // destination.
    let dest_actor = dest_store.get_actor(&src.actor_id)?;
    assert_eq!(dest_actor.actor_id(), src.actor_id);

    let dest_object = dest_store.get_object_genesis(&src.object_id)?;
    assert_eq!(dest_object.object_id(), src.object_id);

    let dest_rev = dest_store.get_object_revision(&src.revision_statement_id)?;
    assert_eq!(dest_rev.statement_id(), src.revision_statement_id);

    let dest_branch = dest_store.latest_branch(&src.actor_id, &src.object_id, "head")?;
    assert!(matches!(dest_branch, Some(s) if s.statement_id() == src.branch_statement_id));

    let dest_tag = dest_store.latest_version_tag(&src.actor_id, &src.object_id, "1.2.3")?;
    assert!(matches!(dest_tag, Some(s) if s.statement_id() == src.version_tag_statement_id));

    let dest_blob = dest_store.get_blob(&src.blob_id)?;
    let (_, expected_blob_bytes, _) = manifest_blob();
    assert_eq!(dest_blob, expected_blob_bytes);

    Ok(())
}

#[test]
fn import_is_idempotent() -> TestResult {
    let src = build_fixture()?;
    let bundle_dir = TempDir::new()?;
    write_bundle(&src.store, &src.object_id, bundle_dir.path(), "ts", "0.1.0")?;

    let dest_dir = TempDir::new()?;
    let dest_store = FilesystemStore::open(dest_dir.path())?;
    let first = import_bundle(bundle_dir.path(), &dest_store)?;
    let second = import_bundle(bundle_dir.path(), &dest_store)?;
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn write_refuses_non_empty_destination() -> TestResult {
    let src = build_fixture()?;
    let bundle_dir = TempDir::new()?;
    fs::write(bundle_dir.path().join("squatter"), b"hi")?;
    let result = write_bundle(&src.store, &src.object_id, bundle_dir.path(), "ts", "0.1.0");
    assert!(matches!(result, Err(BundleError::DestinationNotEmpty { .. })));
    Ok(())
}

#[test]
fn write_errors_when_root_object_missing() -> TestResult {
    let dir = TempDir::new()?;
    let store = FilesystemStore::open(dir.path())?;
    let bundle_dir = TempDir::new()?;
    let unknown = signed_object_genesis(&actor_genesis().actor_id()).object_id();
    let result = write_bundle(&store, &unknown, bundle_dir.path(), "ts", "0.1.0");
    assert!(matches!(result, Err(BundleError::RootObjectNotFound { .. })));
    Ok(())
}

#[test]
fn import_rejects_tampered_blob() -> TestResult {
    let src = build_fixture()?;
    let bundle_dir = TempDir::new()?;
    let manifest = write_bundle(&src.store, &src.object_id, bundle_dir.path(), "ts", "0.1.0")?;

    // Overwrite the blob's bytes — its derived BlobId no longer
    // matches the filename.
    let blob_id = manifest.contents.blobs.first().expect("one blob");
    let blob_path = bundle_dir.path().join("blobs").join(blob_id);
    fs::write(&blob_path, b"tampered")?;

    let dest_dir = TempDir::new()?;
    let dest_store = FilesystemStore::open(dest_dir.path())?;
    let result = import_bundle(bundle_dir.path(), &dest_store);
    assert!(matches!(result, Err(BundleError::BlobHashMismatch { .. })));
    Ok(())
}

#[test]
fn import_rejects_missing_blob_file() -> TestResult {
    let src = build_fixture()?;
    let bundle_dir = TempDir::new()?;
    let manifest = write_bundle(&src.store, &src.object_id, bundle_dir.path(), "ts", "0.1.0")?;

    let blob_id = manifest.contents.blobs.first().expect("one blob");
    fs::remove_file(bundle_dir.path().join("blobs").join(blob_id))?;

    let dest_dir = TempDir::new()?;
    let dest_store = FilesystemStore::open(dest_dir.path())?;
    let result = import_bundle(bundle_dir.path(), &dest_store);
    assert!(matches!(result, Err(BundleError::MissingRecord { kind: "blob", .. })));
    Ok(())
}

#[test]
fn import_rejects_unsupported_schema() -> TestResult {
    let src = build_fixture()?;
    let bundle_dir = TempDir::new()?;
    write_bundle(&src.store, &src.object_id, bundle_dir.path(), "ts", "0.1.0")?;

    let manifest_path = bundle_dir.path().join(MANIFEST_FILENAME);
    let bytes = fs::read(&manifest_path)?;
    let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
    value["schema"] = serde_json::Value::String("kairo.bundle.v999".to_owned());
    fs::write(&manifest_path, serde_json::to_vec_pretty(&value)?)?;

    let dest_dir = TempDir::new()?;
    let dest_store = FilesystemStore::open(dest_dir.path())?;
    let result = import_bundle(bundle_dir.path(), &dest_store);
    assert!(matches!(result, Err(BundleError::UnsupportedSchema { .. })));
    Ok(())
}

#[test]
fn import_rejects_dangling_actor_reference() -> TestResult {
    let src = build_fixture()?;
    let bundle_dir = TempDir::new()?;
    let manifest = write_bundle(&src.store, &src.object_id, bundle_dir.path(), "ts", "0.1.0")?;

    // Strip the actor list; the statements still reference an actor
    // the manifest no longer advertises.
    let manifest_path = bundle_dir.path().join(MANIFEST_FILENAME);
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    value["contents"]["actors"] = serde_json::json!([]);
    fs::write(&manifest_path, serde_json::to_vec_pretty(&value)?)?;
    // Also drop the actor file so it can't accidentally be picked up
    // by a future codepath.
    let actor_id = manifest
        .contents
        .actors
        .first()
        .expect("one actor")
        .clone();
    fs::remove_file(bundle_dir.path().join("actors").join(format!("{actor_id}.json")))?;

    let dest_dir = TempDir::new()?;
    let dest_store = FilesystemStore::open(dest_dir.path())?;
    let result = import_bundle(bundle_dir.path(), &dest_store);
    assert!(matches!(result, Err(BundleError::DanglingActor { .. })));
    Ok(())
}

#[test]
fn export_excludes_actor_trust_statements() -> TestResult {
    use kairo_statement::{ActorTrustBody, TrustDecision};

    let src = build_fixture()?;
    // Add an ActorTrust statement to the source store. It must NOT
    // make it into an object bundle.
    let trusted_actor_genesis = ActorGenesisBody::new(
        ActorKind::person(),
        PublicKey::ed25519(SigningKey::from_bytes(&[12; 32]).verifying_key().to_bytes()),
        timestamp(),
        [99; 32],
    );
    let trusted_actor_id = src.store.put_actor(&trusted_actor_genesis)?;
    let body = ActorTrustBody::new(
        trusted_actor_id.clone(),
        Some(TrustDecision::Trusted),
        None,
        None,
    )?;
    let subject: KairoRef = format!("actor:{trusted_actor_id}").parse()?;
    let unsigned = UnsignedStatement::new(src.actor_id.clone(), subject, timestamp(), body);
    let signature_bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
    let signature = Signature::new(
        src.actor_id.clone(),
        public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.to_vec(),
    );
    let trust_statement = SignedStatement::new(unsigned, signature);
    let trust_statement_id = src.store.put_actor_trust(&trust_statement)?;

    let bundle_dir = TempDir::new()?;
    let manifest = write_bundle(&src.store, &src.object_id, bundle_dir.path(), "ts", "0.1.0")?;
    assert!(
        !manifest
            .contents
            .statements
            .contains(&trust_statement_id.to_string()),
        "ActorTrust must not be carried in an object bundle"
    );
    Ok(())
}

#[test]
fn export_excludes_actor_capability_statements() -> TestResult {
    use kairo_statement::{
        ActorCapabilityGrantBody, ActorCapabilityRevocationBody, Capability, CapabilityScope,
        StatementKind,
    };

    let src = build_fixture()?;
    // Add an ActorCapabilityGrant from src.actor to a new grantee on
    // src.object_id. Per CAPABILITIES.md §8 deferred work, capability
    // statements must NOT travel inside an object bundle — they're
    // first-person speech acts and need their own bundle type.
    let grantee_genesis = ActorGenesisBody::new(
        ActorKind::person(),
        PublicKey::ed25519(SigningKey::from_bytes(&[13; 32]).verifying_key().to_bytes()),
        timestamp(),
        [88; 32],
    );
    let grantee_id = src.store.put_actor(&grantee_genesis)?;

    let capability = Capability::new(
        CapabilityScope::Object(src.object_id.clone()),
        vec![StatementKind::ObjectVersionTag],
        false,
        vec![],
    )?;
    let grant_body = ActorCapabilityGrantBody::new(grantee_id.clone(), capability, None);
    let grant_subject: KairoRef = format!("actor:{grantee_id}").parse()?;
    let grant_unsigned = UnsignedStatement::new(
        src.actor_id.clone(),
        grant_subject,
        timestamp(),
        grant_body,
    );
    let grant_sig_bytes = signing_key()
        .sign(&grant_unsigned.canonical_bytes())
        .to_bytes();
    let grant_signature = Signature::new(
        src.actor_id.clone(),
        public_key().key_id().to_string(),
        "ed25519",
        grant_sig_bytes.to_vec(),
    );
    let grant_statement = SignedStatement::new(grant_unsigned, grant_signature);
    let grant_id = src.store.put_actor_capability_grant(&grant_statement)?;

    // And a revocation against that grant — same exclusion rule.
    let revoke_body = ActorCapabilityRevocationBody::new(grant_id.clone(), false, None);
    let revoke_subject: KairoRef = format!("statement:{grant_id}").parse()?;
    let revoke_unsigned = UnsignedStatement::new(
        src.actor_id.clone(),
        revoke_subject,
        timestamp(),
        revoke_body,
    );
    let revoke_sig_bytes = signing_key()
        .sign(&revoke_unsigned.canonical_bytes())
        .to_bytes();
    let revoke_signature = Signature::new(
        src.actor_id.clone(),
        public_key().key_id().to_string(),
        "ed25519",
        revoke_sig_bytes.to_vec(),
    );
    let revoke_statement = SignedStatement::new(revoke_unsigned, revoke_signature);
    let revoke_id = src
        .store
        .put_actor_capability_revocation(&revoke_statement)?;

    let bundle_dir = TempDir::new()?;
    let manifest = write_bundle(&src.store, &src.object_id, bundle_dir.path(), "ts", "0.1.0")?;
    assert!(
        !manifest
            .contents
            .statements
            .contains(&grant_id.to_string()),
        "ActorCapabilityGrant must not be carried in an object bundle"
    );
    assert!(
        !manifest
            .contents
            .statements
            .contains(&revoke_id.to_string()),
        "ActorCapabilityRevocation must not be carried in an object bundle"
    );
    Ok(())
}
