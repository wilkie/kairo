//! Actor identity and signature primitives.

pub mod json;

use std::error::Error;
use std::fmt;

use std::collections::BTreeMap;

use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use kairo_core::canonical::{encode_bytes, encode_list, encode_str, encode_u8, CanonicalEncode};
pub use kairo_core::ActorId;
use kairo_core::{BlobId, Timestamp};

/// Canonical ActorGenesis v1 encoding is documented at
/// `schemas/canonical/actor-genesis-v1.md`.
const ACTOR_GENESIS_DOMAIN: &[u8] = b"kairo.actor.genesis.v1";
const ACTOR_KEY_DOMAIN: &[u8] = b"kairo.actor.key.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorGenesisBody {
    actor_kind: ActorKind,
    initial_key: PublicKey,
    /// Cold-storage attestation keys. Always non-empty, sorted by raw
    /// public-key bytes ascending (set at construction time), and
    /// disjoint from `initial_key`. Part of canonical bytes / `ActorId`.
    /// See `schemas/canonical/actor-genesis-v1.md` and `ACTORS.md` §5.5.2.
    attestation_keys: Vec<PublicKey>,
    /// M of the M-of-N quorum required to sign attestation-surface
    /// emergency statements. `1 ≤ attestation_threshold ≤
    /// attestation_keys.len()`. Always explicit; no default.
    /// Part of canonical bytes / `ActorId`. See `ACTORS.md` §5.5.3.
    attestation_threshold: u8,
    created_at: Timestamp,
    nonce: [u8; 32],
}

impl ActorGenesisBody {
    /// Construct an `ActorGenesisBody`.
    ///
    /// `attestation_keys` must be non-empty and disjoint from
    /// `initial_key`. `attestation_threshold` must satisfy
    /// `1 ≤ threshold ≤ attestation_keys.len()`. The constructor sorts
    /// and deduplicates the attestation set so identical inputs (modulo
    /// order) produce the same canonical bytes and therefore the same
    /// `ActorId`.
    pub fn new(
        actor_kind: ActorKind,
        initial_key: PublicKey,
        attestation_keys: Vec<PublicKey>,
        attestation_threshold: u8,
        created_at: Timestamp,
        nonce: [u8; 32],
    ) -> Result<Self, ActorGenesisShapeError> {
        if attestation_keys.is_empty() {
            return Err(ActorGenesisShapeError::EmptyAttestationKeys);
        }
        let mut sorted: Vec<PublicKey> = attestation_keys;
        sorted.sort_by(|a, b| a.bytes().cmp(b.bytes()));
        sorted.dedup_by(|a, b| a.bytes() == b.bytes());
        if sorted.iter().any(|key| key.bytes() == initial_key.bytes()) {
            return Err(ActorGenesisShapeError::AttestationKeySharesSigningKey);
        }
        if attestation_threshold < 1 {
            return Err(ActorGenesisShapeError::ThresholdTooSmall);
        }
        if (attestation_threshold as usize) > sorted.len() {
            return Err(ActorGenesisShapeError::ThresholdExceedsKeyCount {
                threshold: attestation_threshold,
                key_count: sorted.len(),
            });
        }
        Ok(Self {
            actor_kind,
            initial_key,
            attestation_keys: sorted,
            attestation_threshold,
            created_at,
            nonce,
        })
    }

    pub fn actor_kind(&self) -> &ActorKind {
        &self.actor_kind
    }

    pub fn initial_key(&self) -> &PublicKey {
        &self.initial_key
    }

    pub fn attestation_keys(&self) -> &[PublicKey] {
        &self.attestation_keys
    }

    pub fn attestation_threshold(&self) -> u8 {
        self.attestation_threshold
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }

    pub fn actor_id(&self) -> ActorId {
        ActorId::from_bytes(ACTOR_GENESIS_DOMAIN, &self.canonical_bytes())
    }
}

impl CanonicalEncode for ActorGenesisBody {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, "ActorGenesis");
        encode_u8(out, 1);
        encode_str(out, self.actor_kind.as_str());
        self.initial_key.encode_canonical(out);
        encode_list(out, &self.attestation_keys, |out, key| {
            key.encode_canonical(out);
        });
        encode_u8(out, self.attestation_threshold);
        self.created_at.encode_canonical(out);
        encode_bytes(out, &self.nonce);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorGenesisShapeError {
    EmptyAttestationKeys,
    AttestationKeySharesSigningKey,
    ThresholdTooSmall,
    ThresholdExceedsKeyCount { threshold: u8, key_count: usize },
}

impl fmt::Display for ActorGenesisShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAttestationKeys => f.write_str(
                "ActorGenesis requires at least one attestation key (see ACTORS.md §5.5.2)",
            ),
            Self::AttestationKeySharesSigningKey => f.write_str(
                "ActorGenesis attestation_keys must be disjoint from initial_key",
            ),
            Self::ThresholdTooSmall => f.write_str(
                "ActorGenesis.attestation_threshold must be >= 1 (see ACTORS.md §5.5.3)",
            ),
            Self::ThresholdExceedsKeyCount {
                threshold,
                key_count,
            } => write!(
                f,
                "ActorGenesis.attestation_threshold {threshold} exceeds attestation_keys count {key_count}",
            ),
        }
    }
}

