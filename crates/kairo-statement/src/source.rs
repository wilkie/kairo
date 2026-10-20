//! Source objects (`specs/DECISIONS.md` §37): an external Git
//! repository archived faithfully, identified by its canonical URL.
//!
//! A [`SourceGenesis`] holds only that URL. Its object id is derived
//! from it under [`SOURCE_GENESIS_DOMAIN`], so every node mirroring the
//! same repository computes the same id; it has no creator, timestamp,
//! or nonce and is not signed. What actors sign about a source are
//! [`SourceObservationBody`] statements: every ref one fetch saw, with
//! its commit. Canonical forms are documented at
//! `schemas/canonical/source-genesis-v1.md` and
//! `schemas/canonical/source-observation-v1.md`.

use std::error::Error;
use std::fmt;

use kairo_core::canonical::{encode_list, encode_str, encode_u8, CanonicalEncode};
use kairo_core::ObjectId;

use crate::{ChainKey, ChainScope, GitObjectFormat, GitRevision, StatementBody};

/// Domain separator for a source's object id.
pub const SOURCE_GENESIS_DOMAIN: &[u8] = b"kairo.source.genesis.v1";

/// A repository URL in canonical form (§37 sub-decision 1): `https` or
/// `git`; scheme and host lowercased; no userinfo, query, or fragment;
/// a default port dropped; one trailing `/` and a trailing `.git`
/// dropped; the path otherwise as written. `ssh` URLs, including the
/// `user@host:path` form, become `https://host/path`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceUrl(String);

impl SourceUrl {
    pub fn parse(input: &str) -> Result<Self, SourceUrlError> {
        let error = |reason: &'static str| SourceUrlError {
            input: input.to_owned(),
            reason,
        };
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(error("is empty"));
        }
        if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(error("contains whitespace or control characters"));
        }
        // `git@host:path`, the scp-like ssh form.
        let (scheme, rest) = match trimmed.split_once("://") {
            Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest.to_owned()),
            None => match trimmed.split_once(':') {
                Some((authority, path)) if authority.contains('@') && !authority.contains('/') => {
                    ("ssh".to_owned(), format!("{authority}/{path}"))
                }
                _ => return Err(error("has no scheme (expected https://, git://, or ssh)")),
            },
        };
        let (target_scheme, default_port) = match scheme.as_str() {
            "https" => ("https", 443),
            "git" => ("git", 9418),
            "ssh" | "git+ssh" | "ssh+git" => ("https", 22),
            "file" => {
                return Err(error(
                    "is a local path, which identifies nothing on another node",
                ))
            }
            _ => {
                return Err(error(
                    "has an unsupported scheme (expected https, git, or ssh)",
                ))
            }
        };
        if rest.contains('?') || rest.contains('#') {
            return Err(error("has a query or fragment"));
        }
        let (authority, path) = rest.split_once('/').ok_or_else(|| error("has no path"))?;
        // Credentials and user names are never part of an identity.
        let host_port = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        let parse_port = |port: &str| {
            port.parse::<u16>()
                .map_err(|_| error("has an invalid port"))
        };
        let (host, port) = if let Some(bracketed) = host_port.strip_prefix('[') {
            // An IPv6 literal: `[addr]` or `[addr]:port`.
            let (address, after) = bracketed
                .split_once(']')
                .ok_or_else(|| error("has an unterminated IPv6 address"))?;
            let port = match after.strip_prefix(':') {
                Some(port) => Some(parse_port(port)?),
                None if after.is_empty() => None,
                None => return Err(error("has text after its IPv6 address")),
            };
            (format!("[{address}]"), port)
        } else {
            match host_port.rsplit_once(':') {
                Some((host, port)) => (host.to_owned(), Some(parse_port(port)?)),
                None => (host_port.to_owned(), None),
            }
        };
        if host.is_empty() || host == "[]" {
            return Err(error("has no host"));
        }
        let host = host.to_ascii_lowercase();
        let port = match port {
            Some(port) if port == default_port => None,
            // An ssh port says nothing about where https is served.
            Some(_) if target_scheme == "https" && default_port == 22 => {
                return Err(error(
                    "is ssh on a non-default port, which has no https form",
                ))
            }
            port => port,
        };
        let mut path = path.strip_suffix('/').unwrap_or(path);
        path = path.strip_suffix(".git").unwrap_or(path);
        if path.is_empty()
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(error("has an empty or relative path segment"));
        }
        Ok(Self(match port {
            Some(port) => format!("{target_scheme}://{host}:{port}/{path}"),
            None => format!("{target_scheme}://{host}/{path}"),
        }))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Resolve a submodule's URL as Git does: relative (`./x`, `../x`)
    /// against this repository's URL, anything else on its own.
    pub fn join(&self, reference: &str) -> Result<Self, SourceUrlError> {
        if !(reference.starts_with("./") || reference.starts_with("../")) {
            return Self::parse(reference);
        }
        let (origin, path) = self
            .0
            .split_once("://")
            .and_then(|(scheme, rest)| {
                rest.split_once('/')
                    .map(|(authority, path)| (format!("{scheme}://{authority}"), path))
            })
            .ok_or_else(|| SourceUrlError {
                input: reference.to_owned(),
                reason: "can't be resolved",
            })?;
        let mut segments: Vec<&str> = path.split('/').collect();
        for part in reference.split('/') {
            match part {
                "." | "" => {}
                ".." => {
                    if segments.pop().is_none() {
                        return Err(SourceUrlError {
                            input: reference.to_owned(),
                            reason: "climbs above the host",
                        });
                    }
                }
                part => segments.push(part),
            }
        }
        Self::parse(&format!("{origin}/{}", segments.join("/")))
    }
}

