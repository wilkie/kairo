//! Announcements (`specs/DECISIONS.md` §26): a node trusted with
//! `federation:push` tells a receiver that objects have new state at
//! its base URL, and the receiver pulls them from there later.

use kairo_core::ObjectId;
use serde::{Deserialize, Serialize};

use crate::schema::schema_tag;

/// Path of the announce endpoint, under [`crate::API_PREFIX`].
pub const ANNOUNCE_PATH: &str = "/announce";

/// Most objects one announcement may name.
pub const MAX_ANNOUNCED_OBJECTS: usize = 256;

schema_tag!(
    /// `"kairo.fed.announce.v1"`.
    AnnouncementSchema = "kairo.fed.announce.v1"
);

schema_tag!(
    /// `"kairo.fed.announce-accepted.v1"`.
    AnnouncementAcceptedSchema = "kairo.fed.announce-accepted.v1"
);

/// `POST /federation/v1/announce` body, signed with `KairoNode1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Announcement {
    pub schema: AnnouncementSchema,
    /// The announcer's base URL, where the receiver pulls from. The
    /// pull pins the announcer's node actor, so a URL serving any
    /// other node fails there.
    pub base_url: String,
    #[serde(with = "crate::wire_id::many")]
    pub objects: Vec<ObjectId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnouncementError {
    InvalidBaseUrl { value: String },
    NoObjects,
    TooMany { count: usize },
    Duplicate { object: ObjectId },
}

impl std::fmt::Display for AnnouncementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBaseUrl { value } => write!(
                f,
                "base_url {value:?} is not an http(s) origin with no path or trailing slash"
            ),
            Self::NoObjects => f.write_str("an announcement names at least one object"),
            Self::TooMany { count } => write!(
                f,
                "an announcement names at most {MAX_ANNOUNCED_OBJECTS} objects, not {count}"
            ),
            Self::Duplicate { object } => write!(f, "object {object} is named twice"),
        }
    }
}

impl std::error::Error for AnnouncementError {}

impl Announcement {
    pub fn new(base_url: String, objects: Vec<ObjectId>) -> Self {
        Self {
            schema: AnnouncementSchema,
            base_url,
            objects,
        }
    }

    pub fn validate(&self) -> Result<(), AnnouncementError> {
        crate::node::validate_base_url(&self.base_url).map_err(|_| {
            AnnouncementError::InvalidBaseUrl {
                value: self.base_url.clone(),
            }
        })?;
        match self.objects.len() {
            0 => return Err(AnnouncementError::NoObjects),
            count if count > MAX_ANNOUNCED_OBJECTS => {
                return Err(AnnouncementError::TooMany { count })
            }
            _ => {}
        }
        let mut seen = std::collections::BTreeSet::new();
        for object in &self.objects {
            if !seen.insert(object) {
                return Err(AnnouncementError::Duplicate {
                    object: object.clone(),
                });
            }
        }
        Ok(())
    }
}

/// `202 Accepted` body: the announcement is in the receiver's inbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnouncementAccepted {
    pub schema: AnnouncementAcceptedSchema,
    /// The inbox entry's name, for the receiver's logs.
    pub entry: String,
}
