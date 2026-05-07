//! Library-API store fixtures.
//!
//! [`StoreFixture`] opens a temp `<root>/{store, keys}` and walks
//! actor → object → revision → branch chains through the kairo-*
//! library APIs. Each `make_*` call signs the relevant body with the
//! actor's secret key, persists the statement, and returns the
//! created entity (id + signed statement + supporting material).
//!
//! These fixtures intentionally do not go through the CLI surface;
//! they're fast, deterministic, and useful from any crate's test
//! suite. CLI-coupled fixtures (those that exercise the CLI's
//! parsing and output formatting) stay in their respective crates.

use std::cell::Cell;
use std::path::PathBuf;

use kairo_core::canonical::CanonicalEncode;
use kairo_core::{ActorId, BlobId, KairoRef, ObjectId, StatementId, Timestamp};
use kairo_identity::{ActorGenesisBody, ActorKind, PublicKey, SecretSigningKey};
use kairo_keystore::{FilesystemKeystore, Keystore};
use kairo_object::ObjectManifest;
use kairo_statement::{
    ObjectBranchBody, ObjectGenesisBody, ObjectGenesisStatement, ObjectKind,
    ObjectRevisionBody, RevisionId, Signature, SignedStatement, UnsignedStatement,
};
use kairo_store::{ActorStore, BlobStore, FilesystemStore, ObjectStore, StatementStore};
use tempfile::TempDir;

/// Fixed timestamp used throughout the fixtures so tests with
/// content-addressed ids (which include `created_at`) produce
/// deterministic results across runs.
const FIXTURE_TIMESTAMP: i64 = 1_700_000_000;

/// Open a fresh temp store + keystore, and offer chained
/// actor/object/revision/branch builders against them. Hold the
/// returned `TempDir` for the fixture's lifetime; dropping it
/// erases the on-disk state.
#[derive(Debug)]
pub struct StoreFixture {
    pub root: PathBuf,
    pub store: FilesystemStore,
    pub keystore: FilesystemKeystore,
    /// Counter used to generate unique seeds for each new actor's
    /// signing/attestation keys. Wrapped in `Cell` so make_actor
    /// can take `&self`.
    next_seed: Cell<u8>,
}

impl StoreFixture {
    /// Construct a fixture rooted at a fresh temp directory.
    /// Returns `(TempDir, StoreFixture)` — the `TempDir` keeps the
    /// on-disk state alive; drop it to clean up.
    pub fn temp() -> (TempDir, Self) {
        let dir = TempDir::new().expect("tempdir");
        let store = FilesystemStore::open(dir.path()).expect("open store");
        let keys_dir = dir.path().join("keys");
        let keystore = FilesystemKeystore::open(&keys_dir).expect("open keystore");
        let fixture = Self {
            root: dir.path().to_path_buf(),
            store,
            keystore,
            next_seed: Cell::new(1),
        };
        (dir, fixture)
    }

    /// Generate a fresh actor with one ed25519 signing key and one
    /// attestation key (threshold = 1). Persists the genesis to the
    /// store and the signing secret to the keystore. Each call uses
    /// a new seed so successive actors have distinct ids.
    pub fn make_actor(&self) -> CreatedActor {
        let signing_seed = self.next_seed();
        let attestation_seed = self.next_seed();
        let signing = SecretSigningKey::ed25519(seed_bytes(signing_seed));
        let attestation = SecretSigningKey::ed25519(seed_bytes(attestation_seed));

        let genesis = ActorGenesisBody::new(
            ActorKind::person(),
            signing.public_key(),
            vec![attestation.public_key()],
            1,
            Timestamp::from_seconds(FIXTURE_TIMESTAMP),
            seed_bytes(self.next_seed()),
        )
        .expect("genesis well-formed");
        let actor_id = genesis.actor_id();

        self.store.put_actor(&genesis).expect("put_actor");
        self.keystore
            .put_signing_key(&actor_id, &signing)
            .expect("put_signing_key");

        CreatedActor {
            actor_id,
            signing,
            attestation_keys: vec![attestation],
            genesis,
        }
    }

