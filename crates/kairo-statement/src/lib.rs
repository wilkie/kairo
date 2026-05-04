//! Signed statement envelope primitives.

pub mod json;
pub mod verify;

use std::error::Error;
use std::fmt;

use kairo_core::canonical::{
    encode_bytes, encode_i64, encode_list, encode_option, encode_str, encode_u8, CanonicalEncode,
};
use semver::Version as SemverVersionInner;
use kairo_core::{ActorId, BlobId, KairoRef, ObjectId, StatementId, Timestamp};
use kairo_identity::{
    verify_signature as verify_identity_signature, KeyId, PublicKey, SignatureBytes,
    SignatureVerificationError, VerifiedSignature,
};

/// Canonical ObjectGenesis body v1 encoding is documented at
/// `schemas/canonical/object-genesis-v1.md`.
const OBJECT_GENESIS_DOMAIN: &[u8] = b"kairo.object.genesis.v1";
const STATEMENT_DOMAIN: &[u8] = b"kairo.statement.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementEnvelope {
    id: StatementId,
    actor: ActorId,
    subject: KairoRef,
    created_at: Timestamp,
}

impl StatementEnvelope {
    pub fn new(id: StatementId, actor: ActorId, subject: KairoRef, created_at: Timestamp) -> Self {
        Self {
            id,
            actor,
            subject,
            created_at,
        }
    }

    pub fn id(&self) -> &StatementId {
        &self.id
    }

    pub fn actor(&self) -> &ActorId {
        &self.actor
    }

    pub fn subject(&self) -> &KairoRef {
        &self.subject
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

pub trait StatementBody: CanonicalEncode {
    const TYPE: &'static str;
    const VERSION: u8;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsignedStatement<B> {
    actor: ActorId,
    subject: KairoRef,
    created_at: Timestamp,
    body: B,
}

impl<B> UnsignedStatement<B> {
    pub fn new(actor: ActorId, subject: KairoRef, created_at: Timestamp, body: B) -> Self {
        Self {
            actor,
            subject,
            created_at,
            body,
        }
    }

    pub fn actor(&self) -> &ActorId {
        &self.actor
    }

    pub fn subject(&self) -> &KairoRef {
        &self.subject
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    pub fn body(&self) -> &B {
        &self.body
    }
}

impl<B: StatementBody> UnsignedStatement<B> {
    pub fn statement_id(&self) -> StatementId {
        StatementId::from_bytes(STATEMENT_DOMAIN, &self.canonical_bytes())
    }
}

impl<B: StatementBody> CanonicalEncode for UnsignedStatement<B> {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, B::TYPE);
        encode_u8(out, B::VERSION);
        encode_str(out, self.actor.as_str());
        encode_str(out, &self.subject.to_string());
        self.created_at.encode_canonical(out);
        self.body.encode_canonical(out);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedStatement<B> {
    unsigned: UnsignedStatement<B>,
    signature: Signature,
}

impl<B> SignedStatement<B> {
    pub fn new(unsigned: UnsignedStatement<B>, signature: Signature) -> Self {
        Self {
            unsigned,
            signature,
        }
    }

    pub fn unsigned(&self) -> &UnsignedStatement<B> {
        &self.unsigned
    }

    pub fn signature(&self) -> &Signature {
        &self.signature
    }
}

impl<B: StatementBody> SignedStatement<B> {
    pub fn statement_id(&self) -> StatementId {
        self.unsigned.statement_id()
    }

    pub fn signed_bytes(&self) -> Vec<u8> {
        self.unsigned.canonical_bytes()
    }

    pub fn verify_signature(
        &self,
        public_key: &PublicKey,
    ) -> Result<VerifiedSignature, StatementSignatureError> {
        let signature = self.signature.to_signature_bytes()?;
        verify_identity_signature(public_key, &self.signed_bytes(), &signature)
            .map_err(StatementSignatureError::Verification)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectGenesisBody {
    object_kind: ObjectKind,
    created_by: ActorId,
    created_at: Timestamp,
    nonce: [u8; 32],
    initial_revision: Option<RevisionId>,
}

impl ObjectGenesisBody {
    pub fn new(
        object_kind: ObjectKind,
        created_by: ActorId,
        created_at: Timestamp,
        nonce: [u8; 32],
        initial_revision: Option<RevisionId>,
    ) -> Self {
        Self {
            object_kind,
            created_by,
            created_at,
            nonce,
            initial_revision,
        }
    }

    pub fn object_kind(&self) -> &ObjectKind {
        &self.object_kind
    }

    pub fn created_by(&self) -> &ActorId {
        &self.created_by
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }

    pub fn initial_revision(&self) -> Option<&RevisionId> {
        self.initial_revision.as_ref()
    }

    pub fn object_id(&self) -> ObjectId {
        ObjectId::from_bytes(OBJECT_GENESIS_DOMAIN, &self.canonical_bytes())
    }
}

impl CanonicalEncode for ObjectGenesisBody {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_bytes(out, b"ObjectGenesis");
        encode_u8(out, 1);
        encode_str(out, self.object_kind.as_str());
        encode_str(out, self.created_by.as_str());
        self.created_at.encode_canonical(out);
        encode_bytes(out, &self.nonce);
        encode_option(out, self.initial_revision.as_ref(), |out, revision| {
            encode_str(out, revision.as_str());
        });
    }
}

/// Canonical ObjectRevision body v1 encoding is documented at
/// `schemas/canonical/object-revision-v1.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectRevisionBody {
    object: ObjectId,
    revision: RevisionId,
    parents: Vec<RevisionId>,
    manifest_hash: BlobId,
    attests_reachable_history: bool,
}

impl ObjectRevisionBody {
    pub fn new(
        object: ObjectId,
        revision: RevisionId,
        parents: Vec<RevisionId>,
        manifest_hash: BlobId,
        attests_reachable_history: bool,
    ) -> Self {
        Self {
            object,
            revision,
            parents,
            manifest_hash,
            attests_reachable_history,
        }
    }

    pub fn object(&self) -> &ObjectId {
        &self.object
    }

    pub fn revision(&self) -> &RevisionId {
        &self.revision
    }

    pub fn parents(&self) -> &[RevisionId] {
        &self.parents
    }

    pub fn manifest_hash(&self) -> &BlobId {
        &self.manifest_hash
    }

    pub fn attests_reachable_history(&self) -> bool {
        self.attests_reachable_history
    }
}

impl StatementBody for ObjectRevisionBody {
    const TYPE: &'static str = "ObjectRevision";
    const VERSION: u8 = 1;
}

impl CanonicalEncode for ObjectRevisionBody {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, self.object.as_str());
        encode_str(out, self.revision.as_str());
        encode_list(out, &self.parents, |out, revision| {
            encode_str(out, revision.as_str());
        });
        encode_str(out, self.manifest_hash.as_str());
        encode_u8(out, u8::from(self.attests_reachable_history));
    }
}

/// Canonical ObjectBranch body v1 encoding is documented at
/// `schemas/canonical/object-branch-v1.md`.
///
/// An `ObjectBranch` is a named, actor-scoped, mutable pointer at a specific
/// `ObjectRevision` statement. Resolution: for a given
/// `(actor, object, name)`, the current branch is whichever `ObjectBranch`
/// statement signed by that actor for that pair has the greatest
/// `(envelope.created_at, statement_id)`. Older `ObjectBranch` statements
/// stay valid evidence of past claims; only the latest is load-bearing for
/// resolution.
///
/// `name` is free-form. The string `"head"` is the conventional default
/// across the CLI but is not reserved at the protocol level — actors can
/// publish branches with any name (`"release"`, `"audit"`, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectBranchBody {
    object: ObjectId,
    name: String,
    revision: StatementId,
}

impl ObjectBranchBody {
    pub fn new(object: ObjectId, name: impl Into<String>, revision: StatementId) -> Self {
        Self {
            object,
            name: name.into(),
            revision,
        }
    }

    pub fn object(&self) -> &ObjectId {
        &self.object
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn revision(&self) -> &StatementId {
        &self.revision
    }
}

impl StatementBody for ObjectBranchBody {
    const TYPE: &'static str = "ObjectBranch";
    const VERSION: u8 = 1;
}

impl CanonicalEncode for ObjectBranchBody {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, self.object.as_str());
        encode_str(out, &self.name);
        encode_str(out, self.revision.as_str());
    }
}

/// A strict semver 2.0.0 version string, normalized to its canonical
/// `Display` form so that `01.2.3` and similar non-canonical inputs are
/// rejected rather than silently encoded into statement bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemverVersion(String);

