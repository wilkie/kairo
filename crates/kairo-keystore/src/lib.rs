//! Local keystore for Kairo signing keys.
//!
//! This crate is **MVP only, not production key management.** Secret material
//! is stored as JSON on disk, protected by file-system permissions (0600 on
//! Unix). Passphrase encryption, OS-keychain integration, HSM/PKCS11 support,
//! and key rotation are explicit non-goals for the MVP and will land later as
//! separate work.
//!
//! The crate provides a [`Keystore`] trait and one concrete implementation,
//! [`FilesystemKeystore`], rooted at a directory. Keys are written one file
//! per actor at `<root>/<actor-id>.json` using the `kairo.key.private.v1`
//! schema. No sharding — at MVP scale a user has a handful of keys.
//!
//! Errors mirror `kairo-store`'s model so callers can distinguish:
//!
//! - [`KeystoreError::Missing`] — semantic, the key file is absent.
//! - [`KeystoreError::Corrupt`] — fixity failure (cross-reference mismatch,
//!   parse error, schema mismatch).
//! - [`KeystoreError::Unavailable`] — operational/transient I/O failure.

mod error;
mod json;
mod lock;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kairo_core::ActorId;
use kairo_identity::{KeyId, SecretSigningKey};

pub use error::{CorruptReason, KeystoreError};
pub use json::PrivateKeyJson;

const JSON_SUFFIX: &str = ".json";

/// Persistence interface for secret signing keys, indexed by [`ActorId`].
pub trait Keystore {
    fn put_signing_key(
        &self,
        actor_id: &ActorId,
        secret: &SecretSigningKey,
    ) -> Result<KeyId, KeystoreError>;

    fn get_signing_key(&self, actor_id: &ActorId) -> Result<SecretSigningKey, KeystoreError>;

    fn has_signing_key(&self, actor_id: &ActorId) -> Result<bool, KeystoreError>;

    /// Replace the actor's signing key with a fresh secret. Used by
    /// `kairo actor rotate-key`: after publishing an
    /// `ActorKeyRotation` signed by the prior active key, the
    /// actor's keystore entry must hold the *new* active key so
    /// future commands sign with it.
    ///
    /// Returns the new [`KeyId`]. Errors with [`KeystoreError::Missing`]
    /// if no prior key exists for `actor_id`. Unlike `put_signing_key`,
    /// this method explicitly opts in to overwriting.
    fn replace_signing_key(
        &self,
        actor_id: &ActorId,
        secret: &SecretSigningKey,
    ) -> Result<KeyId, KeystoreError>;

    /// Enumerate every actor that has a signing key in this keystore.
    /// Order is not guaranteed. Used by callers that need to auto-pick
    /// "the" local actor (e.g. CLI `--as` defaulting) and want to
    /// distinguish "exactly one local actor" from "ambiguous, ask the
    /// user."
    fn list_actors(&self) -> Result<Vec<ActorId>, KeystoreError>;
}

/// Filesystem-backed keystore rooted at a single directory.
///
/// On open, the directory is created if missing. On Unix the directory is set
/// to mode `0700` and key files to mode `0600`. Other platforms are
/// best-effort (no mode is set).
#[derive(Debug, Clone)]
pub struct FilesystemKeystore {
    root: PathBuf,
}

impl FilesystemKeystore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, KeystoreError> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        set_dir_permissions(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_for(&self, actor_id: &ActorId) -> PathBuf {
        self.root.join(format!("{actor_id}{JSON_SUFFIX}"))
    }
}

impl Keystore for FilesystemKeystore {
    fn put_signing_key(
        &self,
        actor_id: &ActorId,
        secret: &SecretSigningKey,
    ) -> Result<KeyId, KeystoreError> {
        let path = self.path_for(actor_id);
        lock::with_key_lock(&path, || {
            if path.exists() {
                return Err(KeystoreError::Corrupt {
                    id: actor_id.to_string(),
                    reason: CorruptReason::AlreadyExists,
                });
            }

            let json = PrivateKeyJson::from_secret(actor_id, secret);
            let bytes =
                serde_json::to_vec_pretty(&json).map_err(|error| KeystoreError::Corrupt {
                    id: actor_id.to_string(),
                    reason: CorruptReason::Parse(error.to_string()),
                })?;
            atomic_write(&path, &bytes)?;
            set_file_permissions(&path)?;
            Ok(secret.public_key().key_id())
        })
    }