    /// Sign and persist an `ObjectGenesis` for `actor`. Returns the
    /// created object's id and the signed statement.
    pub fn make_object(&self, actor: &CreatedActor, kind: &str) -> CreatedObject {
        let body = ObjectGenesisBody::new(
            ObjectKind::new(kind),
            actor.actor_id.clone(),
            Timestamp::from_seconds(FIXTURE_TIMESTAMP),
            seed_bytes(self.next_seed()),
            None,
        );
        let signature_bytes = actor.signing.sign(&body.canonical_bytes());
        let signature = Signature::new(
            actor.actor_id.clone(),
            actor.signing.public_key().key_id().to_string(),
            "ed25519",
            signature_bytes.bytes().to_vec(),
        );
        let statement = ObjectGenesisStatement::new(body, signature);
        let object_id = self
            .store
            .put_object_genesis(&statement)
            .expect("put_object_genesis");
        CreatedObject {
            object_id,
            genesis: statement,
        }
    }

    /// Sign and persist an `ObjectRevision` for (`actor`, `object`)
    /// with `manifest_text` parsed as the kairo.toml manifest. The
    /// manifest's canonical bytes are persisted as a blob alongside
    /// the revision, mirroring `kairo revision create`.
    ///
    /// `revision_id` is the storage commit reference (typically
    /// `git:sha256:<oid>`); pass `RevisionId::new("git:sha256:r1")`
    /// for tests that don't actually need a real Git commit.
    pub fn make_revision(
        &self,
        actor: &CreatedActor,
        object: &CreatedObject,
        revision_id: RevisionId,
        manifest_text: &str,
        parents: Vec<RevisionId>,
    ) -> CreatedRevision {
        let manifest =
            ObjectManifest::parse_toml(manifest_text).expect("manifest parses");
        let manifest_hash = manifest.manifest_hash();
        let manifest_canonical = manifest.canonical_bytes();
        self.store
            .put_blob(&manifest_hash, &manifest_canonical)
            .expect("put_blob");

        let body = ObjectRevisionBody::new(
            object.object_id.clone(),
            revision_id,
            parents,
            manifest_hash.clone(),
            true,
        );
        let subject: KairoRef = format!("object:{}", object.object_id)
            .parse()
            .expect("subject parse");
        let unsigned = UnsignedStatement::new(
            actor.actor_id.clone(),
            subject,
            Timestamp::from_seconds(FIXTURE_TIMESTAMP),
            body,
        );
        let signature_bytes = actor.signing.sign(&unsigned.canonical_bytes());
        let signature = Signature::new(
            actor.actor_id.clone(),
            actor.signing.public_key().key_id().to_string(),
            "ed25519",
            signature_bytes.bytes().to_vec(),
        );
        let statement = SignedStatement::new(unsigned, signature);
        let statement_id = self
            .store
            .put_object_revision(&statement)
            .expect("put_object_revision");
        CreatedRevision {
            statement_id,
            statement,
            manifest_blob_id: manifest_hash,
        }
    }

    /// Sign and persist an `ObjectBranch` pointing `name` at
    /// `revision`'s statement id. Returns the branch statement.
    pub fn set_branch(
        &self,
        actor: &CreatedActor,
        object: &CreatedObject,
        revision: &CreatedRevision,
        name: &str,
    ) -> CreatedBranch {
        let body = ObjectBranchBody::new(
            object.object_id.clone(),
            name,
            revision.statement_id.clone(),
            None,
        );
        let subject: KairoRef = format!("object:{}", object.object_id)
            .parse()
            .expect("subject parse");
        let unsigned = UnsignedStatement::new(
            actor.actor_id.clone(),
            subject,
            Timestamp::from_seconds(FIXTURE_TIMESTAMP),
            body,
        );
        let signature_bytes = actor.signing.sign(&unsigned.canonical_bytes());
        let signature = Signature::new(
            actor.actor_id.clone(),
            actor.signing.public_key().key_id().to_string(),
            "ed25519",
            signature_bytes.bytes().to_vec(),
        );
        let statement = SignedStatement::new(unsigned, signature);
        let statement_id = self
            .store
            .put_object_branch(&statement)
            .expect("put_object_branch");
        CreatedBranch {
            statement_id,
            statement,
        }
    }