impl SemverVersion {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, SemverParseError> {
        let input = value.as_ref();
        let parsed = SemverVersionInner::parse(input).map_err(|error| SemverParseError {
            input: input.to_owned(),
            message: error.to_string(),
        })?;
        Ok(Self(parsed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemverParseError {
    pub input: String,
    pub message: String,
}

impl fmt::Display for SemverParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid semver version {:?}: {}", self.input, self.message)
    }
}

impl Error for SemverParseError {}

/// Canonical ObjectVersionTag body v1 encoding is documented at
/// `schemas/canonical/object-version-tag-v1.md`.
///
/// An `ObjectVersionTag` binds a strict semver string to a specific
/// `ObjectRevision` statement (`target = Some`) or revokes a previously
/// published binding (`target = None`). It is actor-scoped and resolves
/// latest-wins on `(actor, object, version)`, identical to
/// `ObjectBranch`.
///
/// Every non-genesis tag carries an explicit `supersedes` pointer at the
/// prior tag in its chain so the rebind / revoke history is
/// reconstructable without inferring from timestamp order. The genesis
/// tag for `(actor, object, version)` has `supersedes = None` and must
/// be a bind (`target = Some`); a revoke with no chain reference is a
/// shape violation.
///
/// The store (not this body) enforces that `supersedes`, when present,
/// resolves to an existing `ObjectVersionTag` for the same `(actor,
/// object, version)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectVersionTagBody {
    object: ObjectId,
    version: SemverVersion,
    target: Option<StatementId>,
    supersedes: Option<StatementId>,
}

impl ObjectVersionTagBody {
    pub fn new(
        object: ObjectId,
        version: SemverVersion,
        target: Option<StatementId>,
        supersedes: Option<StatementId>,
    ) -> Result<Self, ObjectVersionTagShapeError> {
        if target.is_none() && supersedes.is_none() {
            return Err(ObjectVersionTagShapeError::RevokeWithoutSupersedes);
        }
        Ok(Self {
            object,
            version,
            target,
            supersedes,
        })
    }

    pub fn object(&self) -> &ObjectId {
        &self.object
    }

    pub fn version(&self) -> &SemverVersion {
        &self.version
    }

    pub fn target(&self) -> Option<&StatementId> {
        self.target.as_ref()
    }

    pub fn supersedes(&self) -> Option<&StatementId> {
        self.supersedes.as_ref()
    }

    pub fn is_revocation(&self) -> bool {
        self.target.is_none()
    }

    pub fn is_genesis(&self) -> bool {
        self.supersedes.is_none()
    }
}

impl StatementBody for ObjectVersionTagBody {
    const TYPE: &'static str = "ObjectVersionTag";
    const VERSION: u8 = 1;
}

impl CanonicalEncode for ObjectVersionTagBody {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, self.object.as_str());
        encode_str(out, self.version.as_str());
        encode_option(out, self.target.as_ref(), |out, statement_id| {
            encode_str(out, statement_id.as_str());
        });
        encode_option(out, self.supersedes.as_ref(), |out, statement_id| {
            encode_str(out, statement_id.as_str());
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectVersionTagShapeError {
    RevokeWithoutSupersedes,
}

impl fmt::Display for ObjectVersionTagShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RevokeWithoutSupersedes => f.write_str(
                "ObjectVersionTag with target=null must reference a prior tag via supersedes",
            ),
        }
    }
}

impl Error for ObjectVersionTagShapeError {}

/// A trust decision for an `ActorTrust` statement. Withdrawal is
/// represented at the body level by `decision: None`, not as a third
/// variant here, so that this enum stays aligned with the canonical
/// "trusted" / "untrusted" string encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustDecision {
    Trusted,
    Untrusted,
}

impl TrustDecision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::Untrusted => "untrusted",
        }
    }

    /// Parse a wire-format string. `"trusted"` and `"untrusted"` only;
    /// everything else (including `null`/None at the JSON layer, which
    /// is handled separately as withdrawal) is invalid.
    pub fn parse(value: &str) -> Result<Self, TrustDecisionParseError> {
        match value {
            "trusted" => Ok(Self::Trusted),
            "untrusted" => Ok(Self::Untrusted),
            other => Err(TrustDecisionParseError {
                input: other.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustDecisionParseError {
    pub input: String,
}

impl fmt::Display for TrustDecisionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid TrustDecision {:?}: expected \"trusted\" or \"untrusted\"",
            self.input
        )
    }
}

impl Error for TrustDecisionParseError {}

/// Canonical ActorTrust body v1 encoding is documented at
/// `schemas/canonical/actor-trust-v1.md`.
///
/// `ActorTrust` records a local actor's first-person opinion about
/// another actor's identity claims: trusted, untrusted, or withdrawn
/// (`decision = None`). Resolution is per-truster — `(by_actor,
/// trusted_actor)` is the lookup key. Cross-actor `supersedes` is
/// invalid for trust (tighter than `ObjectVersionTag`); the resolver
/// in `kairo-store` enforces this at lookup time.
///
/// Genesis: `decision` is `Some(_)`, `supersedes` is `None`.
/// Successor: `supersedes` is `Some(_)`; `decision` may be any of
/// `Some(Trusted)`, `Some(Untrusted)`, or `None` (withdrawal). The
/// shape `decision = None && supersedes = None` is invalid — you
/// can't withdraw nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorTrustBody {
    trusted_actor: ActorId,
    decision: Option<TrustDecision>,
    reason: Option<String>,
    supersedes: Option<StatementId>,
}

impl ActorTrustBody {
    pub fn new(
        trusted_actor: ActorId,
        decision: Option<TrustDecision>,
        reason: Option<String>,
        supersedes: Option<StatementId>,
    ) -> Result<Self, ActorTrustShapeError> {
        if decision.is_none() && supersedes.is_none() {
            return Err(ActorTrustShapeError::WithdrawWithoutSupersedes);
        }
        Ok(Self {
            trusted_actor,
            decision,
            reason,
            supersedes,
        })
    }

    pub fn trusted_actor(&self) -> &ActorId {
        &self.trusted_actor
    }

    pub fn decision(&self) -> Option<TrustDecision> {
        self.decision
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    pub fn supersedes(&self) -> Option<&StatementId> {
        self.supersedes.as_ref()
    }

    pub fn is_withdrawal(&self) -> bool {
        self.decision.is_none()
    }

    pub fn is_genesis(&self) -> bool {
        self.supersedes.is_none()
    }
}

impl StatementBody for ActorTrustBody {
    const TYPE: &'static str = "ActorTrust";
    const VERSION: u8 = 1;
}

impl CanonicalEncode for ActorTrustBody {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, self.trusted_actor.as_str());
        encode_option(out, self.decision.as_ref(), |out, decision| {
            encode_str(out, decision.as_str());
        });
        encode_option(out, self.reason.as_ref(), |out, reason| {
            encode_str(out, reason);
        });
        encode_option(out, self.supersedes.as_ref(), |out, statement_id| {
            encode_str(out, statement_id.as_str());
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorTrustShapeError {
    WithdrawWithoutSupersedes,
}

impl fmt::Display for ActorTrustShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WithdrawWithoutSupersedes => f.write_str(
                "ActorTrust with decision=null must reference a prior statement via supersedes",
            ),
        }
    }
}

impl Error for ActorTrustShapeError {}

/// A canonical statement-kind discriminant, used to enumerate the kinds
/// authorized by an `ActorCapabilityGrant` and to drive
/// per-`(scope, kind)` shape validity. The `as_str` mapping matches
/// each statement type's canonical `TYPE` constant.
///
/// `Ord` is derived so that `Capability::statement_kinds` can be sorted
/// for canonical-byte determinism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StatementKind {
    ObjectGenesis,
    ObjectRevision,
    ObjectBranch,
    ObjectVersionTag,
    ActorTrust,
    ActorCapabilityGrant,
    ActorCapabilityRevocation,
}

impl StatementKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ObjectGenesis => "ObjectGenesis",
            Self::ObjectRevision => "ObjectRevision",
            Self::ObjectBranch => "ObjectBranch",
            Self::ObjectVersionTag => "ObjectVersionTag",
            Self::ActorTrust => "ActorTrust",
            Self::ActorCapabilityGrant => "ActorCapabilityGrant",
            Self::ActorCapabilityRevocation => "ActorCapabilityRevocation",
        }
    }

    pub fn parse(value: &str) -> Result<Self, StatementKindParseError> {
        match value {
            "ObjectGenesis" => Ok(Self::ObjectGenesis),
            "ObjectRevision" => Ok(Self::ObjectRevision),
            "ObjectBranch" => Ok(Self::ObjectBranch),
            "ObjectVersionTag" => Ok(Self::ObjectVersionTag),
            "ActorTrust" => Ok(Self::ActorTrust),
            "ActorCapabilityGrant" => Ok(Self::ActorCapabilityGrant),
            "ActorCapabilityRevocation" => Ok(Self::ActorCapabilityRevocation),
            other => Err(StatementKindParseError {
                input: other.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementKindParseError {
    pub input: String,
}

impl fmt::Display for StatementKindParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown StatementKind {:?}", self.input)
    }
}

impl Error for StatementKindParseError {}

/// The target of an `ActorCapabilityGrant`. Today there are two
/// variants — object-scoped and actor-scoped (the grantor's own actor
/// surface). Per `specs/CAPABILITIES.md` Decision E, kind narrowing
/// lives in `Capability::statement_kinds`, not in scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityScope {
    Object(ObjectId),
    Actor(ActorId),
}

impl CapabilityScope {
    /// Tag byte used in canonical encoding and for shape diagnostics.
    pub fn tag(&self) -> u8 {
        match self {
            Self::Object(_) => 0x00,
            Self::Actor(_) => 0x01,
        }
    }

    /// Whether `kind` is a legal statement kind to delegate under this
    /// scope. The MVP table is conservative; future actor-surface
    /// statement kinds will join `Actor` here.
    pub fn is_kind_valid(&self, kind: StatementKind) -> bool {
        match self {
            Self::Object(_) => matches!(
                kind,
                StatementKind::ObjectRevision
                    | StatementKind::ObjectBranch
                    | StatementKind::ObjectVersionTag
            ),
            // No actor-surface statement type is delegatable in v1.
            // Reserved for future kinds (ActorMetadata, etc.).
            Self::Actor(_) => false,
        }
    }
}

impl CanonicalEncode for CapabilityScope {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_u8(out, self.tag());
        match self {
            Self::Object(id) => encode_str(out, id.as_str()),
            Self::Actor(id) => encode_str(out, id.as_str()),
        }
    }
}

