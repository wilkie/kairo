//! Verify one fetched record and store it.
//!
//! Every record is checked before `put_*`:
//!
//! 1. **Schema**: it parses as its kind's `kairo-statement` DTO.
//! 2. **Fixity**: the id re-derived from its canonical bytes equals
//!    the id it was requested by.
//! 3. **Section**: its kind and target match where the listing put it
//!    (an object statement names this object, a grant is scoped to
//!    it, a revocation revokes a listed grant, a key event belongs to
//!    the listed actor). `ActorTrust` is never accepted.
//! 4. **Signature**: it verifies against the signer's keys as already
//!    stored, which is why the caller ingests actors and their key
//!    events first, in `created_at` order.
//!
//! A statement whose signing key is revoked at its `created_at` is
//! stored and flagged, not refused: the bytes are genuine, the
//! revocation is a verified fact about the actor, and the origin
//! holds the same statement. Dropping it would make what a node
//! stores depend on the order key events arrive in. Read-time
//! verification reports it as revoked. Capability checks are not run
//! here either: a verified statement whose signer lacks the capability
//! is stored, and the resolver decides (`FEDERATION.md` "Conflict
//! Handling").

use std::collections::BTreeSet;

use kairo_core::{ActorId, ObjectId, StatementId, Timestamp};
use kairo_identity::{verify_signature, ActorResolver};
use kairo_statement::json::{
    ActorAttestationKeyAddStatementJson, ActorAttestationKeyRevocationStatementJson,
    ActorAttestationThresholdChangeStatementJson, ActorCapabilityGrantStatementJson,
    ActorCapabilityRevocationStatementJson, ActorEmergencyKeyRevocationStatementJson,
    ActorEmergencyKeyRotationStatementJson, ActorKeyRevocationStatementJson,
    ActorKeyRotationStatementJson, ObjectBranchStatementJson, ObjectGenesisStatementJson,
    ObjectRevisionStatementJson, ObjectVersionTagStatementJson,
};
use kairo_statement::verify::{
    verify_envelope_multi_statement, verify_envelope_statement, ActorResolution, SignatureStatus,
    VerificationReport,
};
use kairo_statement::{CapabilityScope, ObjectGenesisStatement, StatementKind};
use kairo_store::{FilesystemStore, ObjectStore, StatementStore, StoreError};
use serde::Deserialize;

use super::RecordRejection;

/// Where a statement was listed, and so what it must be.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Section<'a> {
    /// `ObjectRevision`, `ObjectBranch`, or `ObjectVersionTag` naming
    /// the object.
    Object(&'a ObjectId),
    /// A grant scoped to the object, or a revocation of one of
    /// `grants`.
    Authority {
        object: &'a ObjectId,
        grants: &'a BTreeSet<StatementId>,
    },
    /// A key event signed by the actor.
    KeyEvent(&'a ActorId),
}

/// What happened to a verified statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ingested {
    Stored,
    /// Stored, but the signing key is revoked at `created_at`.
    StoredRevokedKey,
}

