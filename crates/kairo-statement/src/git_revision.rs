//! Git revision identifiers (`specs/DECISIONS.md` §19,
//! `specs/OBJECT_STORE.md` "Revisions").
//!
//! A revision that names a Git commit is spelled
//! `git:<format>:<oid>`, where `<format>` is the repository's object
//! format: `git:sha1:` followed by exactly 40 lowercase hex characters,
//! or `git:sha256:` followed by exactly 64. The algorithm comes from
//! the prefix, never from the digest length, and a `git:` revision
//! that doesn't match one of these shapes is malformed rather than
//! "some other scheme".
//!
//! A Git object id hashes Git's own object serialization, so it is
//! not interchangeable with a Kairo id or a generic SHA-256 digest.

use std::fmt;
use std::str::FromStr;

use crate::RevisionId;

const GIT_SCHEME: &str = "git:";

/// A Git repository's object format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GitObjectFormat {
    Sha1,
    Sha256,
}

impl GitObjectFormat {
    pub const ALL: [Self; 2] = [Self::Sha1, Self::Sha256];

    /// The name `git init --object-format` and revision ids use.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
        }
    }

    /// Length of an object id in hex characters.
    pub fn hex_len(self) -> usize {
        match self {
            Self::Sha1 => 40,
            Self::Sha256 => 64,
        }
    }
}

impl fmt::Display for GitObjectFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for GitObjectFormat {
    type Err = GitRevisionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|format| format.as_str() == value)
            .ok_or_else(|| GitRevisionError::UnknownFormat {
                format: value.to_owned(),
            })
    }
}

/// A parsed `git:<format>:<oid>` revision.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GitRevision {
    format: GitObjectFormat,
    oid: String,
}

impl GitRevision {
    /// Parse `value`. `Ok(None)` when it isn't a `git:` revision at
    /// all (another storage scheme); `Err` when it is one but
    /// malformed.
    pub fn parse(value: &str) -> Result<Option<Self>, GitRevisionError> {
        let Some(rest) = value.strip_prefix(GIT_SCHEME) else {
            return Ok(None);
        };
        let Some((format, oid)) = rest.split_once(':') else {
            return Err(GitRevisionError::MissingFormat {
                value: value.to_owned(),
            });
        };
        let format: GitObjectFormat = format.parse()?;
        Self::new(format, oid).map(Some)
    }

    /// Build from a format and an object id, which must be exactly
    /// `format.hex_len()` lowercase hex characters.
    pub fn new(format: GitObjectFormat, oid: &str) -> Result<Self, GitRevisionError> {
        if oid.len() != format.hex_len() {
            return Err(GitRevisionError::BadLength {
                format,
                expected: format.hex_len(),
                found: oid.len(),
            });
        }
        if !oid
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(GitRevisionError::NotLowercaseHex {
                oid: oid.to_owned(),
            });
        }
        Ok(Self {
            format,
            oid: oid.to_owned(),
        })
    }

    pub fn format(&self) -> GitObjectFormat {
        self.format
    }

    /// The object id, lowercase hex.
    pub fn oid(&self) -> &str {
        &self.oid
    }

    pub fn to_revision_id(&self) -> RevisionId {
        RevisionId::new(self.to_string())
    }
}

impl fmt::Display for GitRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{GIT_SCHEME}{}:{}", self.format, self.oid)
    }
}

impl RevisionId {
    /// This revision as a Git commit, if it uses the `git:` scheme.
    pub fn git(&self) -> Result<Option<GitRevision>, GitRevisionError> {
        GitRevision::parse(self.as_str())
    }
}