/// Constraints layered onto a capability. Each variant has at most one
/// occurrence in `Capability::constraints` (validated at construction).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityConstraint {
    /// Grant is invalid for statements created strictly after this
    /// timestamp.
    ExpiresAt(Timestamp),
    /// Maximum re-grant chain depth.
    MaxDelegationDepth(u8),
    /// Grant is bound to a specific grantor signing key; revocation of
    /// that key auto-invalidates the grant. See `specs/CAPABILITIES.md`
    /// §7.2.
    KeyPinned(KeyId),
}

impl CapabilityConstraint {
    /// Tag byte used for canonical encoding and for sort/dedup ordering.
    pub fn tag(&self) -> u8 {
        match self {
            Self::ExpiresAt(_) => 0x00,
            Self::MaxDelegationDepth(_) => 0x01,
            Self::KeyPinned(_) => 0x02,
        }
    }
}

impl CanonicalEncode for CapabilityConstraint {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_u8(out, self.tag());
        match self {
            Self::ExpiresAt(ts) => encode_i64(out, ts.seconds()),
            Self::MaxDelegationDepth(depth) => encode_u8(out, *depth),
            Self::KeyPinned(key_id) => encode_str(out, key_id.as_str()),
        }
    }
}

/// A capability granted from one actor to another. See
/// `specs/CAPABILITIES.md` §4 and
/// `schemas/canonical/actor-capability-grant-v1.md`.
///
/// Constructor enforces:
/// - `statement_kinds` is non-empty.
/// - `statement_kinds` is sorted and deduplicated (constructor
///   normalizes).
/// - Each kind in `statement_kinds` is valid for `scope` per the
///   per-`(scope, kind)` shape table.
/// - `constraints` has at most one of each variant; constructor sorts
///   by tag byte for canonical determinism.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    scope: CapabilityScope,
    statement_kinds: Vec<StatementKind>,
    delegable: bool,
    constraints: Vec<CapabilityConstraint>,
}

impl Capability {
    pub fn new(
        scope: CapabilityScope,
        mut statement_kinds: Vec<StatementKind>,
        delegable: bool,
        mut constraints: Vec<CapabilityConstraint>,
    ) -> Result<Self, CapabilityShapeError> {
        if statement_kinds.is_empty() {
            return Err(CapabilityShapeError::EmptyStatementKinds);
        }

        statement_kinds.sort();
        statement_kinds.dedup();

        for kind in &statement_kinds {
            if !scope.is_kind_valid(*kind) {
                return Err(CapabilityShapeError::KindInvalidForScope {
                    scope_tag: scope.tag(),
                    kind: *kind,
                });
            }
        }

        constraints.sort_by_key(|c| c.tag());
        let mut prev_tag: Option<u8> = None;
        for constraint in &constraints {
            if Some(constraint.tag()) == prev_tag {
                return Err(CapabilityShapeError::DuplicateConstraintTag {
                    tag: constraint.tag(),
                });
            }
            prev_tag = Some(constraint.tag());
        }

        Ok(Self {
            scope,
            statement_kinds,
            delegable,
            constraints,
        })
    }

    pub fn scope(&self) -> &CapabilityScope {
        &self.scope
    }

    pub fn statement_kinds(&self) -> &[StatementKind] {
        &self.statement_kinds
    }

    pub fn delegable(&self) -> bool {
        self.delegable
    }

    pub fn constraints(&self) -> &[CapabilityConstraint] {
        &self.constraints
    }
}

impl CanonicalEncode for Capability {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.scope.encode_canonical(out);
        encode_list(out, &self.statement_kinds, |out, kind| {
            encode_str(out, kind.as_str());
        });
        encode_u8(out, if self.delegable { 1 } else { 0 });
        encode_list(out, &self.constraints, |out, constraint| {
            constraint.encode_canonical(out);
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityShapeError {
    EmptyStatementKinds,
    KindInvalidForScope {
        scope_tag: u8,
        kind: StatementKind,
    },
    DuplicateConstraintTag {
        tag: u8,
    },
}

impl fmt::Display for CapabilityShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyStatementKinds => f.write_str(
                "Capability::statement_kinds must be non-empty",
            ),
            Self::KindInvalidForScope { scope_tag, kind } => write!(
                f,
                "StatementKind {:?} is not valid for scope tag 0x{scope_tag:02x}",
                kind.as_str()
            ),
            Self::DuplicateConstraintTag { tag } => write!(
                f,
                "Capability::constraints contains duplicate variant (tag 0x{tag:02x})"
            ),
        }
    }
}

impl Error for CapabilityShapeError {}

/// Canonical ActorCapabilityGrant body v1 encoding is documented at
/// `schemas/canonical/actor-capability-grant-v1.md`.
///
/// `ActorCapabilityGrant` is a signed delegation: the grantor (the
/// signer of the envelope) authorizes the named `grantee` to issue a
/// specified set of statement kinds against a scoped target,
/// optionally bounded by constraints.
///
/// Resolution is per-`(grantor, grantee, scope)` triple, with chain
/// precedence: the leaf of the supersedes chain is the source of
/// truth for the grantee's current authority on this scope from this
/// grantor. See `specs/CAPABILITIES.md` §5.1.1 and §6.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorCapabilityGrantBody {
    grantee: ActorId,
    capability: Capability,
    supersedes: Option<StatementId>,
}

impl ActorCapabilityGrantBody {
    pub fn new(
        grantee: ActorId,
        capability: Capability,
        supersedes: Option<StatementId>,
    ) -> Self {
        Self {
            grantee,
            capability,
            supersedes,
        }
    }

    pub fn grantee(&self) -> &ActorId {
        &self.grantee
    }

    pub fn capability(&self) -> &Capability {
        &self.capability
    }

    pub fn supersedes(&self) -> Option<&StatementId> {
        self.supersedes.as_ref()
    }

    pub fn is_genesis(&self) -> bool {
        self.supersedes.is_none()
    }
}

impl StatementBody for ActorCapabilityGrantBody {
    const TYPE: &'static str = "ActorCapabilityGrant";
    const VERSION: u8 = 1;
}

impl CanonicalEncode for ActorCapabilityGrantBody {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, self.grantee.as_str());
        self.capability.encode_canonical(out);
        encode_option(out, self.supersedes.as_ref(), |out, statement_id| {
            encode_str(out, statement_id.as_str());
        });
    }
}

/// Canonical ActorCapabilityRevocation body v1 encoding is documented at
/// `schemas/canonical/actor-capability-revocation-v1.md`.
///
/// `ActorCapabilityRevocation` retracts a previously issued
/// `ActorCapabilityGrant`. The signer of the envelope must equal the
/// original grantor (cross-grantor revocation is invalid in v1; see
/// `specs/CAPABILITIES.md` §5.2). Default revocation invalidates the
/// grant for statements created strictly after the revocation;
/// `retroactive = true` invalidates the grant from inception.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorCapabilityRevocationBody {
    revoked_grant: StatementId,
    retroactive: bool,
    reason: Option<String>,
}

impl ActorCapabilityRevocationBody {
    pub fn new(
        revoked_grant: StatementId,
        retroactive: bool,
        reason: Option<String>,
    ) -> Self {
        Self {
            revoked_grant,
            retroactive,
            reason,
        }
    }

    pub fn revoked_grant(&self) -> &StatementId {
        &self.revoked_grant
    }

