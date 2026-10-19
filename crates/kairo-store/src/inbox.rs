//! The federation inbox (`specs/DECISIONS.md` §26 sub-decision 3):
//! pushes a node accepted and hasn't ingested yet.
//!
//! `<root>/federation/inbox/` holds one file per entry. The federation
//! server writes entries; `kairo federation receive` reads and removes
//! them. Writing an entry is the server's one write to a store, and
//! nothing in the inbox is trusted: ingest runs every record through
//! the pull verifier.
//!
//! An announcement is `announce-<received_at>-<from>-<token>.json`:
//!
//! ```json
//! {
//!   "from": "<node actor id>",
//!   "to": "<receiving node actor id>",
//!   "base_url": "https://laptop.example.org",
//!   "objects": ["<object id>"],
//!   "received_at": "<RFC 3339>"
//! }
//! ```
//!
//! An upload (§26 sub-decision 6) is `push-<received_at>-<from>-<token>.json`,
//! with `"object"` in place of `"base_url"` and `"objects"`, and its
//! closure bundle beside it as the same name with `.tar`. The body
//! streams into `.incoming-<token>.tar` first and is renamed into place
//! only once the request has verified.
//!
//! The sender and seconds are in the name, so entries list in arrival
//! order and the per-node bound counts names without parsing files.
//! Like the pull log, this is primary state that `rebuild_indexes()`
//! leaves alone.

use std::path::{Path, PathBuf};

use kairo_core::{ActorId, ObjectId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, StoreError};
use crate::lock::with_index_lock;
use crate::{atomic_write, json_to_corrupt, read_or_missing, FilesystemStore};

const INBOX_DIR: &str = "inbox";
const ANNOUNCE_PREFIX: &str = "announce-";
const PUSH_PREFIX: &str = "push-";
const BUNDLE_SUFFIX: &str = ".tar";
const ENTRY_SUFFIX: &str = ".json";

/// An accepted announcement waiting to be pulled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxAnnouncement {
    /// The authenticated announcing node.
    pub from: ActorId,
    /// The node actor that accepted it, whose `federation:push` trust
    /// is re-checked at ingest.
    pub to: ActorId,
    /// Where to pull from, as the announcer gave it.
    pub base_url: String,
    pub objects: Vec<ObjectId>,
    pub received_at: Timestamp,
}