/// Check an `ObjectRevision` body's revision ids where a revision
/// enters a store (authoring, the daemon's write path, bundle import,
/// federation pull): every `git:` id among the revision and its
/// parents is well-formed, and the parents use the revision's format,
/// since one Git history has one object format. Ids in other schemes
/// pass untouched.
pub fn check_revision_ids(body: &crate::ObjectRevisionBody) -> Result<(), GitRevisionError> {
    let revision = body.revision().git()?;
    for parent in body.parents() {
        let parent = parent.git()?;
        if let (Some(revision), Some(parent)) = (&revision, &parent) {
            if parent.format() != revision.format() {
                return Err(GitRevisionError::MixedFormats {
                    revision: revision.format(),
                    parent: parent.format(),
                });
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitRevisionError {
    /// `git:<oid>` with no format segment.
    MissingFormat {
        value: String,
    },
    UnknownFormat {
        format: String,
    },
    BadLength {
        format: GitObjectFormat,
        expected: usize,
        found: usize,
    },
    NotLowercaseHex {
        oid: String,
    },
    /// A parent in a different object format than the revision.
    MixedFormats {
        revision: GitObjectFormat,
        parent: GitObjectFormat,
    },
}

impl fmt::Display for GitRevisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFormat { value } => write!(
                f,
                "Git revision {value:?} has no object format; use git:sha1:<oid> or git:sha256:<oid>"
            ),
            Self::UnknownFormat { format } => write!(
                f,
                "unknown Git object format {format:?}; expected sha1 or sha256"
            ),
            Self::BadLength {
                format,
                expected,
                found,
            } => write!(
                f,
                "a git:{format} object id is {expected} hex characters, found {found}"
            ),
            Self::NotLowercaseHex { oid } => {
                write!(f, "Git object id {oid:?} is not lowercase hex")
            }
            Self::MixedFormats { revision, parent } => write!(
                f,
                "a git:{revision} revision cannot have a git:{parent} parent; one Git \
                 history has one object format"
            ),
        }
    }
}

impl std::error::Error for GitRevisionError {}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const SHA1: &str = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
    const SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn parses_both_formats_by_prefix() {
        let sha1 = GitRevision::parse(&format!("git:sha1:{SHA1}"));
        assert_eq!(
            sha1.map(|r| r.map(|r| (r.format(), r.oid().to_owned()))),
            Ok(Some((GitObjectFormat::Sha1, SHA1.to_owned())))
        );
        let sha256 = RevisionId::new(format!("git:sha256:{SHA256}")).git();
        assert_eq!(
            sha256.map(|r| r.map(|r| r.format())),
            Ok(Some(GitObjectFormat::Sha256))
        );
    }

    #[test]
    fn round_trips_through_display() {
        let revision = GitRevision::new(GitObjectFormat::Sha1, SHA1).expect("valid");
        assert_eq!(revision.to_string(), format!("git:sha1:{SHA1}"));
        assert_eq!(revision.to_revision_id().git(), Ok(Some(revision.clone())));
    }

    #[test]
    fn other_schemes_are_not_git() {
        assert_eq!(GitRevision::parse("hg:abc"), Ok(None));
        assert_eq!(GitRevision::parse("revision-1"), Ok(None));
    }

    #[test]
    fn length_never_implies_the_format() {
        // A SHA-1 id under the sha256 label, and the reverse.
        assert!(matches!(
            GitRevision::parse(&format!("git:sha256:{SHA1}")),
            Err(GitRevisionError::BadLength {
                expected: 64,
                found: 40,
                ..
            })
        ));
        assert!(matches!(
            GitRevision::parse(&format!("git:sha1:{SHA256}")),
            Err(GitRevisionError::BadLength {
                expected: 40,
                found: 64,
                ..
            })
        ));
    }

    #[test]
    fn malformed_git_revisions_are_errors() {
        assert!(matches!(
            GitRevision::parse(&format!("git:{SHA1}")),
            Err(GitRevisionError::MissingFormat { .. })
        ));
        assert!(matches!(
            GitRevision::parse("git:md5:abc"),
            Err(GitRevisionError::UnknownFormat { .. })
        ));
        assert!(matches!(
            GitRevision::parse("git:sha1"),
            Err(GitRevisionError::MissingFormat { .. })
        ));
        assert!(matches!(
            GitRevision::parse(&format!("git:sha1:{}", SHA1.to_uppercase())),
            Err(GitRevisionError::NotLowercaseHex { .. })
        ));
        assert!(matches!(
            GitRevision::parse("git:sha256:r1"),
            Err(GitRevisionError::BadLength { .. })
        ));
    }
}