impl fmt::Display for SourceUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceUrlError {
    pub input: String,
    pub reason: &'static str,
}

impl fmt::Display for SourceUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "source URL {:?} {}", self.input, self.reason)
    }
}

impl Error for SourceUrlError {}

/// A source object's genesis: the canonical URL and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceGenesis {
    url: SourceUrl,
}

impl SourceGenesis {
    pub fn new(url: SourceUrl) -> Self {
        Self { url }
    }

    pub fn url(&self) -> &SourceUrl {
        &self.url
    }

    /// The same on every node for the same repository.
    pub fn object_id(&self) -> ObjectId {
        ObjectId::from_bytes(SOURCE_GENESIS_DOMAIN, &self.canonical_bytes())
    }
}

impl CanonicalEncode for SourceGenesis {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, "SourceGenesis");
        encode_u8(out, 1);
        encode_str(out, self.url.as_str());
    }
}

/// One ref an observation saw, at the object it names: a commit, or
/// for an annotated tag the tag object itself, so the tagger's message
/// and signature are archived too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedRef {
    name: String,
    target: GitRevision,
}

impl ObservedRef {
    pub fn new(
        name: impl Into<String>,
        target: GitRevision,
    ) -> Result<Self, SourceObservationShapeError> {
        let name = name.into();
        let tail = name
            .strip_prefix("refs/heads/")
            .or_else(|| name.strip_prefix("refs/tags/"));
        let valid = tail.is_some_and(|tail| {
            !tail.is_empty()
                && !tail.contains("..")
                && !tail.ends_with('/')
                && !tail.chars().any(|c| c.is_whitespace() || c.is_control())
        });
        if !valid {
            return Err(SourceObservationShapeError::InvalidRef(name));
        }
        Ok(Self { name, target })
    }

    /// `refs/heads/...` or `refs/tags/...`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The object the ref names.
    pub fn target(&self) -> &GitRevision {
        &self.target
    }
}