impl Error for ActorGenesisShapeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorKind(String);

impl ActorKind {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn person() -> Self {
        Self("person".to_owned())
    }

    pub fn project() -> Self {
        Self("project".to_owned())
    }

    pub fn organization() -> Self {
        Self("organization".to_owned())
    }

    pub fn service() -> Self {
        Self("service".to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(String);

impl KeyId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn from_public_key(public_key: &PublicKey) -> Self {
        Self(BlobId::from_bytes(ACTOR_KEY_DOMAIN, &public_key.canonical_bytes()).to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureAlgorithm {
    Ed25519,
}

impl SignatureAlgorithm {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ed25519 => "ed25519",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey {
    algorithm: SignatureAlgorithm,
    bytes: [u8; 32],
}

impl PublicKey {
    pub fn ed25519(bytes: [u8; 32]) -> Self {
        Self {
            algorithm: SignatureAlgorithm::Ed25519,
            bytes,
        }
    }

    pub fn algorithm(&self) -> SignatureAlgorithm {
        self.algorithm
    }

    pub fn bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    pub fn key_id(&self) -> KeyId {
        KeyId::from_public_key(self)
    }
}

impl CanonicalEncode for PublicKey {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, self.algorithm.as_str());
        encode_bytes(out, &self.bytes);
    }
}

/// A secret signing key.
///
/// Holds the raw seed/scalar bytes. The `Debug` impl deliberately redacts the
/// secret material; `seed_bytes()` exposes it for serialization or storage.
/// Callers should treat the result of `seed_bytes()` as sensitive.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretSigningKey {
    algorithm: SignatureAlgorithm,
    seed: [u8; 32],
}

impl SecretSigningKey {
    pub fn ed25519(seed: [u8; 32]) -> Self {
        Self {
            algorithm: SignatureAlgorithm::Ed25519,
            seed,
        }
    }

    /// Generate a fresh ed25519 key using OS randomness.
    pub fn generate_ed25519() -> Result<Self, KeyGenerationError> {
        Ok(Self::ed25519(generate_random_bytes()?))
    }

    pub fn algorithm(&self) -> SignatureAlgorithm {
        self.algorithm
    }

    /// Returns the raw seed bytes. **Sensitive.**
    pub fn seed_bytes(&self) -> &[u8; 32] {
        &self.seed
    }

    pub fn public_key(&self) -> PublicKey {
        match self.algorithm {
            SignatureAlgorithm::Ed25519 => {
                let signing = SigningKey::from_bytes(&self.seed);
                PublicKey::ed25519(signing.verifying_key().to_bytes())
            }
        }
    }

    pub fn sign(&self, payload: &[u8]) -> SignatureBytes {
        match self.algorithm {
            SignatureAlgorithm::Ed25519 => {
                let signing = SigningKey::from_bytes(&self.seed);
                SignatureBytes::ed25519(signing.sign(payload).to_bytes())
            }
        }
    }
}

impl fmt::Debug for SecretSigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretSigningKey")
            .field("algorithm", &self.algorithm)
            .field("seed", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyGenerationError {
    pub reason: String,
}

impl fmt::Display for KeyGenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "key generation failed: {}", self.reason)
    }
}

impl Error for KeyGenerationError {}

/// Generate 32 fresh random bytes from the OS RNG.
///
/// Suitable for genesis nonces and other places where a fresh per-record
/// uniqueness token is needed.
pub fn generate_nonce() -> Result<[u8; 32], KeyGenerationError> {
    generate_random_bytes()
}

fn generate_random_bytes() -> Result<[u8; 32], KeyGenerationError> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|error| KeyGenerationError {
        reason: error.to_string(),
    })?;
    Ok(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureBytes {
    algorithm: SignatureAlgorithm,
    bytes: [u8; 64],
}

impl SignatureBytes {
    pub fn ed25519(bytes: [u8; 64]) -> Self {
        Self {
            algorithm: SignatureAlgorithm::Ed25519,
            bytes,
        }
    }

    pub fn algorithm(&self) -> SignatureAlgorithm {
        self.algorithm
    }

    pub fn bytes(&self) -> &[u8; 64] {
        &self.bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSignature;

pub fn verify_signature(
    public_key: &PublicKey,
    payload: &[u8],
    signature: &SignatureBytes,
) -> Result<VerifiedSignature, SignatureVerificationError> {
    if public_key.algorithm() != signature.algorithm() {
        return Err(SignatureVerificationError::AlgorithmMismatch {
            public_key: public_key.algorithm(),
            signature: signature.algorithm(),
        });
    }

    match public_key.algorithm() {
        SignatureAlgorithm::Ed25519 => {
            verify_ed25519(public_key.bytes(), payload, signature.bytes())
        }
    }
}

/// Which signing surface produced a key event.
///
/// - `Operational`: signed by the actor's currently active signing key
///   (`ActorKeyRotation`, `ActorKeyRevocation`).
/// - `Attestation`: signed by an attestation key
///   (`ActorEmergencyKeyRotation`, `ActorEmergencyKeyRevocation`).
///
/// Both surfaces contribute to the same per-actor key-event chain; the
/// distinction matters only for the verifier's signing-surface dispatch.
/// See `ACTORS.md` §5.5.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySurface {
    Operational,
    Attestation,
}

/// One entry in the per-actor rotation chain. The first rotation has
/// `supersedes = None`; the genesis-initial key is implicit and is
/// not represented as an entry.
///
/// Index modules in `kairo-store` produce these summaries from the
/// underlying signed rotation statements (routine and emergency); the
/// resolver trait consumes them here so verification can stay decoupled
/// from the storage layout. `surface` records which signing surface
/// produced the rotation — informational for the resolver, used by the
/// verifier when checking the signing-surface rule on the rotation
/// statement itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRotationEntry {
    pub statement_id: String,
    pub next_key: PublicKey,
    pub created_at: Timestamp,
    pub supersedes: Option<String>,
    pub surface: KeySurface,
}

/// One entry in the per-actor revocation set. Revocations are
/// standalone (no `supersedes` chain). `surface` records which
/// signing surface produced the revocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRevocationEntry {
    pub statement_id: String,
    pub revoked_key: KeyId,
    pub retroactive: bool,
    pub created_at: Timestamp,
    pub surface: KeySurface,
}