    fn next_seed(&self) -> u8 {
        let s = self.next_seed.get();
        self.next_seed.set(s.wrapping_add(1));
        s
    }
}

fn seed_bytes(seed: u8) -> [u8; 32] {
    [seed; 32]
}

#[derive(Debug, Clone)]
pub struct CreatedActor {
    pub actor_id: ActorId,
    pub signing: SecretSigningKey,
    pub attestation_keys: Vec<SecretSigningKey>,
    pub genesis: ActorGenesisBody,
}

impl CreatedActor {
    /// Public keys of the actor's attestation set (matching
    /// `genesis.attestation_keys` after sort + dedup).
    pub fn attestation_public_keys(&self) -> Vec<PublicKey> {
        self.attestation_keys
            .iter()
            .map(|k| k.public_key())
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct CreatedObject {
    pub object_id: ObjectId,
    pub genesis: ObjectGenesisStatement,
}

#[derive(Debug, Clone)]
pub struct CreatedRevision {
    pub statement_id: StatementId,
    pub statement: SignedStatement<ObjectRevisionBody>,
    pub manifest_blob_id: BlobId,
}

#[derive(Debug, Clone)]
pub struct CreatedBranch {
    pub statement_id: StatementId,
    pub statement: SignedStatement<ObjectBranchBody>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MANIFEST: &str = r#"
        [kairo]
        schema = 1
        kind = "software"
        name = "fixture-smoke"

        [content]
        kind = "tree"
    "#;

    #[test]
    fn make_actor_persists_genesis_and_signing_key() {
        let (_dir, fx) = StoreFixture::temp();
        let actor = fx.make_actor();
        // Store has the actor; keystore has the matching secret.
        let loaded_genesis = fx.store.get_actor(&actor.actor_id).expect("get_actor");
        assert_eq!(loaded_genesis, actor.genesis);
        let loaded_secret = fx
            .keystore
            .get_signing_key(&actor.actor_id)
            .expect("get_signing_key");
        assert_eq!(loaded_secret.public_key(), actor.signing.public_key());
    }

    #[test]
    fn make_object_persists_genesis_and_returns_object_id() {
        let (_dir, fx) = StoreFixture::temp();
        let actor = fx.make_actor();
        let object = fx.make_object(&actor, "software");
        let loaded = fx
            .store
            .get_object_genesis(&object.object_id)
            .expect("get_object_genesis");
        assert_eq!(loaded.body(), object.genesis.body());
    }

    #[test]
    fn full_chain_actor_object_revision_branch() {
        let (_dir, fx) = StoreFixture::temp();
        let actor = fx.make_actor();
        let object = fx.make_object(&actor, "software");
        let revision = fx.make_revision(
            &actor,
            &object,
            RevisionId::new("git:sha256:r1"),
            SAMPLE_MANIFEST,
            vec![],
        );
        let branch = fx.set_branch(&actor, &object, &revision, "head");

        // Manifest blob landed in store.
        let blob = fx
            .store
            .get_blob(&revision.manifest_blob_id)
            .expect("get_blob");
        assert!(!blob.is_empty());

        // Revision and branch are both retrievable.
        let loaded_rev = fx
            .store
            .get_object_revision(&revision.statement_id)
            .expect("get_object_revision");
        assert_eq!(loaded_rev, revision.statement);
        let loaded_branch = fx
            .store
            .get_object_branch(&branch.statement_id)
            .expect("get_object_branch");
        assert_eq!(loaded_branch, branch.statement);
    }

    #[test]
    fn successive_actors_have_distinct_ids() {
        let (_dir, fx) = StoreFixture::temp();
        let a = fx.make_actor();
        let b = fx.make_actor();
        assert_ne!(a.actor_id, b.actor_id);
    }
}