    fn get_signing_key(&self, actor_id: &ActorId) -> Result<SecretSigningKey, KeystoreError> {
        let path = self.path_for(actor_id);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(KeystoreError::Missing);
            }
            Err(error) => return Err(KeystoreError::Unavailable(error)),
        };

        let json: PrivateKeyJson =
            serde_json::from_slice(&bytes).map_err(|error| KeystoreError::Corrupt {
                id: actor_id.to_string(),
                reason: CorruptReason::Parse(error.to_string()),
            })?;

        json.to_secret(actor_id)
    }

    fn has_signing_key(&self, actor_id: &ActorId) -> Result<bool, KeystoreError> {
        let path = self.path_for(actor_id);
        match fs::metadata(&path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(KeystoreError::Unavailable(error)),
        }
    }

    fn replace_signing_key(
        &self,
        actor_id: &ActorId,
        secret: &SecretSigningKey,
    ) -> Result<KeyId, KeystoreError> {
        let path = self.path_for(actor_id);
        lock::with_key_lock(&path, || {
            if !path.exists() {
                return Err(KeystoreError::Missing);
            }

            let json = PrivateKeyJson::from_secret(actor_id, secret);
            let bytes =
                serde_json::to_vec_pretty(&json).map_err(|error| KeystoreError::Corrupt {
                    id: actor_id.to_string(),
                    reason: CorruptReason::Parse(error.to_string()),
                })?;
            atomic_write(&path, &bytes)?;
            set_file_permissions(&path)?;
            Ok(secret.public_key().key_id())
        })
    }

    fn list_actors(&self) -> Result<Vec<ActorId>, KeystoreError> {
        let mut actors = Vec::new();
        let entries = match fs::read_dir(&self.root) {
            Ok(iter) => iter,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(actors),
            Err(error) => return Err(KeystoreError::Unavailable(error)),
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let actor_id =
                ActorId::new(stem.to_owned()).map_err(|error| KeystoreError::Corrupt {
                    id: stem.to_owned(),
                    reason: CorruptReason::Parse(format!("invalid actor id in keystore: {error}")),
                })?;
            actors.push(actor_id);
        }
        Ok(actors)
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), KeystoreError> {
    let parent = match path.parent() {
        Some(parent) => parent,
        None => {
            return Err(KeystoreError::Unavailable(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path has no parent",
            )));
        }
    };
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("anon");
    let tmp = parent.join(format!(".{file_name}.tmp"));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(unix)]
fn set_dir_permissions(path: &Path) -> Result<(), KeystoreError> {
    use std::os::unix::fs::PermissionsExt;
    let perms = fs::Permissions::from_mode(0o700);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_dir_permissions(_path: &Path) -> Result<(), KeystoreError> {
    Ok(())
}

#[cfg(unix)]
fn set_file_permissions(path: &Path) -> Result<(), KeystoreError> {
    use std::os::unix::fs::PermissionsExt;
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) -> Result<(), KeystoreError> {
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::cast_possible_truncation)]
mod tests {
    use std::fs;

    use kairo_core::Timestamp;
    use kairo_identity::{ActorGenesisBody, ActorKind, SecretSigningKey};
    use tempfile::TempDir;

    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn fresh_secret() -> SecretSigningKey {
        SecretSigningKey::ed25519([7; 32])
    }

    fn fresh_actor_id(secret: &SecretSigningKey) -> ActorId {
        let attestation = SecretSigningKey::ed25519([200; 32]).public_key();
        ActorGenesisBody::new(
            ActorKind::person(),
            secret.public_key(),
            vec![attestation],
            1,
            Timestamp::from_seconds(1_700_000_000),
            [9; 32],
        )
        .expect("genesis well-formed")
        .actor_id()
    }

    fn open_temp() -> Result<(TempDir, FilesystemKeystore), Box<dyn std::error::Error>> {
        let dir = TempDir::new()?;
        let keystore = FilesystemKeystore::open(dir.path())?;
        Ok((dir, keystore))
    }

    #[test]
    fn round_trips_signing_key() -> TestResult {
        let (_dir, keystore) = open_temp()?;
        let secret = fresh_secret();
        let actor_id = fresh_actor_id(&secret);

        let key_id = keystore.put_signing_key(&actor_id, &secret)?;
        assert_eq!(key_id, secret.public_key().key_id());

        let loaded = keystore.get_signing_key(&actor_id)?;
        assert_eq!(loaded.seed_bytes(), secret.seed_bytes());
        assert_eq!(loaded.public_key(), secret.public_key());
        Ok(())
    }