/// One entry in the per-actor attestation-key add set. Order does not
/// matter; duplicates collapse via key bytes equality. See
/// `schemas/canonical/actor-attestation-key-add-v1.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestationKeyAddEntry {
    pub statement_id: String,
    pub new_key: PublicKey,
    pub created_at: Timestamp,
}

/// One entry in the per-actor attestation-key revocation set. There is
/// no `retroactive` flag and no `surface` field — these statements are
/// always signed by an attestation key (`ACTORS.md` §5.5.2 enforces
/// this at the verifier) and revocation never applies retroactively
/// to emergency events the key signed before `created_at`. See
/// `schemas/canonical/actor-attestation-key-revocation-v1.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestationKeyRevocationEntry {
    pub statement_id: String,
    pub revoked_key: KeyId,
    pub created_at: Timestamp,
}

/// One entry in the per-actor attestation-threshold change set. The
/// resolver walks these to materialize the live threshold at a given
/// `created_at`. The asymmetric authority rule (raises require
/// `max(current, new)` distinct sigs, lowers require `current`) is
/// enforced upstream at put-time and at the verifier. See
/// `schemas/canonical/actor-attestation-threshold-change-v1.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestationThresholdChangeEntry {
    pub statement_id: String,
    pub new_threshold: u8,
    pub created_at: Timestamp,
}

pub trait ActorResolver {
    fn actor_genesis(&self, actor: &ActorId)
        -> Result<Option<ActorGenesisBody>, ActorResolveError>;

    fn initial_key(&self, actor: &ActorId) -> Result<Option<PublicKey>, ActorResolveError> {
        Ok(self
            .actor_genesis(actor)?
            .map(|genesis| genesis.initial_key().clone()))
    }

    /// Per-actor rotation chain in storage order. The default
    /// implementation returns an empty list, in which case the
    /// active key collapses to the genesis-initial key for every
    /// timestamp.
    fn key_rotations(
        &self,
        _actor: &ActorId,
    ) -> Result<Vec<KeyRotationEntry>, ActorResolveError> {
        Ok(Vec::new())
    }

    /// Per-actor revocation set in storage order. The default
    /// implementation returns an empty set, in which case
    /// `is_key_revoked_at` always returns `false`.
    fn key_revocations(
        &self,
        _actor: &ActorId,
    ) -> Result<Vec<KeyRevocationEntry>, ActorResolveError> {
        Ok(Vec::new())
    }

    /// Per-actor `ActorAttestationKeyAdd` entries in storage order.
    /// The default implementation returns an empty list, in which case
    /// `attestation_keys_at` collapses to the genesis-declared
    /// attestation set.
    fn attestation_key_adds(
        &self,
        _actor: &ActorId,
    ) -> Result<Vec<AttestationKeyAddEntry>, ActorResolveError> {
        Ok(Vec::new())
    }

    /// Per-actor `ActorAttestationKeyRevocation` entries in storage
    /// order. The default implementation returns an empty list, in
    /// which case `attestation_keys_at` reduces to "genesis ∪ adds"
    /// with no removal.
    fn attestation_key_revocations(
        &self,
        _actor: &ActorId,
    ) -> Result<Vec<AttestationKeyRevocationEntry>, ActorResolveError> {
        Ok(Vec::new())
    }

