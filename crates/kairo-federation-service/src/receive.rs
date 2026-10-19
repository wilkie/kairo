//! Processing the inbox (`specs/DECISIONS.md` §26 sub-decision 4,
//! §28 sub-decision 3): every announcement is pulled and every upload
//! ingested through the pull verifier, after re-checking the sender's
//! `federation:push` trust and the accept list, and each entry is then
//! removed whatever happened to it.

use std::path::{Path, PathBuf};

use kairo_config::FederationConfig;
use kairo_core::{ActorId, ObjectId, Timestamp};
use kairo_federation_http::client::{ingest_closure_bundle, pull_object, PullOptions, PullReport};
use kairo_statement::{TrustDecision, TrustScope};
use kairo_store::{
    FilesystemStore, InboxAnnouncement, InboxItem, InboxStore, InboxUpload, PublishedObjectStore,
    PullLogStore, PullRecord, TrustResolver,
};

use crate::{limits_from, SyncError};

/// How an object reached the inbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// Pulled from the announcer's base URL.
    Announcement,
    /// Ingested from an uploaded closure bundle.
    Upload,
}

#[derive(Debug)]
pub enum ReceiveOutcome {
    /// Verified and stored. `published` when the accept entry said
    /// `publish = true` and publishing succeeded.
    Received {
        report: Box<PullReport>,
        published: bool,
        /// The pull log couldn't be written; the records are stored.
        pull_log_error: Option<String>,
    },
    /// Trust or the accept list no longer allows it.
    Refused {
        reason: String,
    },
    Failed {
        reason: String,
    },
}

/// One object from one inbox entry.
#[derive(Debug)]
pub struct Received {
    pub entry: String,
    pub via: Via,
    pub from: ActorId,
    pub object: ObjectId,
    pub outcome: ReceiveOutcome,
}

/// Process every inbox entry once, then remove it. `config_path` names
/// the config in a [`SyncError::Disabled`].
pub fn receive_inbox(
    store: &FilesystemStore,
    config: &FederationConfig,
    config_path: PathBuf,
) -> Result<Vec<Received>, SyncError> {
    if !config.enabled {
        return Err(SyncError::Disabled {
            config: config_path,
        });
    }
    let mut received = Vec::new();
    for entry in store.list_inbox().map_err(SyncError::Store)? {
        match &entry.item {
            InboxItem::Announcement(announcement) => {
                for (object, outcome) in receive_announcement(store, config, announcement) {
                    received.push(Received {
                        entry: entry.name.clone(),
                        via: Via::Announcement,
                        from: announcement.from.clone(),
                        object,
                        outcome,
                    });
                }
            }
            InboxItem::Upload(upload, bundle) => received.push(Received {
                entry: entry.name.clone(),
                via: Via::Upload,
                from: upload.from.clone(),
                object: upload.object.clone(),
                outcome: receive_upload(store, config, upload, bundle),
            }),
        }
        store
            .remove_inbox_entry(&entry.name)
            .map_err(SyncError::Store)?;
    }
    Ok(received)
}

/// `Some` refusal unless `to` still trusts `from` with
/// `federation:push`.
fn trust_refusal(store: &FilesystemStore, to: &ActorId, from: &ActorId) -> Option<ReceiveOutcome> {
    let trusted = store
        .latest_trust(to, from, &TrustScope::federation_push())
        .map(|head| {
            head.is_some_and(|signed| {
                signed.unsigned().body().decision() == Some(TrustDecision::Trusted)
            })
        });
    match trusted {
        Ok(true) => None,
        Ok(false) => Some(ReceiveOutcome::Refused {
            reason: format!(
                "{to} no longer trusts it with {}",
                TrustScope::federation_push()
            ),
        }),
        Err(error) => Some(ReceiveOutcome::Failed {
            reason: error.to_string(),
        }),
    }
}

fn not_accepted() -> ReceiveOutcome {
    ReceiveOutcome::Refused {
        reason: "no [[federation.accept]] entry covers it".to_owned(),
    }
}

/// Log the pull and publish if asked, as `kairo fetch` would.
fn received(
    store: &FilesystemStore,
    object: &ObjectId,
    publish: bool,
    report: PullReport,
) -> ReceiveOutcome {
    let pull_log_error = store
        .record_pull(&PullRecord {
            peer_url: report.peer_url.clone(),
            object: object.clone(),
            peer_actor: report.peer_actor.clone(),
            at: Timestamp::now(),
            fetched: report.fetched as u64,
            already_held: report.already_held as u64,
        })
        .err()
        .map(|error| error.to_string());
    let published = publish && store.publish_object(object, Timestamp::now()).is_ok();
    ReceiveOutcome::Received {
        report: Box::new(report),
        published,
        pull_log_error,
    }
}

fn receive_announcement(
    store: &FilesystemStore,
    config: &FederationConfig,
    announcement: &InboxAnnouncement,
) -> Vec<(ObjectId, ReceiveOutcome)> {
    let from = &announcement.from;
    let options = PullOptions {
        limits: limits_from(&config.limits),
        expected_node_actor: Some(from.clone()),
        require_complete: false,
    };
    announcement
        .objects
        .iter()
        .map(|object| {
            let outcome = if let Some(refusal) = trust_refusal(store, &announcement.to, from) {
                refusal
            } else if let Some(accept) = config.accepts(object, from) {
                match pull_object(store, &announcement.base_url, object, &options) {
                    Ok(report) => received(store, object, accept.publish, report),
                    Err(error) => ReceiveOutcome::Failed {
                        reason: error.to_string(),
                    },
                }
            } else {
                not_accepted()
            };
            (object.clone(), outcome)
        })
        .collect()
}

fn receive_upload(
    store: &FilesystemStore,
    config: &FederationConfig,
    upload: &InboxUpload,
    bundle: &Path,
) -> ReceiveOutcome {
    let (object, from) = (&upload.object, &upload.from);
    if let Some(refusal) = trust_refusal(store, &upload.to, from) {
        return refusal;
    }
    let Some(accept) = config.accepts(object, from) else {
        return not_accepted();
    };
    let unpack_dir = bundle.with_extension("unpacked");
    let _ = std::fs::remove_dir_all(&unpack_dir);
    let unpacked = std::fs::File::open(bundle)
        .map_err(|error| error.to_string())
        .and_then(|file| {
            kairo_bundle::closure::unpack_tar(file, &unpack_dir).map_err(|error| error.to_string())
        });
    let failed = |reason: String| ReceiveOutcome::Failed { reason };
    let outcome = match unpacked {
        Err(error) => failed(error),
        Ok(closure) if closure.object() != object => {
            failed(format!("the bundle is for {}", closure.object()))
        }
        Ok(closure) => {
            let options = PullOptions {
                limits: limits_from(&config.limits),
                expected_node_actor: None,
                require_complete: false,
            };
            match ingest_closure_bundle(store, closure, from, &options) {
                Ok(report) => received(store, object, accept.publish, report),
                Err(error) => failed(error.to_string()),
            }
        }
    };
    let _ = std::fs::remove_dir_all(&unpack_dir);
    outcome
}