    #[test]
    fn missing_returns_missing() -> TestResult {
        let (_dir, keystore) = open_temp()?;
        let secret = fresh_secret();
        let actor_id = fresh_actor_id(&secret);
        assert!(matches!(
            keystore.get_signing_key(&actor_id),
            Err(KeystoreError::Missing)
        ));
        Ok(())
    }

    #[test]
    fn has_signing_key_reports_presence() -> TestResult {
        let (_dir, keystore) = open_temp()?;
        let secret = fresh_secret();
        let actor_id = fresh_actor_id(&secret);

        assert!(!keystore.has_signing_key(&actor_id)?);
        keystore.put_signing_key(&actor_id, &secret)?;
        assert!(keystore.has_signing_key(&actor_id)?);
        Ok(())
    }

    #[test]
    fn refuses_to_overwrite() -> TestResult {
        let (_dir, keystore) = open_temp()?;
        let secret = fresh_secret();
        let actor_id = fresh_actor_id(&secret);
        keystore.put_signing_key(&actor_id, &secret)?;

        assert!(matches!(
            keystore.put_signing_key(&actor_id, &secret),
            Err(KeystoreError::Corrupt {
                reason: CorruptReason::AlreadyExists,
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn loaded_key_signs_identically() -> TestResult {
        let (_dir, keystore) = open_temp()?;
        let secret = fresh_secret();
        let actor_id = fresh_actor_id(&secret);
        keystore.put_signing_key(&actor_id, &secret)?;

        let loaded = keystore.get_signing_key(&actor_id)?;
        let payload = b"kairo payload";
        assert_eq!(loaded.sign(payload), secret.sign(payload));
        Ok(())
    }

    #[test]
    fn tampered_actor_id_field_is_corrupt() -> TestResult {
        let (dir, keystore) = open_temp()?;
        let secret = fresh_secret();
        let actor_id = fresh_actor_id(&secret);
        keystore.put_signing_key(&actor_id, &secret)?;

        let path = dir.path().join(format!("{actor_id}.json"));
        let raw = fs::read_to_string(&path)?;
        // Replace stored actor_id with a different one (still well-formed) so
        // the cross-reference fails on load.
        let other_actor = fresh_actor_id(&SecretSigningKey::ed25519([8; 32]));
        let tampered = raw.replace(actor_id.as_str(), other_actor.as_str());
        fs::write(&path, tampered)?;

        assert!(matches!(
            keystore.get_signing_key(&actor_id),
            Err(KeystoreError::Corrupt {
                reason: CorruptReason::ActorIdMismatch { .. },
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn tampered_key_id_field_is_corrupt() -> TestResult {
        let (dir, keystore) = open_temp()?;
        let secret = fresh_secret();
        let actor_id = fresh_actor_id(&secret);
        keystore.put_signing_key(&actor_id, &secret)?;

        let path = dir.path().join(format!("{actor_id}.json"));
        let mut json: PrivateKeyJson = serde_json::from_slice(&fs::read(&path)?)?;
        // Replace stored key_id with a derivation from a different secret;
        // when the keystore re-derives, it'll mismatch.
        let other_key_id = SecretSigningKey::ed25519([8; 32])
            .public_key()
            .key_id()
            .to_string();
        json.key_id = other_key_id;
        fs::write(&path, serde_json::to_vec_pretty(&json)?)?;

        assert!(matches!(
            keystore.get_signing_key(&actor_id),
            Err(KeystoreError::Corrupt {
                reason: CorruptReason::KeyIdMismatch { .. },
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn unparseable_file_is_corrupt() -> TestResult {
        let (dir, keystore) = open_temp()?;
        let secret = fresh_secret();
        let actor_id = fresh_actor_id(&secret);
        keystore.put_signing_key(&actor_id, &secret)?;

        fs::write(dir.path().join(format!("{actor_id}.json")), b"not json")?;

        assert!(matches!(
            keystore.get_signing_key(&actor_id),
            Err(KeystoreError::Corrupt {
                reason: CorruptReason::Parse(_),
                ..
            })
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn key_file_has_restricted_permissions() -> TestResult {
        use std::os::unix::fs::PermissionsExt;
        let (dir, keystore) = open_temp()?;
        let secret = fresh_secret();
        let actor_id = fresh_actor_id(&secret);
        keystore.put_signing_key(&actor_id, &secret)?;

        let path = dir.path().join(format!("{actor_id}.json"));
        let mode = fs::metadata(&path)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        Ok(())
    }

    #[test]
    fn list_actors_enumerates_stored_keys() -> TestResult {
        let (_dir, keystore) = open_temp()?;
        assert!(keystore.list_actors()?.is_empty());

        let secret_a = SecretSigningKey::ed25519([1; 32]);
        let actor_a = fresh_actor_id(&secret_a);
        keystore.put_signing_key(&actor_a, &secret_a)?;

        let secret_b = SecretSigningKey::ed25519([2; 32]);
        let actor_b = fresh_actor_id(&secret_b);
        keystore.put_signing_key(&actor_b, &secret_b)?;

        let actors = keystore.list_actors()?;
        assert_eq!(actors.len(), 2);
        assert!(actors.contains(&actor_a));
        assert!(actors.contains(&actor_b));
        Ok(())
    }

    #[test]
    fn list_actors_ignores_non_json_files() -> TestResult {
        let (dir, keystore) = open_temp()?;
        let secret = fresh_secret();
        let actor_id = fresh_actor_id(&secret);
        keystore.put_signing_key(&actor_id, &secret)?;

        // Junk files should be skipped, not parse-errored.
        fs::write(dir.path().join("README.txt"), b"not a key")?;
        fs::write(dir.path().join(".hidden"), b"also not a key")?;

        let actors = keystore.list_actors()?;
        assert_eq!(actors, vec![actor_id]);
        Ok(())
    }

    /// Concurrent `put_signing_key` calls for the *same* actor must
    /// produce exactly one stored key — the first writer wins, every
    /// other writer gets `AlreadyExists`. The advisory lock around
    /// the existence-check + write is what makes this hold; without
    /// it two writers would both pass the `path.exists()` check
    /// before either had written.
    #[test]
    fn concurrent_put_for_same_actor_admits_exactly_one() -> TestResult {
        const THREADS: usize = 8;
        let (_dir, keystore) = open_temp()?;
        let secret = fresh_secret();
        let actor_id = fresh_actor_id(&secret);

        let successes = std::sync::atomic::AtomicUsize::new(0);
        let already_exists = std::sync::atomic::AtomicUsize::new(0);

        std::thread::scope(|scope| {
            for _ in 0..THREADS {
                let keystore = keystore.clone();
                let actor_id = actor_id.clone();
                let secret = SecretSigningKey::ed25519(*secret.seed_bytes());
                let successes = &successes;
                let already_exists = &already_exists;
                scope.spawn(move || match keystore.put_signing_key(&actor_id, &secret) {
                    Ok(_) => {
                        successes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    Err(KeystoreError::Corrupt {
                        reason: CorruptReason::AlreadyExists,
                        ..
                    }) => {
                        already_exists.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    Err(error) => panic!("unexpected error: {error}"),
                });
            }
        });

        assert_eq!(successes.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            already_exists.load(std::sync::atomic::Ordering::SeqCst),
            THREADS - 1
        );
        // The stored key must round-trip cleanly — no half-written file.
        let loaded = keystore.get_signing_key(&actor_id)?;
        assert_eq!(loaded.seed_bytes(), secret.seed_bytes());
        Ok(())
    }

    /// Concurrent `put_signing_key` calls for *distinct* actors all
    /// succeed — they take per-actor lock files, so they don't
    /// serialize against each other. Verifies the lock granularity.
    #[test]
    fn concurrent_put_for_distinct_actors_all_succeed() -> TestResult {
        const THREADS: usize = 8;
        let (_dir, keystore) = open_temp()?;

        std::thread::scope(|scope| {
            for index in 0..THREADS {
                let keystore = keystore.clone();
                scope.spawn(move || {
                    let secret = SecretSigningKey::ed25519([index as u8 + 1; 32]);
                    let actor_id = fresh_actor_id(&secret);
                    keystore
                        .put_signing_key(&actor_id, &secret)
                        .expect("put succeeds for distinct actor");
                });
            }
        });

        let actors = keystore.list_actors()?;
        assert_eq!(actors.len(), THREADS);
        Ok(())
    }
}