/// Canonical SourceObservation body v1 encoding is documented at
/// `schemas/canonical/source-observation-v1.md`.
///
/// What one fetch of a source saw (§37 sub-decision 2): every ref the
/// remote advertised under `refs/heads/` and `refs/tags/`, each at the
/// object it names, sorted by name. Signing it claims the observer fetched that
/// URL and holds each commit's full closure. A ref absent here that an
/// earlier observation listed was deleted upstream; nothing is dropped
/// from the archive because of it (sub-decision 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceObservationBody {
    source: ObjectId,
    url: SourceUrl,
    refs: Vec<ObservedRef>,
    pinned: Vec<GitRevision>,
}

impl SourceObservationBody {
    /// `refs` in any order; sorted here. Names must be unique, and
    /// every commit in one format, since a repository has one.
    pub fn new(
        url: SourceUrl,
        mut refs: Vec<ObservedRef>,
    ) -> Result<Self, SourceObservationShapeError> {
        refs.sort_by(|a, b| a.name.cmp(&b.name));
        if let Some(pair) = refs.windows(2).find(|pair| pair[0].name == pair[1].name) {
            return Err(SourceObservationShapeError::DuplicateRef(
                pair[0].name.clone(),
            ));
        }
        let formats: Vec<GitObjectFormat> = refs.iter().map(|r| r.target.format()).collect();
        if formats.windows(2).any(|pair| pair[0] != pair[1]) {
            return Err(SourceObservationShapeError::MixedFormats);
        }
        Ok(Self {
            source: SourceGenesis::new(url.clone()).object_id(),
            url,
            refs,
            pinned: Vec::new(),
        })
    }

    /// Add the commits fetched by id because another source's commit
    /// pins them as a submodule (§37 sub-decision 5): not refs, but
    /// held with their full closures all the same. Sorted, unique, and
    /// in the refs' format.
    pub fn with_pinned(
        mut self,
        mut pinned: Vec<GitRevision>,
    ) -> Result<Self, SourceObservationShapeError> {
        pinned.sort_by(|a, b| a.oid().cmp(b.oid()));
        pinned.dedup();
        let format = self.format();
        if pinned
            .windows(2)
            .any(|pair| pair[0].format() != pair[1].format())
            || format
                .zip(pinned.first())
                .is_some_and(|(format, first)| first.format() != format)
        {
            return Err(SourceObservationShapeError::MixedFormats);
        }
        self.pinned = pinned;
        Ok(self)
    }

    /// The source object, derived from `url`.
    pub fn source(&self) -> &ObjectId {
        &self.source
    }

    pub fn url(&self) -> &SourceUrl {
        &self.url
    }

    pub fn refs(&self) -> &[ObservedRef] {
        &self.refs
    }

    /// Commits fetched by id because a parent source pins them.
    pub fn pinned(&self) -> &[GitRevision] {
        &self.pinned
    }

    /// The format of every observed object; `None` for an empty
    /// repository.
    pub fn format(&self) -> Option<GitObjectFormat> {
        self.refs
            .first()
            .map(|r| r.target.format())
            .or_else(|| self.pinned.first().map(GitRevision::format))
    }
}

impl StatementBody for SourceObservationBody {
    const TYPE: &'static str = "SourceObservation";
    const VERSION: u8 = 1;
    const CHAINED: bool = true;

    fn chain_key(&self) -> Option<ChainKey> {
        Some(ChainKey::Scope(ChainScope::Object(self.source.clone())))
    }
}

impl CanonicalEncode for SourceObservationBody {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, self.source.as_str());
        encode_str(out, self.url.as_str());
        encode_list(out, &self.refs, |out, observed| {
            encode_str(out, &observed.name);
            encode_str(out, &observed.target.to_string());
        });
        // Tagged and present only when non-empty, so an observation
        // without pinned commits encodes as it did before they existed.
        if !self.pinned.is_empty() {
            encode_str(out, "pinned");
            encode_list(out, &self.pinned, |out, commit| {
                encode_str(out, &commit.to_string());
            });
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceObservationShapeError {
    InvalidRef(String),
    DuplicateRef(String),
    MixedFormats,
    /// The body's `source` isn't the id its `url` derives.
    SourceMismatch,
}

impl fmt::Display for SourceObservationShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRef(name) => write!(
                f,
                "observed ref {name:?} is not a refs/heads/ or refs/tags/ name"
            ),
            Self::DuplicateRef(name) => write!(f, "observed ref {name} is listed twice"),
            Self::MixedFormats => f.write_str("observed commits mix Git object formats"),
            Self::SourceMismatch => {
                f.write_str("observation's source is not the id its URL derives")
            }
        }
    }
}