    /// Per-actor `ActorAttestationThresholdChange` entries in storage
    /// order. The default implementation returns an empty list, in
    /// which case `attestation_threshold_at` collapses to
    /// `ActorGenesis.attestation_threshold`.
    fn attestation_threshold_changes(
        &self,
        _actor: &ActorId,
    ) -> Result<Vec<AttestationThresholdChangeEntry>, ActorResolveError> {
        Ok(Vec::new())
    }

    /// Resolve the actor's attestation threshold (M of the M-of-N
    /// quorum) at causal position `at`. Returns the most-recent
    /// threshold change with `created_at <= at`, or
    /// `ActorGenesis.attestation_threshold` when none precedes `at`.
    /// Tiebreak on equal `created_at` is `statement_id` ascending
    /// (lexicographic). Returns `None` if the actor genesis is
    /// unknown. See `ACTORS.md` §5.5.3.
    fn attestation_threshold_at(
        &self,
        actor: &ActorId,
        at: Timestamp,
    ) -> Result<Option<u8>, ActorResolveError> {
        let genesis = match self.actor_genesis(actor)? {
            Some(genesis) => genesis,
            None => return Ok(None),
        };
        let mut latest: Option<&AttestationThresholdChangeEntry> = None;
        let entries = self.attestation_threshold_changes(actor)?;
        for entry in &entries {
            if entry.created_at > at {
                continue;
            }
            latest = Some(match latest {
                None => entry,
                Some(current)
                    if entry.created_at > current.created_at
                        || (entry.created_at == current.created_at
                            && entry.statement_id > current.statement_id) =>
                {
                    entry
                }
                Some(current) => current,
            });
        }
        Ok(Some(match latest {
            Some(entry) => entry.new_threshold,
            None => genesis.attestation_threshold(),
        }))
    }

    /// Resolve the actor's attestation key set at causal position `at`.
    ///
    /// The set is `(ActorGenesis.attestation_keys ∪ { add.new_key | add
    /// ∈ attestation_key_adds(actor) where add.created_at <= at })
    /// ∖ { rev.revoked_key | rev ∈ attestation_key_revocations(actor)
    /// where rev.created_at <= at }`. Order is irrelevant; duplicates
    /// collapse via `KeyId` equality. The returned map is keyed by
    /// `KeyId` to make the verifier's "is this signature key id in the
    /// set?" lookup O(log n), and carries the `PublicKey` material so
    /// the verifier can check the signature bytes without a second
    /// trip through the resolver. See `ACTORS.md` §5.5.2.
    fn attestation_keys_at(
        &self,
        actor: &ActorId,
        at: Timestamp,
    ) -> Result<BTreeMap<KeyId, PublicKey>, ActorResolveError> {
        let mut set: BTreeMap<KeyId, PublicKey> = BTreeMap::new();
        if let Some(genesis) = self.actor_genesis(actor)? {
            for key in genesis.attestation_keys() {
                set.insert(key.key_id(), key.clone());
            }
        }
        for entry in self.attestation_key_adds(actor)? {
            if entry.created_at <= at {
                set.insert(entry.new_key.key_id(), entry.new_key);
            }
        }
        for entry in self.attestation_key_revocations(actor)? {
            if entry.created_at <= at {
                set.remove(&entry.revoked_key);
            }
        }
        Ok(set)
    }

    /// Resolve the actor's active signing key at causal position `at`.
    ///
    /// The active key is the rotation chain leaf with `created_at <= at`,
    /// considering only same-actor `supersedes` edges. Chain
    /// precedence wins over `(created_at, statement_id)` ordering;
    /// fork tiebreak is `(created_at, statement_id)` descending.
    /// Falls back to `ActorGenesis.initial_key` if no rotation
    /// precedes `at`.
    fn active_key_at(
        &self,
        actor: &ActorId,
        at: Timestamp,
    ) -> Result<Option<PublicKey>, ActorResolveError> {
        let rotations = self.key_rotations(actor)?;
        let eligible: Vec<&KeyRotationEntry> = rotations
            .iter()
            .filter(|entry| entry.created_at <= at)
            .collect();
        if eligible.is_empty() {
            return self.initial_key(actor);
        }
        let superseded: std::collections::HashSet<&str> = eligible
            .iter()
            .filter_map(|entry| entry.supersedes.as_deref())
            .collect();
        let mut best: Option<&KeyRotationEntry> = None;
        for entry in &eligible {
            if superseded.contains(entry.statement_id.as_str()) {
                continue;
            }
            best = Some(match best {
                None => entry,
                Some(current) if rotation_greater(entry, current) => entry,
                Some(current) => current,
            });
        }
        Ok(best.map(|entry| entry.next_key.clone()))
    }

    /// Whether `(actor, key_id)` is revoked at causal position `at`.
    ///
    /// A key is revoked iff some revocation matches with either
    /// `retroactive = true` or `created_at <= at`. Most-restrictive
    /// interpretation wins on duplicates.
    fn is_key_revoked_at(
        &self,
        actor: &ActorId,
        key_id: &KeyId,
        at: Timestamp,
    ) -> Result<bool, ActorResolveError> {
        Ok(self
            .key_revocations(actor)?
            .into_iter()
            .any(|entry| {
                entry.revoked_key == *key_id && (entry.retroactive || entry.created_at <= at)
            }))
    }
}

