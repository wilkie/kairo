//! Per-signer index of actor chains (`specs/DECISIONS.md` §23).
//!
//! Each file at `<root>/actor_chains/<XX>/<YY>/<actor-id>.json` holds
//! every chained statement the actor signed, grouped by chain: its
//! `previous` and `created_at`. The file is sharded on the **signer**,
//! since a chain belongs to one signer and every query names it.
//!
//! ```json
//! {
//!   "object:<id>": { "<statement-id>": { "previous": ["..."], "created_at": "..." } },
//!   "keys":        { "...": { ... } },
//!   "grant:<id>":  { "...": { ... } }
//! }
//! ```
//!
//! An `ActorCapabilityRevocation` chains in its grant's scope, which
//! only the grant knows, and the grant may arrive after the
//! revocation. So revocations are filed under `grant:<grant-id>` and
//! resolved to a scope when a chain is read. Placement never depends on
//! arrival order, and a rebuild in any order produces the same file.
//!
//! The index is a strict materialization of `statements/`:
//! `rebuild_indexes()` reproduces it. Multi-process safety comes from
//! `lock::with_index_lock` on the per-signer sidecar.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use kairo_core::{ActorId, StatementId, Timestamp};
use kairo_statement::{ChainKey, ChainScope, Previous, StatementBody, UnsignedStatement};
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};
use crate::lock::with_index_lock;
use crate::{
    atomic_write, json_to_corrupt, read_or_missing, FilesystemStore, StatementStore, JSON_SUFFIX,
};

pub(crate) const ACTOR_CHAINS_DIR: &str = "actor_chains";

const GRANT_KEY_PREFIX: &str = "grant:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ChainLink {
    previous: Vec<String>,
    created_at: String,
}

/// A chain's links (statement → its `previous`) and the signer's
/// revocations whose grant isn't held (revocation → grant).
#[derive(Debug, Default)]
struct ChainLinks {
    links: BTreeMap<StatementId, Vec<StatementId>>,
    unplaced: BTreeMap<StatementId, StatementId>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
struct ActorChainsFile(BTreeMap<String, BTreeMap<String, ChainLink>>);

fn chain_key_string(key: &ChainKey) -> String {
    match key {
        ChainKey::Scope(scope) => scope.key(),
        ChainKey::RevokedGrant(grant) => format!("{GRANT_KEY_PREFIX}{grant}"),
    }
}

/// Whether a node holds everything a chain names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainCompleteness {
    /// Every `previous` of every counted statement is held.
    Complete,
    /// These ids are named by some counted statement's `previous` but
    /// not held. Their own predecessors are unknown until they arrive.
    Incomplete { missing: Vec<StatementId> },
}

/// One chain as this node holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainReport {
    pub signer: ActorId,
    pub scope: ChainScope,
    /// Held statements no other held statement of the chain names.
    /// Ascending. What the signer's next statement here must name.
    pub tips: Vec<StatementId>,
    pub completeness: ChainCompleteness,
    /// Statements whose `previous` names a held statement outside this
    /// chain (another signer, another scope, an unchained kind). Such a
    /// statement is `Invalid` (`CORE_LIBRARY.md` §12.2) and its
    /// `previous` is not counted.
    pub contradictions: Vec<StatementId>,
}

fn corrupt(actor: &ActorId, message: String) -> StoreError {
    StoreError::Corrupt {
        id: actor.to_string(),
        reason: CorruptReason::Parse(message),
    }
}

fn parse_id(actor: &ActorId, raw: &str) -> Result<StatementId, StoreError> {
    StatementId::new(raw.to_owned()).map_err(|error| {
        corrupt(
            actor,
            format!("invalid id {raw:?} in actor chain index: {error}"),
        )
    })
}

impl FilesystemStore {
    fn actor_chains_path(&self, actor: &ActorId) -> Result<PathBuf, StoreError> {
        self.shard_path(ACTOR_CHAINS_DIR, actor.as_str(), JSON_SUFFIX)
    }

