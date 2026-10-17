//! Pagination: `limit` + opaque `cursor`
//! (`specs/PHASE_2_FEDERATION.md` §2.2).
//!
//! Listings are ordered by id, so a cursor is a position the server
//! encodes and only the server interprets. The client echoes
//! `next_cursor` back verbatim until it is `null`.

use std::fmt;

/// Page size when the request names none.
pub const DEFAULT_PAGE_LIMIT: u32 = 500;

/// Largest page a node serves (`specs/DECISIONS.md` §16
/// sub-decision 5).
pub const MAX_PAGE_LIMIT: u32 = 1000;

/// Longest cursor accepted. Cursors are server-minted position
/// tokens; anything longer is not one of ours.
pub const MAX_CURSOR_LEN: usize = 256;

/// An opaque page position minted by the serving node.
///
/// Non-empty, at most [`MAX_CURSOR_LEN`] bytes of URL-safe ASCII
/// (`A–Z a–z 0–9 - _ . ~`), so it rides in a query string unescaped.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cursor(String);

impl Cursor {
    pub fn new(value: impl Into<String>) -> Result<Self, CursorError> {
        let value = value.into();
        if value.is_empty() {
            return Err(CursorError::Empty);
        }
        if value.len() > MAX_CURSOR_LEN {
            return Err(CursorError::TooLong);
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
        {
            return Err(CursorError::InvalidCharacter);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for Cursor {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Cursor {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = <String as serde::Deserialize>::deserialize(d)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorError {
    Empty,
    TooLong,
    InvalidCharacter,
}

impl fmt::Display for CursorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("cursor is empty"),
            Self::TooLong => write!(f, "cursor exceeds {MAX_CURSOR_LEN} bytes"),
            Self::InvalidCharacter => {
                f.write_str("cursor contains a character outside A-Za-z0-9-_.~")
            }
        }
    }
}

impl std::error::Error for CursorError {}

/// A validated `?limit=&cursor=` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRequest {
    limit: u32,
    cursor: Option<Cursor>,
}

impl PageRequest {
    /// The first page at the default size.
    pub fn first() -> Self {
        Self {
            limit: DEFAULT_PAGE_LIMIT,
            cursor: None,
        }
    }

    /// Parse the raw query parameters. An absent `limit` is
    /// [`DEFAULT_PAGE_LIMIT`]; `0` or anything above
    /// [`MAX_PAGE_LIMIT`] is refused rather than clamped, so a client
    /// never silently gets a smaller page than it asked for.
    pub fn from_query(limit: Option<&str>, cursor: Option<&str>) -> Result<Self, PageRequestError> {
        let limit = match limit {
            None => DEFAULT_PAGE_LIMIT,
            Some(raw) => match raw.parse::<u32>() {
                Ok(value) if (1..=MAX_PAGE_LIMIT).contains(&value) => value,
                _ => return Err(PageRequestError::BadLimit),
            },
        };
        let cursor = cursor
            .map(Cursor::new)
            .transpose()
            .map_err(PageRequestError::BadCursor)?;
        Ok(Self { limit, cursor })
    }

    /// The same page size, continuing from `cursor`.
    pub fn next(&self, cursor: Cursor) -> Self {
        Self {
            limit: self.limit,
            cursor: Some(cursor),
        }
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }

    pub fn cursor(&self) -> Option<&Cursor> {
        self.cursor.as_ref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageRequestError {
    BadLimit,
    BadCursor(CursorError),
}

impl fmt::Display for PageRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadLimit => write!(f, "limit must be an integer from 1 to {MAX_PAGE_LIMIT}"),
            Self::BadCursor(error) => write!(f, "invalid cursor: {error}"),
        }
    }
}

impl std::error::Error for PageRequestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BadLimit => None,
            Self::BadCursor(error) => Some(error),
        }
    }
}