fn rotation_greater(candidate: &KeyRotationEntry, current: &KeyRotationEntry) -> bool {
    if candidate.created_at > current.created_at {
        return true;
    }
    if candidate.created_at < current.created_at {
        return false;
    }
    candidate.statement_id > current.statement_id
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryActorResolver {
    actors: BTreeMap<ActorId, ActorGenesisBody>,
    rotations: BTreeMap<ActorId, Vec<KeyRotationEntry>>,
    revocations: BTreeMap<ActorId, Vec<KeyRevocationEntry>>,
    attestation_adds: BTreeMap<ActorId, Vec<AttestationKeyAddEntry>>,
    attestation_revocations: BTreeMap<ActorId, Vec<AttestationKeyRevocationEntry>>,
    threshold_changes: BTreeMap<ActorId, Vec<AttestationThresholdChangeEntry>>,
}

impl MemoryActorResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, genesis: ActorGenesisBody) -> ActorId {
        let actor_id = genesis.actor_id();
        self.actors.insert(actor_id.clone(), genesis);
        actor_id
    }

    pub fn insert_rotation(&mut self, actor: ActorId, entry: KeyRotationEntry) {
        self.rotations.entry(actor).or_default().push(entry);
    }

    pub fn insert_revocation(&mut self, actor: ActorId, entry: KeyRevocationEntry) {
        self.revocations.entry(actor).or_default().push(entry);
    }

    pub fn insert_attestation_add(&mut self, actor: ActorId, entry: AttestationKeyAddEntry) {
        self.attestation_adds.entry(actor).or_default().push(entry);
    }

    pub fn insert_attestation_revocation(
        &mut self,
        actor: ActorId,
        entry: AttestationKeyRevocationEntry,
    ) {
        self.attestation_revocations
            .entry(actor)
            .or_default()
            .push(entry);
    }

    pub fn insert_threshold_change(
        &mut self,
        actor: ActorId,
        entry: AttestationThresholdChangeEntry,
    ) {
        self.threshold_changes.entry(actor).or_default().push(entry);
    }

    pub fn len(&self) -> usize {
        self.actors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }
}

impl ActorResolver for MemoryActorResolver {
    fn actor_genesis(
        &self,
        actor: &ActorId,
    ) -> Result<Option<ActorGenesisBody>, ActorResolveError> {
        Ok(self.actors.get(actor).cloned())
    }

    fn key_rotations(
        &self,
        actor: &ActorId,
    ) -> Result<Vec<KeyRotationEntry>, ActorResolveError> {
        Ok(self.rotations.get(actor).cloned().unwrap_or_default())
    }

    fn key_revocations(
        &self,
        actor: &ActorId,
    ) -> Result<Vec<KeyRevocationEntry>, ActorResolveError> {
        Ok(self.revocations.get(actor).cloned().unwrap_or_default())
    }

    fn attestation_key_adds(
        &self,
        actor: &ActorId,
    ) -> Result<Vec<AttestationKeyAddEntry>, ActorResolveError> {
        Ok(self.attestation_adds.get(actor).cloned().unwrap_or_default())
    }

    fn attestation_key_revocations(
        &self,
        actor: &ActorId,
    ) -> Result<Vec<AttestationKeyRevocationEntry>, ActorResolveError> {
        Ok(self
            .attestation_revocations
            .get(actor)
            .cloned()
            .unwrap_or_default())
    }

    fn attestation_threshold_changes(
        &self,
        actor: &ActorId,
    ) -> Result<Vec<AttestationThresholdChangeEntry>, ActorResolveError> {
        Ok(self
            .threshold_changes
            .get(actor)
            .cloned()
            .unwrap_or_default())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorResolveError {
    Unavailable(String),
}

impl fmt::Display for ActorResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(reason) => write!(f, "actor resolver unavailable: {reason}"),
        }
    }
}

impl Error for ActorResolveError {}

fn verify_ed25519(
    public_key: &[u8; 32],
    payload: &[u8],
    signature: &[u8; 64],
) -> Result<VerifiedSignature, SignatureVerificationError> {
    let public_key = VerifyingKey::from_bytes(public_key)
        .map_err(|_| SignatureVerificationError::InvalidPublicKey)?;
    let signature = ed25519_dalek::Signature::from_bytes(signature);

    public_key
        .verify(payload, &signature)
        .map(|()| VerifiedSignature)
        .map_err(|_| SignatureVerificationError::InvalidSignature)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureVerificationError {
    AlgorithmMismatch {
        public_key: SignatureAlgorithm,
        signature: SignatureAlgorithm,
    },
    InvalidPublicKey,
    InvalidSignature,
}

impl fmt::Display for SignatureVerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlgorithmMismatch {
                public_key,
                signature,
            } => write!(
                f,
                "signature algorithm {} does not match public key algorithm {}",
                signature.as_str(),
                public_key.as_str()
            ),
            Self::InvalidPublicKey => f.write_str("invalid public key"),
            Self::InvalidSignature => f.write_str("invalid signature"),
        }
    }
}