    pub fn retroactive(&self) -> bool {
        self.retroactive
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

impl StatementBody for ActorCapabilityRevocationBody {
    const TYPE: &'static str = "ActorCapabilityRevocation";
    const VERSION: u8 = 1;
}

impl CanonicalEncode for ActorCapabilityRevocationBody {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, self.revoked_grant.as_str());
        encode_u8(out, if self.retroactive { 1 } else { 0 });
        encode_option(out, self.reason.as_ref(), |out, reason| {
            encode_str(out, reason);
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectGenesisStatement {
    body: ObjectGenesisBody,
    signature: Signature,
}

impl ObjectGenesisStatement {
    pub fn new(body: ObjectGenesisBody, signature: Signature) -> Self {
        Self { body, signature }
    }

    pub fn body(&self) -> &ObjectGenesisBody {
        &self.body
    }

    pub fn signature(&self) -> &Signature {
        &self.signature
    }

    pub fn object_id(&self) -> ObjectId {
        self.body.object_id()
    }

    pub fn signed_bytes(&self) -> Vec<u8> {
        self.body.canonical_bytes()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    actor: ActorId,
    key_id: String,
    algorithm: String,
    bytes: Vec<u8>,
}

impl Signature {
    pub fn new(
        actor: ActorId,
        key_id: impl Into<String>,
        algorithm: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            actor,
            key_id: key_id.into(),
            algorithm: algorithm.into(),
            bytes,
        }
    }

    pub fn actor(&self) -> &ActorId {
        &self.actor
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn to_signature_bytes(&self) -> Result<SignatureBytes, StatementSignatureError> {
        match self.algorithm.as_str() {
            "ed25519" => {
                let bytes = <[u8; 64]>::try_from(self.bytes.as_slice()).map_err(|_| {
                    StatementSignatureError::InvalidSignatureLength {
                        expected: 64,
                        actual: self.bytes.len(),
                    }
                })?;
                Ok(SignatureBytes::ed25519(bytes))
            }
            algorithm => Err(StatementSignatureError::UnsupportedAlgorithm(
                algorithm.to_owned(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementSignatureError {
    UnsupportedAlgorithm(String),
    InvalidSignatureLength { expected: usize, actual: usize },
    Verification(SignatureVerificationError),
}

impl fmt::Display for StatementSignatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedAlgorithm(algorithm) => {
                write!(f, "unsupported signature algorithm {algorithm}")
            }
            Self::InvalidSignatureLength { expected, actual } => {
                write!(f, "invalid signature length {actual}; expected {expected}")
            }
            Self::Verification(error) => write!(f, "{error}"),
        }
    }
}

impl Error for StatementSignatureError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Verification(error) => Some(error),
            Self::UnsupportedAlgorithm(_) | Self::InvalidSignatureLength { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectKind(String);

impl ObjectKind {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn software() -> Self {
        Self("software".to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionId(String);

impl RevisionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use kairo_core::canonical::encode_str;
    use kairo_identity::PublicKey;

    const ACTOR_ID: &str = "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t";
    const OBJECT_ID: &str = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";
    const BLOB_ID: &str = "zQmfBE2w2UqKJhGZAxK4ZWb4JuCrvxnN9P4YKuvzfbPSvD5";

    fn actor_id() -> Result<ActorId, kairo_core::IdError> {
        ActorId::new(ACTOR_ID)
    }

    fn timestamp() -> Timestamp {
        Timestamp::from_seconds(1_700_000_000)
    }

    fn genesis_with_nonce(nonce: [u8; 32]) -> Result<ObjectGenesisBody, kairo_core::IdError> {
        Ok(ObjectGenesisBody::new(
            ObjectKind::software(),
            actor_id()?,
            timestamp(),
            nonce,
            None,
        ))
    }

    fn signature(key_id: &str, bytes: Vec<u8>) -> Result<Signature, kairo_core::IdError> {
        Ok(Signature::new(actor_id()?, key_id, "test", bytes))
    }

    fn object_ref() -> Result<KairoRef, kairo_core::IdError> {
        format!("object:{OBJECT_ID}").parse()
    }

    fn object_id() -> Result<ObjectId, kairo_core::IdError> {
        ObjectId::new(OBJECT_ID)
    }

    fn blob_id() -> Result<BlobId, kairo_core::IdError> {
        BlobId::new(BLOB_ID)
    }

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn public_key() -> PublicKey {
        PublicKey::ed25519(signing_key().verifying_key().to_bytes())
    }

    fn other_public_key() -> PublicKey {
        PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes())
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestBody {
        value: String,
    }

    impl CanonicalEncode for TestBody {
        fn encode_canonical(&self, out: &mut Vec<u8>) {
            encode_str(out, &self.value);
        }
    }

    impl StatementBody for TestBody {
        const TYPE: &'static str = "TestBody";
        const VERSION: u8 = 1;
    }

    fn unsigned_test_statement(
        value: &str,
    ) -> Result<UnsignedStatement<TestBody>, kairo_core::IdError> {
        Ok(UnsignedStatement::new(
            actor_id()?,
            object_ref()?,
            timestamp(),
            TestBody {
                value: value.to_owned(),
            },
        ))
    }

    fn object_revision_body(
        parents: Vec<RevisionId>,
        manifest_hash: BlobId,
    ) -> Result<ObjectRevisionBody, kairo_core::IdError> {
        Ok(ObjectRevisionBody::new(
            object_id()?,
            RevisionId::new("git:sha256:revision"),
            parents,
            manifest_hash,
            true,
        ))
    }

    fn unsigned_object_revision(
        body: ObjectRevisionBody,
    ) -> Result<UnsignedStatement<ObjectRevisionBody>, kairo_core::IdError> {
        Ok(UnsignedStatement::new(
            actor_id()?,
            object_ref()?,
            timestamp(),
            body,
        ))
    }

    #[test]
    fn same_genesis_produces_same_object_id() {
        let first = genesis_with_nonce([7; 32]);
        let second = genesis_with_nonce([7; 32]);

        assert_eq!(
            first.map(|genesis| (genesis.canonical_bytes(), genesis.object_id())),
            second.map(|genesis| (genesis.canonical_bytes(), genesis.object_id()))
        );
    }

    #[test]
    fn different_nonce_produces_different_object_id() {
        let first = genesis_with_nonce([7; 32]).map(|genesis| genesis.object_id());
        let second = genesis_with_nonce([8; 32]).map(|genesis| genesis.object_id());

        assert!(matches!((first, second), (Ok(first), Ok(second)) if first != second));
    }

    #[test]
    fn initial_revision_changes_object_id() {
        let without_revision = genesis_with_nonce([7; 32]).map(|genesis| genesis.object_id());
        let with_revision = actor_id().map(|actor_id| {
            ObjectGenesisBody::new(
                ObjectKind::software(),
                actor_id,
                timestamp(),
                [7; 32],
                Some(RevisionId::new("git:sha256:abc123")),
            )
            .object_id()
        });

        assert!(
            matches!((without_revision, with_revision), (Ok(without_revision), Ok(with_revision)) if without_revision != with_revision)
        );
    }

    #[test]
    fn object_genesis_created_at_changes_object_id() {
        let first = genesis_with_nonce([7; 32]).map(|genesis| genesis.object_id());
        let second = actor_id().map(|actor_id| {
            ObjectGenesisBody::new(
                ObjectKind::software(),
                actor_id,
                Timestamp::from_seconds(timestamp().seconds() + 1),
                [7; 32],
                None,
            )
            .object_id()
        });

        assert!(matches!((first, second), (Ok(first), Ok(second)) if first != second));
    }

    #[test]
    fn object_revision_created_at_changes_statement_id() -> Result<(), kairo_core::IdError> {
        let body = || {
            object_revision_body(
                vec![RevisionId::new("git:sha256:parent")],
                BlobId::from_sha256_digest([1; 32]),
            )
        };
        let first = body()
            .and_then(unsigned_object_revision)
            .map(|s| s.statement_id());
        let second_body = body()?;
        let second = UnsignedStatement::new(
            actor_id()?,
            object_ref()?,
            Timestamp::from_seconds(timestamp().seconds() + 1),
            second_body,
        )
        .statement_id();

        assert!(matches!(first, Ok(first) if first != second));
        Ok(())
    }

    #[test]
    fn generated_object_id_is_valid() {
        let object_id = genesis_with_nonce([7; 32]).map(|genesis| genesis.object_id());

        assert!(
            matches!(object_id, Ok(object_id) if ObjectId::new(object_id.to_string()) == Ok(object_id.clone()))
        );
    }

    #[test]
    fn signature_does_not_change_object_id() {
        let body = genesis_with_nonce([7; 32]);
        let first = body
            .clone()
            .and_then(|body| signature("key-1", vec![1, 2, 3]).map(|signature| (body, signature)))
            .map(|(body, signature)| ObjectGenesisStatement::new(body, signature).object_id());
        let second = body
            .and_then(|body| signature("key-2", vec![4, 5, 6]).map(|signature| (body, signature)))
            .map(|(body, signature)| ObjectGenesisStatement::new(body, signature).object_id());

        assert_eq!(first, second);
    }

    #[test]
    fn same_unsigned_statement_produces_same_statement_id() {
        let first = unsigned_test_statement("same").map(|statement| statement.statement_id());
        let second = unsigned_test_statement("same").map(|statement| statement.statement_id());

        assert_eq!(first, second);
    }

    #[test]
    fn different_body_produces_different_statement_id() {
        let first = unsigned_test_statement("first").map(|statement| statement.statement_id());
        let second = unsigned_test_statement("second").map(|statement| statement.statement_id());

        assert!(matches!((first, second), (Ok(first), Ok(second)) if first != second));
    }

    #[test]
    fn signature_does_not_change_statement_id() {
        let unsigned = unsigned_test_statement("same");
        let first = unsigned
            .clone()
            .and_then(|unsigned| {
                signature("key-1", vec![1, 2, 3]).map(|signature| (unsigned, signature))
            })
            .map(|(unsigned, signature)| SignedStatement::new(unsigned, signature).statement_id());
        let second = unsigned
            .and_then(|unsigned| {
                signature("key-2", vec![4, 5, 6]).map(|signature| (unsigned, signature))
            })
            .map(|(unsigned, signature)| SignedStatement::new(unsigned, signature).statement_id());

        assert_eq!(first, second);
    }

    #[test]
    fn same_object_revision_produces_same_statement_id() -> Result<(), kairo_core::IdError> {
        let first = object_revision_body(vec![RevisionId::new("git:sha256:parent")], blob_id()?)
            .and_then(unsigned_object_revision)
            .map(|statement| statement.statement_id());
        let second = object_revision_body(vec![RevisionId::new("git:sha256:parent")], blob_id()?)
            .and_then(unsigned_object_revision)
            .map(|statement| statement.statement_id());

        assert_eq!(first, second);
        Ok(())
    }

    #[test]
    fn object_revision_parent_order_affects_statement_id() -> Result<(), kairo_core::IdError> {
        let first = object_revision_body(
            vec![
                RevisionId::new("git:sha256:first-parent"),
                RevisionId::new("git:sha256:second-parent"),
            ],
            blob_id()?,
        )
        .and_then(unsigned_object_revision)
        .map(|statement| statement.statement_id());
        let second = object_revision_body(
            vec![
                RevisionId::new("git:sha256:second-parent"),
                RevisionId::new("git:sha256:first-parent"),
            ],
            blob_id()?,
        )
        .and_then(unsigned_object_revision)
        .map(|statement| statement.statement_id());

        assert!(matches!((first, second), (Ok(first), Ok(second)) if first != second));
        Ok(())
    }

    #[test]
    fn object_revision_manifest_hash_affects_statement_id() {
        let first = object_revision_body(
            vec![RevisionId::new("git:sha256:parent")],
            BlobId::from_sha256_digest([1; 32]),
        )
        .and_then(unsigned_object_revision)
        .map(|statement| statement.statement_id());
        let second = object_revision_body(
            vec![RevisionId::new("git:sha256:parent")],
            BlobId::from_sha256_digest([2; 32]),
        )
        .and_then(unsigned_object_revision)
        .map(|statement| statement.statement_id());

        assert!(matches!((first, second), (Ok(first), Ok(second)) if first != second));
    }

    #[test]
    fn object_revision_signature_does_not_change_statement_id() -> Result<(), kairo_core::IdError> {
        let unsigned = object_revision_body(vec![RevisionId::new("git:sha256:parent")], blob_id()?)
            .and_then(unsigned_object_revision);
        let first = unsigned
            .clone()
            .and_then(|unsigned| {
                signature("key-1", vec![1, 2, 3]).map(|signature| (unsigned, signature))
            })
            .map(|(unsigned, signature)| SignedStatement::new(unsigned, signature).statement_id());
        let second = unsigned
            .and_then(|unsigned| {
                signature("key-2", vec![4, 5, 6]).map(|signature| (unsigned, signature))
            })
            .map(|(unsigned, signature)| SignedStatement::new(unsigned, signature).statement_id());

        assert_eq!(first, second);
        Ok(())
    }

    #[test]
    fn verifies_object_revision_ed25519_signature() -> Result<(), Box<dyn std::error::Error>> {
        let unsigned = unsigned_object_revision(object_revision_body(
            vec![RevisionId::new("git:sha256:parent")],
            blob_id()?,
        )?)?;
        let signature = ed25519_signature(&unsigned)?;
        let signed = SignedStatement::new(unsigned, signature);

        assert_eq!(
            signed.verify_signature(&public_key()),
            Ok(VerifiedSignature)
        );
        Ok(())
    }

    #[test]
    fn rejects_object_revision_signature_after_body_change(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let signed_unsigned = unsigned_object_revision(object_revision_body(
            vec![RevisionId::new("git:sha256:parent")],
            blob_id()?,
        )?)?;
        let changed_unsigned = unsigned_object_revision(object_revision_body(
            vec![RevisionId::new("git:sha256:different-parent")],
            blob_id()?,
        )?)?;
        let signature = ed25519_signature(&signed_unsigned)?;
        let signed = SignedStatement::new(changed_unsigned, signature);

        assert!(matches!(
            signed.verify_signature(&public_key()),
            Err(StatementSignatureError::Verification(
                SignatureVerificationError::InvalidSignature
            ))
        ));
        Ok(())
    }

    #[test]
    fn rejects_changed_object_revision_signature() -> Result<(), Box<dyn std::error::Error>> {
        let unsigned = unsigned_object_revision(object_revision_body(
            vec![RevisionId::new("git:sha256:parent")],
            blob_id()?,
        )?)?;
        let mut signature = ed25519_signature(&unsigned)?;
        signature.bytes[0] ^= 1;
        let signed = SignedStatement::new(unsigned, signature);

        assert!(matches!(
            signed.verify_signature(&public_key()),
            Err(StatementSignatureError::Verification(
                SignatureVerificationError::InvalidSignature
            ))
        ));
        Ok(())
    }

    #[test]
    fn rejects_wrong_object_revision_public_key() -> Result<(), Box<dyn std::error::Error>> {
        let unsigned = unsigned_object_revision(object_revision_body(
            vec![RevisionId::new("git:sha256:parent")],
            blob_id()?,
        )?)?;
        let signature = ed25519_signature(&unsigned)?;
        let signed = SignedStatement::new(unsigned, signature);

        assert!(matches!(
            signed.verify_signature(&other_public_key()),
            Err(StatementSignatureError::Verification(
                SignatureVerificationError::InvalidSignature
            ))
        ));
        Ok(())
    }

    fn ed25519_signature<B: StatementBody>(
        unsigned: &UnsignedStatement<B>,
    ) -> Result<Signature, kairo_core::IdError> {
        let signature = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
        Ok(Signature::new(
            actor_id()?,
            public_key().key_id().to_string(),
            "ed25519",
            signature.to_vec(),
        ))
    }

    fn statement_id_one() -> StatementId {
        StatementId::from_sha256_digest([0x11; 32])
    }

    fn statement_id_two() -> StatementId {
        StatementId::from_sha256_digest([0x22; 32])
    }

    fn unsigned_object_branch(
        name: &str,
        revision: StatementId,
    ) -> Result<UnsignedStatement<ObjectBranchBody>, kairo_core::IdError> {
        Ok(UnsignedStatement::new(
            actor_id()?,
            object_ref()?,
            timestamp(),
            ObjectBranchBody::new(object_id()?, name, revision),
        ))
    }

    #[test]
    fn same_object_branch_body_produces_same_statement_id() -> Result<(), kairo_core::IdError> {
        let first = unsigned_object_branch("head", statement_id_one())?;
        let second = unsigned_object_branch("head", statement_id_one())?;

        assert_eq!(first.statement_id(), second.statement_id());
        Ok(())
    }

    #[test]
    fn object_branch_name_changes_statement_id() -> Result<(), kairo_core::IdError> {
        let first = unsigned_object_branch("head", statement_id_one())?;
        let second = unsigned_object_branch("release", statement_id_one())?;

        assert_ne!(first.statement_id(), second.statement_id());
        Ok(())
    }

    #[test]
    fn object_branch_revision_changes_statement_id() -> Result<(), kairo_core::IdError> {
        let first = unsigned_object_branch("head", statement_id_one())?;
        let second = unsigned_object_branch("head", statement_id_two())?;

        assert_ne!(first.statement_id(), second.statement_id());
        Ok(())
    }

    #[test]
    fn object_branch_created_at_changes_statement_id() -> Result<(), kairo_core::IdError> {
        let body = ObjectBranchBody::new(object_id()?, "head", statement_id_one());
        let first = UnsignedStatement::new(actor_id()?, object_ref()?, timestamp(), body.clone());
        let later = UnsignedStatement::new(
            actor_id()?,
            object_ref()?,
            Timestamp::from_seconds(timestamp().seconds() + 1),
            body,
        );

        assert_ne!(first.statement_id(), later.statement_id());
        Ok(())
    }

    #[test]
    fn object_branch_signature_does_not_change_statement_id() -> Result<(), kairo_core::IdError> {
        let unsigned = unsigned_object_branch("head", statement_id_one())?;
        let first = SignedStatement::new(unsigned.clone(), signature("k1", vec![1, 2, 3])?);
        let second = SignedStatement::new(unsigned, signature("k2", vec![4, 5, 6])?);

        assert_eq!(first.statement_id(), second.statement_id());
        Ok(())
    }

    #[test]
    fn verifies_object_branch_ed25519_signature() -> Result<(), Box<dyn std::error::Error>> {
        let unsigned = unsigned_object_branch("head", statement_id_one())?;
        let signature = ed25519_signature(&unsigned)?;
        let signed = SignedStatement::new(unsigned, signature);

        assert_eq!(
            signed.verify_signature(&public_key()),
            Ok(VerifiedSignature)
        );
        Ok(())
    }

    #[test]
    fn rejects_object_branch_signature_after_body_change() -> Result<(), Box<dyn std::error::Error>>
    {
        let signed_unsigned = unsigned_object_branch("head", statement_id_one())?;
        let tampered = unsigned_object_branch("release", statement_id_one())?;
        let signature = ed25519_signature(&signed_unsigned)?;
        let signed = SignedStatement::new(tampered, signature);

        assert!(matches!(
            signed.verify_signature(&public_key()),
            Err(StatementSignatureError::Verification(
                SignatureVerificationError::InvalidSignature
            ))
        ));
        Ok(())
    }

    fn unsigned_version_tag(
        version: &str,
        target: Option<StatementId>,
        supersedes: Option<StatementId>,
    ) -> Result<UnsignedStatement<ObjectVersionTagBody>, Box<dyn std::error::Error>> {
        let body = ObjectVersionTagBody::new(
            object_id()?,
            SemverVersion::parse(version)?,
            target,
            supersedes,
        )?;
        Ok(UnsignedStatement::new(
            actor_id()?,
            object_ref()?,
            timestamp(),
            body,
        ))
    }

    #[test]
    fn semver_version_rejects_non_semver_input() {
        let parsed = SemverVersion::parse("not.semver");
        assert!(matches!(parsed, Err(SemverParseError { .. })));
    }

    #[test]
    fn semver_version_rejects_leading_zeros() {
        let parsed = SemverVersion::parse("01.2.3");
        assert!(matches!(parsed, Err(SemverParseError { .. })));
    }

    #[test]
    fn semver_version_accepts_prerelease_and_build() {
        let parsed = SemverVersion::parse("1.2.3-rc.1+build.5");
        assert!(matches!(parsed, Ok(v) if v.as_str() == "1.2.3-rc.1+build.5"));
    }

    #[test]
    fn version_tag_body_rejects_revoke_without_supersedes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let body =
            ObjectVersionTagBody::new(object_id()?, SemverVersion::parse("1.2.3")?, None, None);
        assert_eq!(
            body.err(),
            Some(ObjectVersionTagShapeError::RevokeWithoutSupersedes)
        );
        Ok(())
    }

    #[test]
    fn version_tag_body_accepts_genesis_bind() -> Result<(), Box<dyn std::error::Error>> {
        let body = ObjectVersionTagBody::new(
            object_id()?,
            SemverVersion::parse("1.2.3")?,
            Some(statement_id_one()),
            None,
        )?;
        assert!(body.is_genesis());
        assert!(!body.is_revocation());
        Ok(())
    }

    #[test]
    fn version_tag_body_accepts_successor_revoke() -> Result<(), Box<dyn std::error::Error>> {
        let body = ObjectVersionTagBody::new(
            object_id()?,
            SemverVersion::parse("1.2.3")?,
            None,
            Some(statement_id_one()),
        )?;
        assert!(!body.is_genesis());
        assert!(body.is_revocation());
        Ok(())
    }

    #[test]
    fn same_version_tag_body_produces_same_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let first = unsigned_version_tag("1.2.3", Some(statement_id_one()), None)?;
        let second = unsigned_version_tag("1.2.3", Some(statement_id_one()), None)?;
        assert_eq!(first.statement_id(), second.statement_id());
        Ok(())
    }

    #[test]
    fn version_tag_version_changes_statement_id() -> Result<(), Box<dyn std::error::Error>> {
        let first = unsigned_version_tag("1.2.3", Some(statement_id_one()), None)?;
        let second = unsigned_version_tag("1.2.4", Some(statement_id_one()), None)?;
        assert_ne!(first.statement_id(), second.statement_id());
        Ok(())
    }

    #[test]
    fn version_tag_target_changes_statement_id() -> Result<(), Box<dyn std::error::Error>> {
        let first = unsigned_version_tag("1.2.3", Some(statement_id_one()), None)?;
        let second = unsigned_version_tag("1.2.3", Some(statement_id_two()), None)?;
        assert_ne!(first.statement_id(), second.statement_id());
        Ok(())
    }

    #[test]
    fn version_tag_revoke_differs_from_bind() -> Result<(), Box<dyn std::error::Error>> {
        let bind = unsigned_version_tag("1.2.3", Some(statement_id_one()), Some(statement_id_two()))?;
        let revoke = unsigned_version_tag("1.2.3", None, Some(statement_id_two()))?;
        assert_ne!(bind.statement_id(), revoke.statement_id());
        Ok(())
    }

    #[test]
    fn version_tag_supersedes_changes_statement_id() -> Result<(), Box<dyn std::error::Error>> {
        let genesis = unsigned_version_tag("1.2.3", Some(statement_id_one()), None)?;
        let successor = unsigned_version_tag(
            "1.2.3",
            Some(statement_id_one()),
            Some(statement_id_two()),
        )?;
        assert_ne!(genesis.statement_id(), successor.statement_id());
        Ok(())
    }

    #[test]
    fn version_tag_created_at_changes_statement_id() -> Result<(), Box<dyn std::error::Error>> {
        let body =
            ObjectVersionTagBody::new(object_id()?, SemverVersion::parse("1.2.3")?, Some(statement_id_one()), None)?;
        let first = UnsignedStatement::new(actor_id()?, object_ref()?, timestamp(), body.clone());
        let later = UnsignedStatement::new(
            actor_id()?,
            object_ref()?,
            Timestamp::from_seconds(timestamp().seconds() + 1),
            body,
        );
        assert_ne!(first.statement_id(), later.statement_id());
        Ok(())
    }

    #[test]
    fn version_tag_signature_does_not_change_statement_id() -> Result<(), Box<dyn std::error::Error>>
    {
        let unsigned = unsigned_version_tag("1.2.3", Some(statement_id_one()), None)?;
        let first = SignedStatement::new(unsigned.clone(), signature("k1", vec![1, 2, 3])?);
        let second = SignedStatement::new(unsigned, signature("k2", vec![4, 5, 6])?);
        assert_eq!(first.statement_id(), second.statement_id());
        Ok(())
    }

    #[test]
    fn verifies_version_tag_ed25519_signature() -> Result<(), Box<dyn std::error::Error>> {
        let unsigned = unsigned_version_tag("1.2.3", Some(statement_id_one()), None)?;
        let signature = ed25519_signature(&unsigned)?;
        let signed = SignedStatement::new(unsigned, signature);
        assert_eq!(
            signed.verify_signature(&public_key()),
            Ok(VerifiedSignature)
        );
        Ok(())
    }

    #[test]
    fn rejects_version_tag_signature_after_body_change() -> Result<(), Box<dyn std::error::Error>> {
        let signed_unsigned = unsigned_version_tag("1.2.3", Some(statement_id_one()), None)?;
        let tampered = unsigned_version_tag("1.2.4", Some(statement_id_one()), None)?;
        let sig = ed25519_signature(&signed_unsigned)?;
        let signed = SignedStatement::new(tampered, sig);
        assert!(matches!(
            signed.verify_signature(&public_key()),
            Err(StatementSignatureError::Verification(
                SignatureVerificationError::InvalidSignature
            ))
        ));
        Ok(())
    }

    fn trusted_actor() -> Result<ActorId, kairo_core::IdError> {
        // A second actor distinct from actor_id() so trusted != truster in tests.
        ActorId::new("zQmTbHEDi1jqyu1WKzmUaT9eJ48nWjMv55GrW88JArfCZUu")
    }

    fn unsigned_actor_trust(
        decision: Option<TrustDecision>,
        reason: Option<&str>,
        supersedes: Option<StatementId>,
    ) -> Result<UnsignedStatement<ActorTrustBody>, Box<dyn std::error::Error>> {
        let trusted = trusted_actor()?;
        let body = ActorTrustBody::new(
            trusted.clone(),
            decision,
            reason.map(|r| r.to_owned()),
            supersedes,
        )?;
        let subject: KairoRef = format!("actor:{trusted}").parse()?;
        Ok(UnsignedStatement::new(actor_id()?, subject, timestamp(), body))
    }

    #[test]
    fn trust_decision_parses_known_strings() {
        assert_eq!(TrustDecision::parse("trusted"), Ok(TrustDecision::Trusted));
        assert_eq!(
            TrustDecision::parse("untrusted"),
            Ok(TrustDecision::Untrusted)
        );
    }

    #[test]
    fn trust_decision_rejects_unknown_string() {
        let err = TrustDecision::parse("maybe");
        assert!(matches!(err, Err(TrustDecisionParseError { ref input }) if input == "maybe"));
    }

    #[test]
    fn actor_trust_body_rejects_withdraw_without_supersedes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let body = ActorTrustBody::new(trusted_actor()?, None, None, None);
        assert_eq!(
            body.err(),
            Some(ActorTrustShapeError::WithdrawWithoutSupersedes)
        );
        Ok(())
    }

    #[test]
    fn actor_trust_body_genesis_grant_is_valid() -> Result<(), Box<dyn std::error::Error>> {
        let body = ActorTrustBody::new(
            trusted_actor()?,
            Some(TrustDecision::Trusted),
            None,
            None,
        )?;
        assert!(body.is_genesis());
        assert!(!body.is_withdrawal());
        Ok(())
    }

    #[test]
    fn actor_trust_body_successor_withdraw_is_valid(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let body =
            ActorTrustBody::new(trusted_actor()?, None, None, Some(statement_id_one()))?;
        assert!(!body.is_genesis());
        assert!(body.is_withdrawal());
        Ok(())
    }

    #[test]
    fn same_actor_trust_body_produces_same_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let first = unsigned_actor_trust(Some(TrustDecision::Trusted), None, None)?;
        let second = unsigned_actor_trust(Some(TrustDecision::Trusted), None, None)?;
        assert_eq!(first.statement_id(), second.statement_id());
        Ok(())
    }

    #[test]
    fn actor_trust_decision_changes_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let trusted = unsigned_actor_trust(Some(TrustDecision::Trusted), None, None)?;
        let untrusted = unsigned_actor_trust(Some(TrustDecision::Untrusted), None, None)?;
        assert_ne!(trusted.statement_id(), untrusted.statement_id());
        Ok(())
    }

    #[test]
    fn actor_trust_withdraw_differs_from_grant(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let grant = unsigned_actor_trust(
            Some(TrustDecision::Trusted),
            None,
            Some(statement_id_one()),
        )?;
        let withdraw = unsigned_actor_trust(None, None, Some(statement_id_one()))?;
        assert_ne!(grant.statement_id(), withdraw.statement_id());
        Ok(())
    }

    #[test]
    fn actor_trust_reason_changes_statement_id() -> Result<(), Box<dyn std::error::Error>> {
        let without = unsigned_actor_trust(Some(TrustDecision::Trusted), None, None)?;
        let with = unsigned_actor_trust(
            Some(TrustDecision::Trusted),
            Some("verified at conference"),
            None,
        )?;
        assert_ne!(without.statement_id(), with.statement_id());
        Ok(())
    }

    #[test]
    fn actor_trust_supersedes_changes_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let genesis = unsigned_actor_trust(Some(TrustDecision::Trusted), None, None)?;
        let successor = unsigned_actor_trust(
            Some(TrustDecision::Trusted),
            None,
            Some(statement_id_one()),
        )?;
        assert_ne!(genesis.statement_id(), successor.statement_id());
        Ok(())
    }

    #[test]
    fn actor_trust_signature_does_not_change_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let unsigned = unsigned_actor_trust(Some(TrustDecision::Trusted), None, None)?;
        let first = SignedStatement::new(unsigned.clone(), signature("k1", vec![1, 2, 3])?);
        let second = SignedStatement::new(unsigned, signature("k2", vec![4, 5, 6])?);
        assert_eq!(first.statement_id(), second.statement_id());
        Ok(())
    }

    #[test]
    fn verifies_actor_trust_ed25519_signature() -> Result<(), Box<dyn std::error::Error>> {
        let unsigned = unsigned_actor_trust(Some(TrustDecision::Trusted), None, None)?;
        let signature = ed25519_signature(&unsigned)?;
        let signed = SignedStatement::new(unsigned, signature);
        assert_eq!(
            signed.verify_signature(&public_key()),
            Ok(VerifiedSignature)
        );
        Ok(())
    }

    #[test]
    fn rejects_actor_trust_signature_after_body_change(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let signed_unsigned = unsigned_actor_trust(Some(TrustDecision::Trusted), None, None)?;
        let tampered = unsigned_actor_trust(Some(TrustDecision::Untrusted), None, None)?;
        let sig = ed25519_signature(&signed_unsigned)?;
        let signed = SignedStatement::new(tampered, sig);
        assert!(matches!(
            signed.verify_signature(&public_key()),
            Err(StatementSignatureError::Verification(
                SignatureVerificationError::InvalidSignature
            ))
        ));
        Ok(())
    }

    fn grantee_actor() -> Result<ActorId, kairo_core::IdError> {
        // Third actor distinct from actor_id() and trusted_actor().
        ActorId::new("zQmZsHt8fzNFmDDYE3RZ7mTpCrz9rYpzXmFFvPbV5Q3KcAa")
    }

    fn object_scope() -> Result<CapabilityScope, kairo_core::IdError> {
        Ok(CapabilityScope::Object(object_id()?))
    }

    fn simple_capability(
        kinds: Vec<StatementKind>,
    ) -> Result<Capability, Box<dyn std::error::Error>> {
        Ok(Capability::new(object_scope()?, kinds, false, vec![])?)
    }

    fn unsigned_capability_grant(
        capability: Capability,
        supersedes: Option<StatementId>,
    ) -> Result<UnsignedStatement<ActorCapabilityGrantBody>, Box<dyn std::error::Error>> {
        let grantee = grantee_actor()?;
        let body = ActorCapabilityGrantBody::new(grantee.clone(), capability, supersedes);
        let subject: KairoRef = format!("actor:{grantee}").parse()?;
        Ok(UnsignedStatement::new(
            actor_id()?,
            subject,
            timestamp(),
            body,
        ))
    }

    fn unsigned_capability_revocation(
        revoked_grant: StatementId,
        retroactive: bool,
        reason: Option<&str>,
    ) -> Result<UnsignedStatement<ActorCapabilityRevocationBody>, Box<dyn std::error::Error>> {
        let body = ActorCapabilityRevocationBody::new(
            revoked_grant.clone(),
            retroactive,
            reason.map(|r| r.to_owned()),
        );
        let subject: KairoRef = format!("statement:{revoked_grant}").parse()?;
        Ok(UnsignedStatement::new(
            actor_id()?,
            subject,
            timestamp(),
            body,
        ))
    }

    // ---- StatementKind ----

    #[test]
    fn statement_kind_parses_known_strings() {
        assert_eq!(
            StatementKind::parse("ObjectVersionTag"),
            Ok(StatementKind::ObjectVersionTag)
        );
        assert_eq!(
            StatementKind::parse("ActorCapabilityGrant"),
            Ok(StatementKind::ActorCapabilityGrant)
        );
    }

    #[test]
    fn statement_kind_rejects_unknown_string() {
        let err = StatementKind::parse("WidgetCreated");
        assert!(matches!(err, Err(StatementKindParseError { ref input }) if input == "WidgetCreated"));
    }

    #[test]
    fn statement_kind_round_trips_through_as_str() {
        for kind in [
            StatementKind::ObjectGenesis,
            StatementKind::ObjectRevision,
            StatementKind::ObjectBranch,
            StatementKind::ObjectVersionTag,
            StatementKind::ActorTrust,
            StatementKind::ActorCapabilityGrant,
            StatementKind::ActorCapabilityRevocation,
        ] {
            assert_eq!(StatementKind::parse(kind.as_str()), Ok(kind));
        }
    }

    // ---- Capability shape ----

    #[test]
    fn capability_rejects_empty_statement_kinds() -> Result<(), Box<dyn std::error::Error>> {
        let err = Capability::new(object_scope()?, vec![], false, vec![]);
        assert_eq!(err.err(), Some(CapabilityShapeError::EmptyStatementKinds));
        Ok(())
    }

    #[test]
    fn capability_rejects_invalid_kind_for_object_scope(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let err = Capability::new(
            object_scope()?,
            vec![StatementKind::ActorTrust],
            false,
            vec![],
        );
        assert!(matches!(
            err,
            Err(CapabilityShapeError::KindInvalidForScope { kind: StatementKind::ActorTrust, .. })
        ));
        Ok(())
    }

    #[test]
    fn capability_rejects_any_kind_for_actor_scope_in_v1(
    ) -> Result<(), Box<dyn std::error::Error>> {
        // No actor-surface kinds are delegatable in v1.
        let err = Capability::new(
            CapabilityScope::Actor(actor_id()?),
            vec![StatementKind::ActorTrust],
            false,
            vec![],
        );
        assert!(matches!(
            err,
            Err(CapabilityShapeError::KindInvalidForScope { .. })
        ));
        Ok(())
    }

    #[test]
    fn capability_rejects_duplicate_constraint_variant(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let err = Capability::new(
            object_scope()?,
            vec![StatementKind::ObjectVersionTag],
            false,
            vec![
                CapabilityConstraint::MaxDelegationDepth(1),
                CapabilityConstraint::MaxDelegationDepth(2),
            ],
        );
        assert!(matches!(
            err,
            Err(CapabilityShapeError::DuplicateConstraintTag { tag: 0x01 })
        ));
        Ok(())
    }

    #[test]
    fn capability_sorts_and_deduplicates_statement_kinds(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cap = Capability::new(
            object_scope()?,
            vec![
                StatementKind::ObjectVersionTag,
                StatementKind::ObjectBranch,
                StatementKind::ObjectVersionTag, // duplicate
            ],
            false,
            vec![],
        )?;
        assert_eq!(
            cap.statement_kinds(),
            &[StatementKind::ObjectBranch, StatementKind::ObjectVersionTag]
        );
        Ok(())
    }

    #[test]
    fn capability_sorts_constraints_by_tag() -> Result<(), Box<dyn std::error::Error>> {
        // Inserted in non-tag order.
        let cap = Capability::new(
            object_scope()?,
            vec![StatementKind::ObjectVersionTag],
            true,
            vec![
                CapabilityConstraint::MaxDelegationDepth(2),
                CapabilityConstraint::ExpiresAt(Timestamp::from_seconds(1)),
            ],
        )?;
        let tags: Vec<u8> = cap.constraints().iter().map(|c| c.tag()).collect();
        assert_eq!(tags, vec![0x00, 0x01]);
        Ok(())
    }

    #[test]
    fn capability_canonical_bytes_independent_of_input_order(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let kinds_a = vec![StatementKind::ObjectBranch, StatementKind::ObjectVersionTag];
        let kinds_b = vec![StatementKind::ObjectVersionTag, StatementKind::ObjectBranch];
        let a = Capability::new(object_scope()?, kinds_a, false, vec![])?;
        let b = Capability::new(object_scope()?, kinds_b, false, vec![])?;
        assert_eq!(a.canonical_bytes(), b.canonical_bytes());
        Ok(())
    }

    // ---- ActorCapabilityGrant ----

    #[test]
    fn same_capability_grant_body_produces_same_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cap_a = simple_capability(vec![StatementKind::ObjectVersionTag])?;
        let cap_b = simple_capability(vec![StatementKind::ObjectVersionTag])?;
        let first = unsigned_capability_grant(cap_a, None)?;
        let second = unsigned_capability_grant(cap_b, None)?;
        assert_eq!(first.statement_id(), second.statement_id());
        Ok(())
    }

    #[test]
    fn capability_grant_grantee_changes_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cap = simple_capability(vec![StatementKind::ObjectVersionTag])?;
        let with_grantee_one = unsigned_capability_grant(cap.clone(), None)?;
        // Build a second statement with a different grantee but same scope/kinds.
        let other_grantee = trusted_actor()?;
        let body = ActorCapabilityGrantBody::new(other_grantee.clone(), cap, None);
        let subject: KairoRef = format!("actor:{other_grantee}").parse()?;
        let with_grantee_two =
            UnsignedStatement::new(actor_id()?, subject, timestamp(), body);
        assert_ne!(
            with_grantee_one.statement_id(),
            with_grantee_two.statement_id()
        );
        Ok(())
    }

    #[test]
    fn capability_grant_kinds_change_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let one = unsigned_capability_grant(
            simple_capability(vec![StatementKind::ObjectVersionTag])?,
            None,
        )?;
        let two = unsigned_capability_grant(
            simple_capability(vec![
                StatementKind::ObjectVersionTag,
                StatementKind::ObjectBranch,
            ])?,
            None,
        )?;
        assert_ne!(one.statement_id(), two.statement_id());
        Ok(())
    }

    #[test]
    fn capability_grant_delegable_changes_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let kinds = vec![StatementKind::ObjectVersionTag];
        let non_delegable = Capability::new(object_scope()?, kinds.clone(), false, vec![])?;
        let delegable = Capability::new(object_scope()?, kinds, true, vec![])?;
        let one = unsigned_capability_grant(non_delegable, None)?;
        let two = unsigned_capability_grant(delegable, None)?;
        assert_ne!(one.statement_id(), two.statement_id());
        Ok(())
    }