    fn read_actor_chains(&self, actor: &ActorId) -> Result<ActorChainsFile, StoreError> {
        match read_or_missing(&self.actor_chains_path(actor)?) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| corrupt(actor, format!("invalid actor chain index: {error}"))),
            Err(StoreError::Missing) => Ok(ActorChainsFile::default()),
            Err(error) => Err(error),
        }
    }

    /// Record that `signer` signed `id` in `placement` with `previous`.
    pub(crate) fn upsert_actor_chain(
        &self,
        signer: &ActorId,
        key: &ChainKey,
        id: &StatementId,
        previous: &Previous,
        created_at: Timestamp,
    ) -> Result<(), StoreError> {
        let path = self.actor_chains_path(signer)?;
        with_index_lock(&path, || {
            let mut file = self.read_actor_chains(signer)?;
            let link = ChainLink {
                previous: previous.ids().iter().map(ToString::to_string).collect(),
                created_at: created_at.to_string(),
            };
            let chain = file.0.entry(chain_key_string(key)).or_default();
            if chain.get(id.as_str()) == Some(&link) {
                return Ok(());
            }
            chain.insert(id.to_string(), link);
            let bytes = serde_json::to_vec_pretty(&file).map_err(json_to_corrupt(signer))?;
            atomic_write(&path, &bytes)
        })
    }

    /// The links of `signer`'s chain in `scope`, revocations resolved
    /// through their grants. A revocation whose grant isn't held has no
    /// known scope (`DECISIONS.md` §23 sub-decision 2); those are
    /// returned separately, mapped to the grant they wait on.
    fn chain_links(&self, signer: &ActorId, scope: &ChainScope) -> Result<ChainLinks, StoreError> {
        let file = self.read_actor_chains(signer)?;
        let mut chain_links = ChainLinks::default();
        for (key, chain) in &file.0 {
            let waiting_on = match key.strip_prefix(GRANT_KEY_PREFIX) {
                Some(grant) => {
                    let grant = parse_id(signer, grant)?;
                    match self.grant_scope(&grant)? {
                        Some(grant_scope) if &grant_scope == scope => None,
                        Some(_) => continue,
                        None => Some(grant),
                    }
                }
                None if key == &scope.key() => None,
                None => continue,
            };
            for (id, link) in chain {
                let id = parse_id(signer, id)?;
                if let Some(grant) = &waiting_on {
                    chain_links.unplaced.insert(id, grant.clone());
                    continue;
                }
                let previous = link
                    .previous
                    .iter()
                    .map(|raw| parse_id(signer, raw))
                    .collect::<Result<Vec<_>, _>>()?;
                chain_links.links.insert(id, previous);
            }
        }
        Ok(chain_links)
    }

    /// The chain scope of a held grant, `None` if it isn't held.
    fn grant_scope(&self, grant: &StatementId) -> Result<Option<ChainScope>, StoreError> {
        match self.get_actor_capability_grant(grant) {
            Ok(statement) => Ok(Some(ChainScope::for_capability(
                statement.unsigned().body().capability().scope(),
            ))),
            Err(StoreError::Missing) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Every chain scope `signer` has statements in, ascending.
    /// Revocations whose grant isn't held contribute no scope.
    pub fn actor_chain_scopes(&self, signer: &ActorId) -> Result<Vec<ChainScope>, StoreError> {
        let file = self.read_actor_chains(signer)?;
        let mut scopes = BTreeSet::new();
        for key in file.0.keys() {
            match key.strip_prefix(GRANT_KEY_PREFIX) {
                Some(grant) => {
                    if let Some(scope) = self.grant_scope(&parse_id(signer, grant)?)? {
                        scopes.insert(scope);
                    }
                }
                None => {
                    let scope = ChainScope::parse_key(key)
                        .ok_or_else(|| corrupt(signer, format!("invalid chain scope {key:?}")))?;
                    scopes.insert(scope);
                }
            }
        }
        Ok(scopes.into_iter().collect())
    }

    /// The tips of `signer`'s chain in `scope`: what its next statement
    /// there names. Empty for a chain with no statements. Every held
    /// statement counts, including ones a verifier would flag.
    pub fn actor_chain_tips(
        &self,
        signer: &ActorId,
        scope: &ChainScope,
    ) -> Result<Vec<StatementId>, StoreError> {
        Ok(tips(&self.chain_links(signer, scope)?.links))
    }

    /// `unsigned` with `previous` set to its signer's current tips in
    /// its chain: what every write path signs (`DECISIONS.md` §23
    /// sub-decision 5). Unchained kinds come back unchanged. A
    /// revocation whose grant isn't held, and so has no known scope,
    /// starts with an empty `previous`. With more than
    /// [`kairo_statement::MAX_PREVIOUS`] tips, the lowest ids are named
    /// and the rest merge on later writes.
    pub fn link_to_chain<B: StatementBody>(
        &self,
        unsigned: UnsignedStatement<B>,
    ) -> Result<UnsignedStatement<B>, StoreError> {
        let scope = match unsigned.body().chain_key() {
            None => return Ok(unsigned),
            Some(ChainKey::Scope(scope)) => scope,
            Some(ChainKey::RevokedGrant(grant)) => match self.grant_scope(&grant)? {
                Some(scope) => scope,
                None => return Ok(unsigned),
            },
        };
        let mut tips = self.actor_chain_tips(unsigned.actor(), &scope)?;
        tips.truncate(kairo_statement::MAX_PREVIOUS);
        let previous = Previous::new(tips).map_err(|error| StoreError::Rejected {
            reason: error.to_string(),
        })?;
        unsigned
            .with_previous(previous)
            .map_err(|error| StoreError::Rejected {
                reason: error.to_string(),
            })
    }

    /// `signer`'s chain in `scope` as this node holds it.
    ///
    /// `discount` names statements whose `previous` must not count
    /// toward completeness: those signed with a key revoked at their
    /// `created_at` (`DECISIONS.md` §23 sub-decision 6). Key
    /// resolution lives above the store, so the caller decides. A
    /// discounted statement still counts as held when another names
    /// it.
    pub fn actor_chain_report(
        &self,
        signer: &ActorId,
        scope: &ChainScope,
        discount: impl Fn(&StatementId) -> bool,
    ) -> Result<ChainReport, StoreError> {
        let ChainLinks { links, unplaced } = self.chain_links(signer, scope)?;
        let mut missing = BTreeSet::new();
        let mut contradictions = Vec::new();
        for (id, previous) in &links {
            if discount(id) {
                continue;
            }
            let mut contradicts = false;
            let mut absent = Vec::new();
            for named in previous {
                if links.contains_key(named) {
                    continue;
                }
                // The signer's own revocation, waiting on its grant: a
                // gap, and the grant is what's missing.
                if let Some(grant) = unplaced.get(named) {
                    absent.push(grant.clone());
                    continue;
                }
                match self.get_statement_bytes(named) {
                    // Held, but not a statement of this chain.
                    Ok(_) => contradicts = true,
                    Err(StoreError::Missing) => absent.push(named.clone()),
                    Err(error) => return Err(error),
                }
            }
            if contradicts {
                contradictions.push(id.clone());
            } else {
                missing.extend(absent);
            }
        }
        let completeness = if missing.is_empty() {
            ChainCompleteness::Complete
        } else {
            ChainCompleteness::Incomplete {
                missing: missing.into_iter().collect(),
            }
        };
        Ok(ChainReport {
            signer: signer.clone(),
            scope: scope.clone(),
            tips: tips(&links),
            completeness,
            contradictions,
        })
    }
}

/// Held statements no other held statement names, ascending.
fn tips(links: &BTreeMap<StatementId, Vec<StatementId>>) -> Vec<StatementId> {
    let named: BTreeSet<&StatementId> = links.values().flatten().collect();
    links
        .keys()
        .filter(|id| !named.contains(id))
        .cloned()
        .collect()
}

/// The envelope's `type`, read without parsing the rest.
#[derive(Deserialize)]
struct KindOnly {
    #[serde(rename = "type")]
    kind: String,
}

impl FilesystemStore {
    /// Whether the held statement `id` was signed with a key revoked
    /// at its `created_at`. Such a statement is stored and flagged
    /// (`DECISIONS.md` §16), and its `previous` doesn't count toward
    /// completeness (§23 sub-decision 6). Unchained kinds are never
    /// chain members, so they answer `false`.
    pub fn signed_with_revoked_key(&self, id: &StatementId) -> Result<bool, StoreError> {
        use kairo_statement::verify::{
            verify_envelope_multi_statement, verify_envelope_statement, SignatureStatus,
        };
        use kairo_statement::StatementKind;

        let bytes = self.get_statement_bytes(id)?;
        let kind: KindOnly =
            serde_json::from_slice(&bytes).map_err(|error| StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::Parse(error.to_string()),
            })?;
        let kind = StatementKind::parse(&kind.kind).map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let status = match kind {
            StatementKind::ObjectBranch => {
                verify_envelope_statement(&self.get_object_branch(id)?, self).signature
            }
            StatementKind::ObjectVersionTag => {
                verify_envelope_statement(&self.get_object_version_tag(id)?, self).signature
            }
            StatementKind::ActorCapabilityGrant => {
                verify_envelope_statement(&self.get_actor_capability_grant(id)?, self).signature
            }
            StatementKind::ActorCapabilityRevocation => {
                verify_envelope_statement(&self.get_actor_capability_revocation(id)?, self)
                    .signature
            }
            StatementKind::ActorKeyRotation => {
                verify_envelope_statement(&self.get_actor_key_rotation(id)?, self).signature
            }
            StatementKind::ActorKeyRevocation => {
                verify_envelope_statement(&self.get_actor_key_revocation(id)?, self).signature
            }
            StatementKind::ActorEmergencyKeyRotation => {
                verify_envelope_multi_statement(&self.get_actor_emergency_key_rotation(id)?, self)
                    .signature
            }
            StatementKind::ActorEmergencyKeyRevocation => {
                verify_envelope_multi_statement(&self.get_actor_emergency_key_revocation(id)?, self)
                    .signature
            }
            StatementKind::ActorAttestationKeyAdd => {
                verify_envelope_multi_statement(&self.get_actor_attestation_key_add(id)?, self)
                    .signature
            }
            StatementKind::ActorAttestationKeyRevocation => {
                verify_envelope_multi_statement(
                    &self.get_actor_attestation_key_revocation(id)?,
                    self,
                )
                .signature
            }
            StatementKind::ActorAttestationThresholdChange => {
                verify_envelope_multi_statement(
                    &self.get_actor_attestation_threshold_change(id)?,
                    self,
                )
                .signature
            }
            StatementKind::ObjectRevision
            | StatementKind::ActorTrust
            | StatementKind::ObjectGenesis => return Ok(false),
        };
        Ok(status == SignatureStatus::KeyRevoked)
    }

    /// The chains a verification of `object` depends on
    /// (`DECISIONS.md` §23 sub-decision 8), as this node holds them,
    /// revoked-key statements discounted:
    ///
    /// - the actors consulted: the genesis creator, every actor in
    ///   `signers` (the signers of the resolved branch, tag, and
    ///   revision), and the grantors of any object-scoped grant held
    ///   to one of those signers (the authority path);
    /// - for each, its chain in the object's scope and its `keys`
    ///   chain.
    ///
    /// Ordered by signer, then scope. A chain with no statements is
    /// trivially complete and is included, so callers can show what
    /// was checked.
    pub fn consulted_chains(
        &self,
        object: &kairo_core::ObjectId,
        signers: &[ActorId],
    ) -> Result<Vec<ChainReport>, StoreError> {
        use crate::{CapabilityResolver, ObjectStore};

        let mut actors: BTreeSet<ActorId> = signers.iter().cloned().collect();
        actors.insert(self.get_object_genesis(object)?.body().created_by().clone());
        let grantors: Vec<ActorId> = self
            .list_capabilities_for_object(object)?
            .into_iter()
            .filter(|head| actors.contains(&head.grantee))
            .map(|head| head.grantor)
            .collect();
        actors.extend(grantors);

        let discount = |id: &StatementId| self.signed_with_revoked_key(id).unwrap_or(false);
        let mut reports = Vec::new();
        for actor in &actors {
            for scope in [ChainScope::Object(object.clone()), ChainScope::Keys] {
                reports.push(self.actor_chain_report(actor, &scope, discount)?);
            }
        }
        Ok(reports)
    }
}