/// An accepted upload waiting to be ingested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxUpload {
    /// The authenticated pushing node.
    pub from: ActorId,
    pub to: ActorId,
    pub object: ObjectId,
    pub received_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboxItem {
    Announcement(InboxAnnouncement),
    /// With the path of its closure bundle tar.
    Upload(InboxUpload, PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxEntry {
    /// The file name, which identifies the entry.
    pub name: String,
    pub item: InboxItem,
}

/// What [`InboxStore::put_inbox_announcement`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboxPut {
    Stored {
        name: String,
    },
    /// The sender already has this many unprocessed entries.
    Full {
        held: u64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadFile {
    from: String,
    to: String,
    object: String,
    received_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnouncementFile {
    from: String,
    to: String,
    base_url: String,
    objects: Vec<String>,
    received_at: String,
}

pub trait InboxStore {
    /// Store `announcement` unless its sender already holds
    /// `max_per_node` entries. `token` makes the name unique (the
    /// request's nonce, in hex); it must be lowercase hex.
    fn put_inbox_announcement(
        &self,
        announcement: &InboxAnnouncement,
        token: &str,
        max_per_node: u64,
    ) -> Result<InboxPut, StoreError>;

    /// Every entry, oldest first. A file that doesn't parse is
    /// `Corrupt`.
    fn list_inbox(&self) -> Result<Vec<InboxEntry>, StoreError>;

    /// Remove a processed entry, and an upload's bundle with it.
    /// Removing one already gone is fine.
    fn remove_inbox_entry(&self, name: &str) -> Result<(), StoreError>;

    /// Unprocessed entries from `node`.
    fn inbox_held(&self, node: &ActorId) -> Result<u64, StoreError>;

    /// Where an upload's body streams to before it's accepted. Delete
    /// it if the request fails.
    fn inbox_incoming_path(&self, token: &str) -> Result<PathBuf, StoreError>;

    /// Accept the upload streamed to `incoming`, unless its sender
    /// already holds `max_per_node` entries (then `incoming` is
    /// deleted). `token` is as for announcements.
    fn put_inbox_upload(
        &self,
        upload: &InboxUpload,
        incoming: &Path,
        token: &str,
        max_per_node: u64,
    ) -> Result<InboxPut, StoreError>;
}

impl FilesystemStore {
    fn inbox_dir(&self) -> PathBuf {
        self.root.join("federation").join(INBOX_DIR)
    }

    fn inbox_names(&self) -> Result<Vec<String>, StoreError> {
        let entries = match std::fs::read_dir(self.inbox_dir()) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(StoreError::Unavailable(error)),
        };
        let mut names = Vec::new();
        for entry in entries {
            let name = entry.map_err(StoreError::Unavailable)?.file_name();
            // Temporary files from an interrupted write, and the lock,
            // aren't entries.
            if let Some(name) = name.to_str() {
                if (name.starts_with(ANNOUNCE_PREFIX) || name.starts_with(PUSH_PREFIX))
                    && name.ends_with(ENTRY_SUFFIX)
                {
                    names.push(name.to_owned());
                }
            }
        }
        // By arrival across kinds, then by name.
        names.sort_by(|a, b| (seconds_of(a), a).cmp(&(seconds_of(b), b)));
        Ok(names)
    }
}

fn corrupt(name: &str, error: &dyn std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        id: format!("federation/inbox/{name}"),
        reason: CorruptReason::Parse(error.to_string()),
    }
}

fn sender_of(name: &str) -> Option<&str> {
    // <kind>-<20-digit seconds>-<from>-<token>.json
    let rest = name
        .strip_prefix(ANNOUNCE_PREFIX)
        .or_else(|| name.strip_prefix(PUSH_PREFIX))?;
    rest.split('-').nth(1)
}

fn seconds_of(name: &str) -> Option<&str> {
    let rest = name
        .strip_prefix(ANNOUNCE_PREFIX)
        .or_else(|| name.strip_prefix(PUSH_PREFIX))?;
    rest.split('-').next()
}

fn check_token(token: &str) -> Result<(), StoreError> {
    if token.is_empty()
        || !token
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(StoreError::Rejected {
            reason: format!("inbox token {token:?} is not lowercase hex"),
        });
    }
    Ok(())
}

fn entry_name(prefix: &str, at: Timestamp, from: &ActorId, token: &str) -> String {
    // Seconds since the epoch, zero-padded so names sort by arrival; a
    // pre-1970 clock sorts first.
    let seconds = at.seconds().max(0);
    format!("{prefix}{seconds:020}-{from}-{token}{ENTRY_SUFFIX}")
}

fn bundle_name(entry: &str) -> String {
    format!("{}{BUNDLE_SUFFIX}", entry.trim_end_matches(ENTRY_SUFFIX))
}

impl InboxStore for FilesystemStore {
    fn put_inbox_announcement(
        &self,
        announcement: &InboxAnnouncement,
        token: &str,
        max_per_node: u64,
    ) -> Result<InboxPut, StoreError> {
        check_token(token)?;
        let dir = self.inbox_dir();
        with_index_lock(&dir, || {
            let from = announcement.from.as_str();
            let held = self.inbox_held(&announcement.from)?;
            if held >= max_per_node {
                return Ok(InboxPut::Full { held });
            }
            let name = entry_name(
                ANNOUNCE_PREFIX,
                announcement.received_at,
                &announcement.from,
                token,
            );
            let file = AnnouncementFile {
                from: from.to_owned(),
                to: announcement.to.to_string(),
                base_url: announcement.base_url.clone(),
                objects: announcement
                    .objects
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                received_at: announcement.received_at.to_string(),
            };
            let mut bytes = serde_json::to_vec_pretty(&file).map_err(json_to_corrupt(&name))?;
            bytes.push(b'\n');
            atomic_write(&dir.join(&name), &bytes)?;
            Ok(InboxPut::Stored { name })
        })
    }

    fn list_inbox(&self) -> Result<Vec<InboxEntry>, StoreError> {
        let dir = self.inbox_dir();
        let mut entries = Vec::new();
        for name in self.inbox_names()? {
            let bytes = match read_or_missing(&dir.join(&name)) {
                Ok(bytes) => bytes,
                // Removed by a concurrent receive.
                Err(StoreError::Missing) => continue,
                Err(error) => return Err(error),
            };
            if name.starts_with(PUSH_PREFIX) {
                let file: UploadFile =
                    serde_json::from_slice(&bytes).map_err(|e| corrupt(&name, &e))?;
                let upload = InboxUpload {
                    from: ActorId::new(&file.from).map_err(|e| corrupt(&name, &e))?,
                    to: ActorId::new(&file.to).map_err(|e| corrupt(&name, &e))?,
                    object: ObjectId::new(&file.object).map_err(|e| corrupt(&name, &e))?,
                    received_at: file.received_at.parse().map_err(|e| corrupt(&name, &e))?,
                };
                let bundle = dir.join(bundle_name(&name));
                entries.push(InboxEntry {
                    name,
                    item: InboxItem::Upload(upload, bundle),
                });
                continue;
            }
            let file: AnnouncementFile =
                serde_json::from_slice(&bytes).map_err(|e| corrupt(&name, &e))?;
            let objects = file
                .objects
                .iter()
                .map(ObjectId::new)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| corrupt(&name, &e))?;
            let announcement = InboxAnnouncement {
                from: ActorId::new(&file.from).map_err(|e| corrupt(&name, &e))?,
                to: ActorId::new(&file.to).map_err(|e| corrupt(&name, &e))?,
                base_url: file.base_url,
                objects,
                received_at: file.received_at.parse().map_err(|e| corrupt(&name, &e))?,
            };
            entries.push(InboxEntry {
                name,
                item: InboxItem::Announcement(announcement),
            });
        }
        Ok(entries)
    }

    fn remove_inbox_entry(&self, name: &str) -> Result<(), StoreError> {
        if name.contains(['/', '\\']) || !name.ends_with(ENTRY_SUFFIX) {
            return Err(StoreError::Rejected {
                reason: format!("{name:?} is not an inbox entry name"),
            });
        }
        let dir = self.inbox_dir();
        // The record first: a bundle without one is ignored, while a
        // record without its bundle would fail to ingest.
        remove_if_present(&dir.join(name))?;
        if name.starts_with(PUSH_PREFIX) {
            remove_if_present(&dir.join(bundle_name(name)))?;
        }
        Ok(())
    }

    fn inbox_held(&self, node: &ActorId) -> Result<u64, StoreError> {
        Ok(self
            .inbox_names()?
            .iter()
            .filter(|name| sender_of(name) == Some(node.as_str()))
            .count() as u64)
    }

    fn inbox_incoming_path(&self, token: &str) -> Result<PathBuf, StoreError> {
        check_token(token)?;
        let dir = self.inbox_dir();
        std::fs::create_dir_all(&dir).map_err(StoreError::Unavailable)?;
        Ok(dir.join(format!(".incoming-{token}{BUNDLE_SUFFIX}")))
    }

    fn put_inbox_upload(
        &self,
        upload: &InboxUpload,
        incoming: &Path,
        token: &str,
        max_per_node: u64,
    ) -> Result<InboxPut, StoreError> {
        check_token(token)?;
        let dir = self.inbox_dir();
        with_index_lock(&dir, || {
            let held = self.inbox_held(&upload.from)?;
            if held >= max_per_node {
                remove_if_present(incoming)?;
                return Ok(InboxPut::Full { held });
            }
            let name = entry_name(PUSH_PREFIX, upload.received_at, &upload.from, token);
            std::fs::rename(incoming, dir.join(bundle_name(&name)))
                .map_err(StoreError::Unavailable)?;
            let file = UploadFile {
                from: upload.from.to_string(),
                to: upload.to.to_string(),
                object: upload.object.to_string(),
                received_at: upload.received_at.to_string(),
            };
            let mut bytes = serde_json::to_vec_pretty(&file).map_err(json_to_corrupt(&name))?;
            bytes.push(b'\n');
            atomic_write(&dir.join(&name), &bytes)?;
            Ok(InboxPut::Stored { name })
        })
    }
}