impl Error for SignatureVerificationError {}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    // proptest is used by tests/property_tests.rs only.
    use proptest as _;

    use super::*;

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn public_key() -> PublicKey {
        PublicKey::ed25519(signing_key().verifying_key().to_bytes())
    }

    /// Test attestation key. Disjoint from `public_key()` (seed [7; 32]),
    /// `other_public_key()` (seed [8; 32]), and `third_public_key()`
    /// (seed [9; 32]) so no test fixture's attestation set collides with
    /// the operational signing surface.
    fn attestation_key() -> PublicKey {
        PublicKey::ed25519(SigningKey::from_bytes(&[200; 32]).verifying_key().to_bytes())
    }

    fn timestamp() -> Timestamp {
        Timestamp::from_seconds(1_700_000_000)
    }

    #[test]
    fn same_actor_genesis_produces_same_actor_id() {
        let first = ActorGenesisBody::new(ActorKind::person(), public_key(), vec![attestation_key()], 1, timestamp(), [9; 32]).expect("genesis well-formed");
        let second = ActorGenesisBody::new(ActorKind::person(), public_key(), vec![attestation_key()], 1, timestamp(), [9; 32]).expect("genesis well-formed");

        assert_eq!(first.actor_id(), second.actor_id());
    }

    #[test]
    fn actor_genesis_nonce_changes_actor_id() {
        let first = ActorGenesisBody::new(ActorKind::person(), public_key(), vec![attestation_key()], 1, timestamp(), [9; 32]).expect("genesis well-formed");
        let second =
            ActorGenesisBody::new(ActorKind::person(), public_key(), vec![attestation_key()], 1, timestamp(), [10; 32]).expect("genesis well-formed");

        assert_ne!(first.actor_id(), second.actor_id());
    }

    #[test]
    fn actor_genesis_key_changes_actor_id() {
        let first = ActorGenesisBody::new(ActorKind::person(), public_key(), vec![attestation_key()], 1, timestamp(), [9; 32]).expect("genesis well-formed");
        let second_key =
            PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes());
        let second = ActorGenesisBody::new(ActorKind::person(), second_key, vec![attestation_key()], 1, timestamp(), [9; 32]).expect("genesis well-formed");

        assert_ne!(first.actor_id(), second.actor_id());
    }

    #[test]
    fn actor_genesis_created_at_changes_actor_id() {
        let first = ActorGenesisBody::new(ActorKind::person(), public_key(), vec![attestation_key()], 1, timestamp(), [9; 32]).expect("genesis well-formed");
        let second = ActorGenesisBody::new(
            ActorKind::person(),
            public_key(),
            vec![attestation_key()],
            1,
            Timestamp::from_seconds(timestamp().seconds() + 1),
            [9; 32],
        )
        .expect("genesis well-formed");

        assert_ne!(first.actor_id(), second.actor_id());
    }

    #[test]
    fn key_id_is_stable_for_same_public_key() {
        assert_eq!(public_key().key_id(), public_key().key_id());
    }

    #[test]
    fn verifies_ed25519_signature() {
        let payload = b"kairo payload";
        let signature = signing_key().sign(payload).to_bytes();

        assert_eq!(
            verify_signature(&public_key(), payload, &SignatureBytes::ed25519(signature)),
            Ok(VerifiedSignature)
        );
    }

    #[test]
    fn rejects_changed_payload() {
        let signature = signing_key().sign(b"kairo payload").to_bytes();

        assert_eq!(
            verify_signature(
                &public_key(),
                b"changed payload",
                &SignatureBytes::ed25519(signature)
            ),
            Err(SignatureVerificationError::InvalidSignature)
        );
    }

    #[test]
    fn rejects_changed_signature() {
        let payload = b"kairo payload";
        let mut signature = signing_key().sign(payload).to_bytes();
        signature[0] ^= 1;

        assert_eq!(
            verify_signature(&public_key(), payload, &SignatureBytes::ed25519(signature)),
            Err(SignatureVerificationError::InvalidSignature)
        );
    }

    #[test]
    fn secret_signing_key_round_trips_public_key() {
        let secret = SecretSigningKey::ed25519([7; 32]);
        assert_eq!(secret.public_key(), public_key());
    }

    #[test]
    fn secret_signing_key_signs_and_verifies() {
        let secret = SecretSigningKey::ed25519([7; 32]);
        let payload = b"kairo payload";
        let signature = secret.sign(payload);
        assert_eq!(
            verify_signature(&secret.public_key(), payload, &signature),
            Ok(VerifiedSignature)
        );
    }

    #[test]
    fn secret_signing_key_debug_redacts_seed() {
        let secret = SecretSigningKey::ed25519([7; 32]);
        let rendered = format!("{secret:?}");
        assert!(rendered.contains("redacted"));
        assert!(!rendered.contains("7, 7, 7"));
    }

    #[test]
    fn generate_ed25519_returns_valid_key() {
        let secret = SecretSigningKey::generate_ed25519();
        assert!(matches!(
            secret,
            Ok(secret) if matches!(secret.algorithm(), SignatureAlgorithm::Ed25519)
        ));
    }

    #[test]
    fn memory_resolver_finds_actor_genesis_by_derived_actor_id() {
        let genesis =
            ActorGenesisBody::new(ActorKind::person(), public_key(), vec![attestation_key()], 1, timestamp(), [9; 32]).expect("genesis well-formed");
        let mut resolver = MemoryActorResolver::new();
        let actor_id = resolver.insert(genesis.clone());

        assert_eq!(resolver.actor_genesis(&actor_id), Ok(Some(genesis)));
    }

    #[test]
    fn memory_resolver_returns_none_for_missing_actor() {
        let resolver = MemoryActorResolver::new();
        let missing_actor =
            ActorGenesisBody::new(ActorKind::person(), public_key(), vec![attestation_key()], 1, timestamp(), [9; 32]).expect("genesis well-formed")
                .actor_id();

        assert_eq!(resolver.actor_genesis(&missing_actor), Ok(None));
    }

    #[test]
    fn memory_resolver_resolves_initial_key() {
        let genesis =
            ActorGenesisBody::new(ActorKind::person(), public_key(), vec![attestation_key()], 1, timestamp(), [9; 32]).expect("genesis well-formed");
        let mut resolver = MemoryActorResolver::new();
        let actor_id = resolver.insert(genesis.clone());

        assert_eq!(resolver.initial_key(&actor_id), Ok(Some(public_key())));
    }

    fn other_public_key() -> PublicKey {
        PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes())
    }

    fn third_public_key() -> PublicKey {
        PublicKey::ed25519(SigningKey::from_bytes(&[9; 32]).verifying_key().to_bytes())
    }

    #[test]
    fn active_key_falls_back_to_genesis_initial_when_no_rotations() {
        let genesis =
            ActorGenesisBody::new(ActorKind::person(), public_key(), vec![attestation_key()], 1, timestamp(), [9; 32]).expect("genesis well-formed");
        let mut resolver = MemoryActorResolver::new();
        let actor_id = resolver.insert(genesis);

        let resolved = resolver
            .active_key_at(&actor_id, Timestamp::from_seconds(timestamp().seconds() + 100))
            .expect("query succeeds");
        assert_eq!(resolved, Some(public_key()));
    }

    #[test]
    fn active_key_at_walks_rotation_chain() {
        let genesis =
            ActorGenesisBody::new(ActorKind::person(), public_key(), vec![attestation_key()], 1, timestamp(), [9; 32]).expect("genesis well-formed");
        let mut resolver = MemoryActorResolver::new();
        let actor_id = resolver.insert(genesis);

        // First rotation moves the active key to other_public_key
        // at t+10.
        resolver.insert_rotation(
            actor_id.clone(),
            KeyRotationEntry {
                statement_id: "rotation-1".to_owned(),
                next_key: other_public_key(),
                created_at: Timestamp::from_seconds(timestamp().seconds() + 10),
                supersedes: None,
                surface: KeySurface::Operational,
            },
        );
        // Successor rotation at t+20 names rotation-1 in supersedes
        // and rotates to third_public_key.
        resolver.insert_rotation(
            actor_id.clone(),
            KeyRotationEntry {
                statement_id: "rotation-2".to_owned(),
                next_key: third_public_key(),
                created_at: Timestamp::from_seconds(timestamp().seconds() + 20),
                supersedes: Some("rotation-1".to_owned()),
                surface: KeySurface::Operational,
            },
        );

        // Before any rotation: genesis-initial wins.
        assert_eq!(
            resolver.active_key_at(&actor_id, timestamp()).unwrap(),
            Some(public_key())
        );
        // Between the two rotations: rotation-1's next_key wins.
        assert_eq!(
            resolver
                .active_key_at(&actor_id, Timestamp::from_seconds(timestamp().seconds() + 15))
                .unwrap(),
            Some(other_public_key())
        );
        // After both rotations: rotation-2's next_key wins.
        assert_eq!(
            resolver
                .active_key_at(&actor_id, Timestamp::from_seconds(timestamp().seconds() + 25))
                .unwrap(),
            Some(third_public_key())
        );
    }

    #[test]
    fn revocation_default_only_invalidates_after_created_at() {
        let genesis =
            ActorGenesisBody::new(ActorKind::person(), public_key(), vec![attestation_key()], 1, timestamp(), [9; 32]).expect("genesis well-formed");
        let mut resolver = MemoryActorResolver::new();
        let actor_id = resolver.insert(genesis);
        let key_id = public_key().key_id();

        resolver.insert_revocation(
            actor_id.clone(),
            KeyRevocationEntry {
                statement_id: "revocation-1".to_owned(),
                revoked_key: key_id.clone(),
                retroactive: false,
                created_at: Timestamp::from_seconds(timestamp().seconds() + 100),
                surface: KeySurface::Operational,
            },
        );

        // Before the revocation: not revoked.
        assert!(!resolver
            .is_key_revoked_at(&actor_id, &key_id, timestamp())
            .unwrap());
        // After the revocation: revoked.
        assert!(resolver
            .is_key_revoked_at(
                &actor_id,
                &key_id,
                Timestamp::from_seconds(timestamp().seconds() + 200)
            )
            .unwrap());
    }

    #[test]
    fn retroactive_revocation_invalidates_at_every_timestamp() {
        let genesis =
            ActorGenesisBody::new(ActorKind::person(), public_key(), vec![attestation_key()], 1, timestamp(), [9; 32]).expect("genesis well-formed");
        let mut resolver = MemoryActorResolver::new();
        let actor_id = resolver.insert(genesis);
        let key_id = public_key().key_id();

        resolver.insert_revocation(
            actor_id.clone(),
            KeyRevocationEntry {
                statement_id: "revocation-1".to_owned(),
                revoked_key: key_id.clone(),
                retroactive: true,
                created_at: Timestamp::from_seconds(timestamp().seconds() + 100),
                surface: KeySurface::Operational,
            },
        );

        // Even before the revocation's created_at, the key is treated
        // as revoked because retroactive = true.
        assert!(resolver
            .is_key_revoked_at(&actor_id, &key_id, timestamp())
            .unwrap());
    }

    #[test]
    fn most_restrictive_revocation_wins() {
        let genesis =
            ActorGenesisBody::new(ActorKind::person(), public_key(), vec![attestation_key()], 1, timestamp(), [9; 32]).expect("genesis well-formed");
        let mut resolver = MemoryActorResolver::new();
        let actor_id = resolver.insert(genesis);
        let key_id = public_key().key_id();

        // First a non-retroactive revocation, then a retroactive one
        // for the same key. The retroactive one wins.
        resolver.insert_revocation(
            actor_id.clone(),
            KeyRevocationEntry {
                statement_id: "rev-default".to_owned(),
                revoked_key: key_id.clone(),
                retroactive: false,
                created_at: Timestamp::from_seconds(timestamp().seconds() + 100),
                surface: KeySurface::Operational,
            },
        );
        resolver.insert_revocation(
            actor_id.clone(),
            KeyRevocationEntry {
                statement_id: "rev-retro".to_owned(),
                revoked_key: key_id.clone(),
                retroactive: true,
                created_at: Timestamp::from_seconds(timestamp().seconds() + 200),
                surface: KeySurface::Operational,
            },
        );

        assert!(resolver
            .is_key_revoked_at(&actor_id, &key_id, timestamp())
            .unwrap());
    }

    #[test]
    fn attestation_revocation_removes_key_from_set_after_created_at() {
        // Genesis declares one attestation key, an add appends a second,
        // a revocation later removes the genesis key. Before the
        // revocation: both keys are in the set. At/after the
        // revocation: only the appended key remains. This is the
        // resolver-level expression of the §5.5.2 set difference rule.
        let genesis = ActorGenesisBody::new(
            ActorKind::person(),
            public_key(),
            vec![attestation_key()],
            1,
            timestamp(),
            [42; 32],
        )
        .expect("genesis well-formed");
        let mut resolver = MemoryActorResolver::new();
        let actor_id = resolver.insert(genesis);

        let appended = third_public_key();
        let appended_id = appended.key_id();
        let attestation_id = attestation_key().key_id();

        resolver.insert_attestation_add(
            actor_id.clone(),
            AttestationKeyAddEntry {
                statement_id: "add-1".to_owned(),
                new_key: appended,
                created_at: Timestamp::from_seconds(timestamp().seconds() + 100),
            },
        );
        resolver.insert_attestation_revocation(
            actor_id.clone(),
            AttestationKeyRevocationEntry {
                statement_id: "rev-1".to_owned(),
                revoked_key: attestation_id.clone(),
                created_at: Timestamp::from_seconds(timestamp().seconds() + 200),
            },
        );

        // Before any add or revocation: only the genesis key.
        let set_before = resolver
            .attestation_keys_at(&actor_id, timestamp())
            .unwrap();
        assert_eq!(set_before.len(), 1);
        assert!(set_before.contains_key(&attestation_id));

        // After add but before revocation: both keys present.
        let set_mid = resolver
            .attestation_keys_at(
                &actor_id,
                Timestamp::from_seconds(timestamp().seconds() + 150),
            )
            .unwrap();
        assert_eq!(set_mid.len(), 2);
        assert!(set_mid.contains_key(&attestation_id));
        assert!(set_mid.contains_key(&appended_id));

        // At/after revocation: only the appended key remains.
        let set_after = resolver
            .attestation_keys_at(
                &actor_id,
                Timestamp::from_seconds(timestamp().seconds() + 300),
            )
            .unwrap();
        assert_eq!(set_after.len(), 1);
        assert!(set_after.contains_key(&appended_id));
        assert!(!set_after.contains_key(&attestation_id));
    }
}