impl Error for SourceObservationShapeError {}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn canonical(input: &str) -> String {
        SourceUrl::parse(input).expect("parses").to_string()
    }

    #[test]
    fn urls_canonicalize_so_one_repository_has_one_id() {
        let forms = [
            "https://github.com/oriansj/stage0-posix",
            "https://github.com/oriansj/stage0-posix.git",
            "https://github.com/oriansj/stage0-posix/",
            "HTTPS://GitHub.com/oriansj/stage0-posix.git",
            "https://user:secret@github.com:443/oriansj/stage0-posix",
            "ssh://git@github.com/oriansj/stage0-posix.git",
            "git@github.com:oriansj/stage0-posix.git",
            "git+ssh://git@github.com:22/oriansj/stage0-posix",
        ];
        let ids: Vec<ObjectId> = forms
            .iter()
            .map(|form| SourceGenesis::new(SourceUrl::parse(form).expect(form)).object_id())
            .collect();
        assert!(ids.windows(2).all(|pair| pair[0] == pair[1]), "{forms:?}");
        assert_eq!(
            canonical(forms[3]),
            "https://github.com/oriansj/stage0-posix"
        );
        // Path case is kept: hosts may treat it as significant.
        assert_ne!(
            canonical("https://github.com/OrianSJ/x"),
            canonical("https://github.com/oriansj/x")
        );
        assert_eq!(canonical("git://localhost:9418/x.git"), "git://localhost/x");
        assert_eq!(
            canonical("git://127.0.0.1:4000/a/b"),
            "git://127.0.0.1:4000/a/b"
        );
        // https and git are different sources.
        assert_ne!(
            canonical("git://example.org/x"),
            canonical("https://example.org/x")
        );
    }

    #[test]
    fn urls_without_a_portable_identity_are_refused() {
        for (input, reason) in [
            ("file:///home/me/repo", "local path"),
            ("/home/me/repo", "no scheme"),
            ("ftp://example.org/x", "unsupported scheme"),
            ("https://example.org/", "empty or relative"),
            ("https://example.org/a/../b", "empty or relative"),
            ("https://example.org/x?ref=main", "query"),
            ("ssh://git@example.org:2222/x", "non-default port"),
            ("https://example.org", "no path"),
            ("", "empty"),
        ] {
            let error = SourceUrl::parse(input).expect_err(input);
            assert!(error.to_string().contains(reason), "{input}: {error}");
        }
    }

    #[test]
    fn submodule_urls_resolve_like_git() {
        let parent = SourceUrl::parse("https://github.com/oriansj/stage0-posix").expect("url");
        assert_eq!(
            parent.join("../M2libc").expect("relative").as_str(),
            "https://github.com/oriansj/M2libc"
        );
        assert_eq!(
            parent.join("./sub").expect("relative").as_str(),
            "https://github.com/oriansj/stage0-posix/sub"
        );
        assert_eq!(
            parent
                .join("https://git.savannah.gnu.org/git/mes.git")
                .expect("absolute")
                .as_str(),
            "https://git.savannah.gnu.org/git/mes"
        );
        assert!(parent.join("../../../x").is_err());
    }

    #[test]
    fn observations_sort_refs_and_reject_bad_shapes() {
        let url = SourceUrl::parse("https://example.org/x").expect("url");
        let sha1 = |c: char| {
            GitRevision::new(GitObjectFormat::Sha1, &c.to_string().repeat(40)).expect("rev")
        };
        let body = SourceObservationBody::new(
            url.clone(),
            vec![
                ObservedRef::new("refs/tags/v1", sha1('b')).expect("ref"),
                ObservedRef::new("refs/heads/main", sha1('a')).expect("ref"),
            ],
        )
        .expect("body");
        assert_eq!(body.refs()[0].name(), "refs/heads/main");
        assert_eq!(body.source(), &SourceGenesis::new(url.clone()).object_id());
        assert_eq!(body.format(), Some(GitObjectFormat::Sha1));

        for name in [
            "HEAD",
            "refs/remotes/origin/main",
            "refs/heads/",
            "refs/heads/a..b",
            "refs/pull/1/head",
        ] {
            assert!(ObservedRef::new(name, sha1('a')).is_err(), "{name}");
        }
        let duplicate = SourceObservationBody::new(
            url.clone(),
            vec![
                ObservedRef::new("refs/heads/main", sha1('a')).expect("ref"),
                ObservedRef::new("refs/heads/main", sha1('b')).expect("ref"),
            ],
        );
        assert!(matches!(
            duplicate,
            Err(SourceObservationShapeError::DuplicateRef(_))
        ));
        let sha256 = GitRevision::new(GitObjectFormat::Sha256, &"c".repeat(64)).expect("rev");
        let mixed = SourceObservationBody::new(
            url,
            vec![
                ObservedRef::new("refs/heads/main", sha1('a')).expect("ref"),
                ObservedRef::new("refs/heads/next", sha256).expect("ref"),
            ],
        );
        assert!(matches!(
            mixed,
            Err(SourceObservationShapeError::MixedFormats)
        ));
    }

    #[test]
    fn pinned_commits_are_encoded_only_when_present() {
        let url = SourceUrl::parse("https://example.org/x").expect("url");
        let sha1 = |c: char| {
            GitRevision::new(GitObjectFormat::Sha1, &c.to_string().repeat(40)).expect("rev")
        };
        let refs = vec![ObservedRef::new("refs/heads/main", sha1('a')).expect("ref")];
        let plain = SourceObservationBody::new(url.clone(), refs.clone()).expect("body");
        let empty = plain.clone().with_pinned(Vec::new()).expect("empty");
        assert_eq!(plain.canonical_bytes(), empty.canonical_bytes());
        let pinned = plain
            .clone()
            .with_pinned(vec![sha1('c'), sha1('b'), sha1('c')])
            .expect("pinned");
        assert_eq!(pinned.pinned(), [sha1('b'), sha1('c')]);
        assert_ne!(pinned.canonical_bytes(), plain.canonical_bytes());
        let sha256 = GitRevision::new(GitObjectFormat::Sha256, &"d".repeat(64)).expect("rev");
        assert!(plain.with_pinned(vec![sha256.clone()]).is_err());
        // With no refs, the pinned commits give the format.
        let only_pinned = SourceObservationBody::new(url, Vec::new())
            .and_then(|body| body.with_pinned(vec![sha256]))
            .expect("only pinned");
        assert_eq!(only_pinned.format(), Some(GitObjectFormat::Sha256));
    }

    #[test]
    fn every_observed_field_changes_the_canonical_bytes() {
        let url = SourceUrl::parse("https://example.org/x").expect("url");
        let sha1 = |c: char| {
            GitRevision::new(GitObjectFormat::Sha1, &c.to_string().repeat(40)).expect("rev")
        };
        let body = |name: &str, commit: char, url: &SourceUrl| {
            SourceObservationBody::new(
                url.clone(),
                vec![ObservedRef::new(name, sha1(commit)).expect("ref")],
            )
            .expect("body")
            .canonical_bytes()
        };
        let other = SourceUrl::parse("https://example.org/y").expect("url");
        let base = body("refs/heads/main", 'a', &url);
        assert_ne!(base, body("refs/heads/next", 'a', &url));
        assert_ne!(base, body("refs/heads/main", 'b', &url));
        assert_ne!(base, body("refs/heads/main", 'a', &other));
    }
}