/// The envelope fields ordering needs, read before full parsing.
#[derive(Debug, Deserialize)]
struct PeekJson {
    #[serde(rename = "type")]
    kind: String,
    created_at: String,
    /// Required by full parsing; tolerated here so a malformed record
    /// is refused there, with the precise error.
    #[serde(default)]
    previous: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Peek {
    pub kind: StatementKind,
    pub created_at: Timestamp,
    /// The signer's chain tips (`DECISIONS.md` §23): ingest orders a
    /// chain's statements after what they name.
    pub previous: Vec<StatementId>,
}

pub(crate) fn peek(bytes: &[u8]) -> Result<Peek, RecordRejection> {
    let peek: PeekJson = serde_json::from_slice(bytes)
        .map_err(|e| RecordRejection::Schema(format!("missing type or created_at: {e}")))?;
    let kind = StatementKind::parse(&peek.kind)
        .map_err(|e| RecordRejection::Schema(format!("unknown statement kind: {e}")))?;
    let created_at = peek
        .created_at
        .parse()
        .map_err(|e| RecordRejection::Schema(format!("invalid created_at: {e}")))?;
    let previous = peek
        .previous
        .into_iter()
        .map(|id| {
            StatementId::new(id)
                .map_err(|e| RecordRejection::Schema(format!("invalid previous: {e}")))
        })
        .collect::<Result<_, _>>()?;
    Ok(Peek {
        kind,
        created_at,
        previous,
    })
}

fn schema(error: impl std::fmt::Display) -> RecordRejection {
    RecordRejection::Schema(error.to_string())
}

fn fixity(expected: &StatementId, actual: StatementId) -> Result<(), RecordRejection> {
    if &actual == expected {
        Ok(())
    } else {
        Err(RecordRejection::Fixity {
            actual: actual.to_string(),
        })
    }
}

fn wrong_section(message: impl Into<String>) -> RecordRejection {
    RecordRejection::WrongSection(message.into())
}

/// Collapse a verification report into the ingest outcome.
fn signature_outcome(report: &VerificationReport) -> Result<Ingested, RecordRejection> {
    match &report.actor {
        ActorResolution::Resolved => {}
        ActorResolution::SignatureActorMismatch => {
            return Err(RecordRejection::Signature(
                "envelope actor does not match signature actor".to_owned(),
            ))
        }
        ActorResolution::NotFound => {
            return Err(RecordRejection::Signature(format!(
                "signer {} is not known; the peer did not list it",
                report.envelope_actor
            )))
        }
        ActorResolution::ResolverUnavailable(reason) => {
            return Err(RecordRejection::Store(reason.clone()))
        }
    }
    match &report.signature {
        SignatureStatus::Valid => Ok(Ingested::Stored),
        SignatureStatus::KeyRevoked => Ok(Ingested::StoredRevokedKey),
        other => Err(RecordRejection::Signature(format!("{other:?}"))),
    }
}

fn store(error: StoreError) -> RecordRejection {
    RecordRejection::Store(error.to_string())
}

/// Verify `bytes` as the statement `expected`, listed in `section`,
/// and store it if it verifies.
pub(crate) fn ingest_statement(
    store_: &FilesystemStore,
    expected: &StatementId,
    bytes: &[u8],
    section: Section<'_>,
) -> Result<Ingested, RecordRejection> {
    let kind = peek(bytes)?.kind;

    // Single-signed envelope kinds.
    macro_rules! single {
        ($dto:ty, $put:ident, |$signed:ident| $check:expr) => {{
            let dto: $dto = serde_json::from_slice(bytes).map_err(schema)?;
            let $signed = dto.to_statement().map_err(schema)?;
            fixity(expected, $signed.statement_id())?;
            $check?;
            let outcome = signature_outcome(&verify_envelope_statement(&$signed, store_))?;
            store_.$put(&$signed).map_err(store)?;
            Ok(outcome)
        }};
    }
    // Attestation-surface (multi-signed) kinds.
    macro_rules! multi {
        ($dto:ty, $put:ident, |$signed:ident| $check:expr) => {{
            let dto: $dto = serde_json::from_slice(bytes).map_err(schema)?;
            let $signed = dto.to_statement().map_err(schema)?;
            fixity(expected, $signed.statement_id())?;
            $check?;
            let outcome = signature_outcome(&verify_envelope_multi_statement(&$signed, store_))?;
            store_.$put(&$signed).map_err(store)?;
            Ok(outcome)
        }};
    }

    let object_section = |object: &ObjectId| match section {
        Section::Object(listed) if listed == object => Ok(()),
        Section::Object(listed) => Err(wrong_section(format!(
            "statement names object {object}, listed under {listed}"
        ))),
        _ => Err(wrong_section(format!(
            "{} listed outside the object statements",
            kind.as_str()
        ))),
    };
    let key_event_section = |actor: &ActorId| match section {
        Section::KeyEvent(listed) if listed == actor => Ok(()),
        _ => Err(wrong_section(format!(
            "{} by {actor} not listed as that actor's key event",
            kind.as_str()
        ))),
    };

    match kind {
        StatementKind::ObjectRevision => {
            single!(ObjectRevisionStatementJson, put_object_revision, |signed| {
                object_section(signed.unsigned().body().object()).and_then(|()| {
                    kairo_statement::check_revision_ids(signed.unsigned().body()).map_err(schema)
                })
            })
        }
        StatementKind::ObjectBranch => {
            single!(ObjectBranchStatementJson, put_object_branch, |signed| {
                object_section(signed.unsigned().body().object())
            })
        }
        StatementKind::ObjectVersionTag => single!(
            ObjectVersionTagStatementJson,
            put_object_version_tag,
            |signed| object_section(signed.unsigned().body().object())
        ),
        StatementKind::ActorCapabilityGrant => single!(
            ActorCapabilityGrantStatementJson,
            put_actor_capability_grant,
            |signed| match (section, signed.unsigned().body().capability().scope()) {
                (Section::Authority { object, .. }, CapabilityScope::Object(scope))
                    if scope == object =>
                {
                    Ok(())
                }
                _ => Err(wrong_section("grant is not scoped to the listed object")),
            }
        ),
        StatementKind::ActorCapabilityRevocation => single!(
            ActorCapabilityRevocationStatementJson,
            put_actor_capability_revocation,
            |signed| match section {
                Section::Authority { grants, .. }
                    if grants.contains(signed.unsigned().body().revoked_grant()) =>
                {
                    Ok(())
                }
                _ => Err(wrong_section("revocation does not revoke a listed grant")),
            }
        ),
        StatementKind::ActorKeyRotation => single!(
            ActorKeyRotationStatementJson,
            put_actor_key_rotation,
            |signed| key_event_section(signed.unsigned().actor())
        ),
        StatementKind::ActorKeyRevocation => single!(
            ActorKeyRevocationStatementJson,
            put_actor_key_revocation,
            |signed| key_event_section(signed.unsigned().actor())
        ),
        StatementKind::ActorEmergencyKeyRotation => multi!(
            ActorEmergencyKeyRotationStatementJson,
            put_actor_emergency_key_rotation,
            |signed| key_event_section(signed.unsigned().actor())
        ),
        StatementKind::ActorEmergencyKeyRevocation => multi!(
            ActorEmergencyKeyRevocationStatementJson,
            put_actor_emergency_key_revocation,
            |signed| key_event_section(signed.unsigned().actor())
        ),
        StatementKind::ActorAttestationKeyAdd => multi!(
            ActorAttestationKeyAddStatementJson,
            put_actor_attestation_key_add,
            |signed| key_event_section(signed.unsigned().actor())
        ),
        StatementKind::ActorAttestationKeyRevocation => multi!(
            ActorAttestationKeyRevocationStatementJson,
            put_actor_attestation_key_revocation,
            |signed| key_event_section(signed.unsigned().actor())
        ),
        StatementKind::ActorAttestationThresholdChange => multi!(
            ActorAttestationThresholdChangeStatementJson,
            put_actor_attestation_threshold_change,
            |signed| key_event_section(signed.unsigned().actor())
        ),
        StatementKind::ActorTrust => Err(wrong_section(
            "ActorTrust is never accepted from a peer (DECISIONS.md §16 sub-decision 4)",
        )),
        StatementKind::ObjectGenesis => Err(wrong_section(
            "ObjectGenesis is fetched by object id, not as a statement",
        )),
    }
}

/// Verify `bytes` as `object`'s `ObjectGenesis` and store it. The
/// genesis signature covers the body, signed by `created_by`'s key
/// active at `created_at`.
pub(crate) fn ingest_genesis(
    store_: &FilesystemStore,
    object: &ObjectId,
    bytes: &[u8],
) -> Result<(), RecordRejection> {
    let dto: ObjectGenesisStatementJson = serde_json::from_slice(bytes).map_err(schema)?;
    let genesis: ObjectGenesisStatement = dto.to_statement().map_err(schema)?;
    let derived = genesis.object_id();
    if &derived != object {
        return Err(RecordRejection::Fixity {
            actual: derived.to_string(),
        });
    }
    let creator = genesis.body().created_by();
    if genesis.signature().actor() != creator {
        return Err(RecordRejection::Signature(
            "genesis signature actor is not created_by".to_owned(),
        ));
    }
    let key = store_
        .active_key_at(creator, genesis.body().created_at())
        .map_err(|e| RecordRejection::Store(e.to_string()))?
        .ok_or_else(|| {
            RecordRejection::Signature(format!(
                "creator {creator} is not known; the peer did not list it"
            ))
        })?;
    let signature = genesis
        .signature()
        .to_signature_bytes()
        .map_err(|e| RecordRejection::Signature(e.to_string()))?;
    verify_signature(&key, &genesis.signed_bytes(), &signature)
        .map_err(|e| RecordRejection::Signature(e.to_string()))?;
    store_.put_object_genesis(&genesis).map_err(store)?;
    Ok(())
}