fn remove_if_present(path: &Path) -> Result<(), StoreError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StoreError::Unavailable(error)),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn announcement(from: u8, at: i64) -> InboxAnnouncement {
        InboxAnnouncement {
            from: ActorId::from_sha256_digest([from; 32]),
            to: ActorId::from_sha256_digest([0xee; 32]),
            base_url: "https://laptop.example.org".to_owned(),
            objects: vec![ObjectId::from_sha256_digest([1; 32])],
            received_at: Timestamp::from_seconds(at),
        }
    }

    #[test]
    fn entries_round_trip_in_arrival_order_and_are_removed() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let store = FilesystemStore::open(dir.path()).expect("store");
        assert!(store.list_inbox().expect("empty").is_empty());

        let later = store
            .put_inbox_announcement(&announcement(1, 2_000), "bb", 8)
            .expect("put");
        let earlier = store
            .put_inbox_announcement(&announcement(2, 1_000), "aa", 8)
            .expect("put");
        let entries = store.list_inbox().expect("list");
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].item,
            InboxItem::Announcement(announcement(2, 1_000))
        );
        assert_eq!(
            InboxPut::Stored {
                name: entries[0].name.clone()
            },
            earlier
        );
        assert_eq!(
            InboxPut::Stored {
                name: entries[1].name.clone()
            },
            later
        );

        store.remove_inbox_entry(&entries[0].name).expect("remove");
        store
            .remove_inbox_entry(&entries[0].name)
            .expect("already gone");
        assert_eq!(store.list_inbox().expect("list").len(), 1);
        assert!(store.remove_inbox_entry("../config.toml").is_err());
    }

    #[test]
    fn uploads_are_renamed_into_place_and_share_the_bound() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let store = FilesystemStore::open(dir.path()).expect("store");
        let from = ActorId::from_sha256_digest([1; 32]);
        let upload = InboxUpload {
            from: from.clone(),
            to: ActorId::from_sha256_digest([0xee; 32]),
            object: ObjectId::from_sha256_digest([2; 32]),
            received_at: Timestamp::from_seconds(1_000),
        };
        let incoming = store.inbox_incoming_path("0a").expect("incoming");
        std::fs::write(&incoming, b"TAR").expect("write");
        // Not an entry until accepted.
        assert!(store.list_inbox().expect("list").is_empty());
        let put = store
            .put_inbox_upload(&upload, &incoming, "0a", 2)
            .expect("put");
        assert!(matches!(put, InboxPut::Stored { .. }));
        assert!(!incoming.exists());
        store
            .put_inbox_announcement(&announcement(1, 1_001), "0b", 2)
            .expect("announce");
        assert_eq!(store.inbox_held(&from).expect("held"), 2);

        let full = store.inbox_incoming_path("0c").expect("incoming");
        std::fs::write(&full, b"TAR").expect("write");
        assert_eq!(
            store
                .put_inbox_upload(&upload, &full, "0c", 2)
                .expect("put"),
            InboxPut::Full { held: 2 }
        );
        assert!(!full.exists(), "a refused upload's body is deleted");

        let entries = store.list_inbox().expect("list");
        let first = &entries[0].item;
        assert!(
            matches!(first, InboxItem::Upload(listed, _) if listed == &upload),
            "{entries:?}"
        );
        let bundle = dir
            .path()
            .join("federation/inbox")
            .join(entries[0].name.replace(".json", ".tar"));
        assert_eq!(std::fs::read(&bundle).expect("bundle"), b"TAR");
        store.remove_inbox_entry(&entries[0].name).expect("remove");
        assert!(!bundle.exists());
    }

    #[test]
    fn each_sender_is_bounded_separately() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let store = FilesystemStore::open(dir.path()).expect("store");
        for token in ["01", "02"] {
            let put = store
                .put_inbox_announcement(&announcement(1, 1_000), token, 2)
                .expect("put");
            assert!(matches!(put, InboxPut::Stored { .. }));
        }
        assert_eq!(
            store
                .put_inbox_announcement(&announcement(1, 1_000), "03", 2)
                .expect("put"),
            InboxPut::Full { held: 2 }
        );
        let other = store
            .put_inbox_announcement(&announcement(2, 1_000), "03", 2)
            .expect("put");
        assert!(matches!(other, InboxPut::Stored { .. }));
        assert!(store
            .put_inbox_announcement(&announcement(3, 1_000), "Not-Hex", 2)
            .is_err());
    }
}