    #[test]
    fn capability_grant_constraint_changes_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let kinds = vec![StatementKind::ObjectVersionTag];
        let unconstrained = Capability::new(object_scope()?, kinds.clone(), false, vec![])?;
        let with_constraint = Capability::new(
            object_scope()?,
            kinds,
            false,
            vec![CapabilityConstraint::MaxDelegationDepth(3)],
        )?;
        let one = unsigned_capability_grant(unconstrained, None)?;
        let two = unsigned_capability_grant(with_constraint, None)?;
        assert_ne!(one.statement_id(), two.statement_id());
        Ok(())
    }

    #[test]
    fn capability_grant_supersedes_changes_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cap = simple_capability(vec![StatementKind::ObjectVersionTag])?;
        let genesis = unsigned_capability_grant(cap.clone(), None)?;
        let successor = unsigned_capability_grant(cap, Some(statement_id_one()))?;
        assert_ne!(genesis.statement_id(), successor.statement_id());
        Ok(())
    }

    #[test]
    fn capability_grant_signature_does_not_change_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let unsigned = unsigned_capability_grant(
            simple_capability(vec![StatementKind::ObjectVersionTag])?,
            None,
        )?;
        let first = SignedStatement::new(unsigned.clone(), signature("k1", vec![1, 2, 3])?);
        let second = SignedStatement::new(unsigned, signature("k2", vec![4, 5, 6])?);
        assert_eq!(first.statement_id(), second.statement_id());
        Ok(())
    }

    #[test]
    fn verifies_capability_grant_ed25519_signature() -> Result<(), Box<dyn std::error::Error>> {
        let unsigned = unsigned_capability_grant(
            simple_capability(vec![StatementKind::ObjectVersionTag])?,
            None,
        )?;
        let signature = ed25519_signature(&unsigned)?;
        let signed = SignedStatement::new(unsigned, signature);
        assert_eq!(
            signed.verify_signature(&public_key()),
            Ok(VerifiedSignature)
        );
        Ok(())
    }

    #[test]
    fn rejects_capability_grant_signature_after_body_change(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let signed_unsigned = unsigned_capability_grant(
            simple_capability(vec![StatementKind::ObjectVersionTag])?,
            None,
        )?;
        let tampered = unsigned_capability_grant(
            simple_capability(vec![StatementKind::ObjectBranch])?,
            None,
        )?;
        let sig = ed25519_signature(&signed_unsigned)?;
        let signed = SignedStatement::new(tampered, sig);
        assert!(matches!(
            signed.verify_signature(&public_key()),
            Err(StatementSignatureError::Verification(
                SignatureVerificationError::InvalidSignature
            ))
        ));
        Ok(())
    }

    // ---- ActorCapabilityRevocation ----

    #[test]
    fn same_capability_revocation_body_produces_same_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let first = unsigned_capability_revocation(statement_id_one(), false, None)?;
        let second = unsigned_capability_revocation(statement_id_one(), false, None)?;
        assert_eq!(first.statement_id(), second.statement_id());
        Ok(())
    }

    #[test]
    fn capability_revocation_revoked_grant_changes_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let one = unsigned_capability_revocation(statement_id_one(), false, None)?;
        let two = unsigned_capability_revocation(statement_id_two(), false, None)?;
        assert_ne!(one.statement_id(), two.statement_id());
        Ok(())
    }

    #[test]
    fn capability_revocation_retroactive_changes_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let default = unsigned_capability_revocation(statement_id_one(), false, None)?;
        let retro = unsigned_capability_revocation(statement_id_one(), true, None)?;
        assert_ne!(default.statement_id(), retro.statement_id());
        Ok(())
    }

    #[test]
    fn capability_revocation_reason_changes_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let without = unsigned_capability_revocation(statement_id_one(), false, None)?;
        let with = unsigned_capability_revocation(
            statement_id_one(),
            false,
            Some("delegate stepped down"),
        )?;
        assert_ne!(without.statement_id(), with.statement_id());
        Ok(())
    }

    #[test]
    fn capability_revocation_signature_does_not_change_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let unsigned = unsigned_capability_revocation(statement_id_one(), false, None)?;
        let first = SignedStatement::new(unsigned.clone(), signature("k1", vec![1, 2, 3])?);
        let second = SignedStatement::new(unsigned, signature("k2", vec![4, 5, 6])?);
        assert_eq!(first.statement_id(), second.statement_id());
        Ok(())
    }

    #[test]
    fn verifies_capability_revocation_ed25519_signature(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let unsigned = unsigned_capability_revocation(statement_id_one(), false, None)?;
        let signature = ed25519_signature(&unsigned)?;
        let signed = SignedStatement::new(unsigned, signature);
        assert_eq!(
            signed.verify_signature(&public_key()),
            Ok(VerifiedSignature)
        );
        Ok(())
    }

    #[test]
    fn rejects_capability_revocation_signature_after_body_change(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let signed_unsigned =
            unsigned_capability_revocation(statement_id_one(), false, None)?;
        let tampered = unsigned_capability_revocation(statement_id_one(), true, None)?;
        let sig = ed25519_signature(&signed_unsigned)?;
        let signed = SignedStatement::new(tampered, sig);
        assert!(matches!(
            signed.verify_signature(&public_key()),
            Err(StatementSignatureError::Verification(
                SignatureVerificationError::InvalidSignature
            ))
        ));
        Ok(())
    }
}
