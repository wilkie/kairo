//! File sources (`specs/DECISIONS.md` §38 sub-decisions 1–2): a single
//! external file, a distfile or release tarball, identified by the
//! SHA-256 of its bytes.
//!
//! A [`FileSourceGenesis`] holds only that hash. Its object id is
//! derived from it under [`FILE_SOURCE_GENESIS_DOMAIN`], so every node
//! holding the same bytes computes the same id however it got them; it
//! is not signed. The bytes are a blob whose id is the plain SHA-256
//! multihash of the file ([`Sha256Digest::blob_id`]). What actors sign
//! are [`FileObservationBody`] statements: "fetching this URL, I got
//! bytes hashing to this". Canonical forms are documented at
//! `schemas/canonical/file-source-genesis-v1.md` and
//! `schemas/canonical/file-observation-v1.md`.

use std::error::Error;
use std::fmt;

use kairo_core::canonical::{encode_str, encode_u64, encode_u8, CanonicalEncode};
use kairo_core::{BlobId, ObjectId};

use crate::{ChainKey, ChainScope, StatementBody};

/// Domain separator for a file source's object id.
pub const FILE_SOURCE_GENESIS_DOMAIN: &[u8] = b"kairo.source.file.genesis.v1";

/// A SHA-256 digest, spelled as 64 lowercase hex digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    pub fn from_bytes(digest: [u8; 32]) -> Self {
        Self(digest)
    }

    /// Exactly 64 lowercase hex digits; anything else is refused, so
    /// one digest has one spelling.
    pub fn parse(input: &str) -> Result<Self, Sha256DigestError> {
        let bytes = input.as_bytes();
        if bytes.len() != 64 {
            return Err(Sha256DigestError(input.to_owned()));
        }
        let nibble = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        };
        let mut digest = [0u8; 32];
        for (i, pair) in bytes.chunks(2).enumerate() {
            let (Some(high), Some(low)) = (nibble(pair[0]), nibble(pair[1])) else {
                return Err(Sha256DigestError(input.to_owned()));
            };
            digest[i] = high << 4 | low;
        }
        Ok(Self(digest))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The blob holding the file: its id is the plain SHA-256 multihash
    /// of the bytes, with no domain, so it is the file's own hash.
    pub fn blob_id(&self) -> BlobId {
        BlobId::from_sha256_digest(self.0)
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sha256DigestError(pub String);

impl fmt::Display for Sha256DigestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SHA-256 digest {:?} is not 64 lowercase hex digits",
            self.0
        )
    }
}

impl Error for Sha256DigestError {}

/// A file source's genesis: the hash and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSourceGenesis {
    sha256: Sha256Digest,
}

impl FileSourceGenesis {
    pub fn new(sha256: Sha256Digest) -> Self {
        Self { sha256 }
    }

    pub fn sha256(&self) -> &Sha256Digest {
        &self.sha256
    }

    /// The same on every node for the same bytes.
    pub fn object_id(&self) -> ObjectId {
        ObjectId::from_bytes(FILE_SOURCE_GENESIS_DOMAIN, &self.canonical_bytes())
    }
}

impl CanonicalEncode for FileSourceGenesis {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, "FileSourceGenesis");
        encode_u8(out, 1);
        encode_str(out, &self.sha256.to_string());
    }
}

/// Where a file was fetched from (§38 sub-decision 2): `https`, or
/// `http` only for a loopback host (tests, a mirror on the same
/// machine); scheme and host lowercased and a default port dropped; no
/// userinfo or fragment. Path and query are kept as written: unlike a
/// Git source's URL this is not an identity, only what was fetched.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileUrl(String);

impl FileUrl {
    pub fn parse(input: &str) -> Result<Self, FileUrlError> {
        let error = |reason: &'static str| FileUrlError {
            input: input.to_owned(),
            reason,
        };
        if input.is_empty() {
            return Err(error("is empty"));
        }
        if input.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(error("contains whitespace or control characters"));
        }
        let (scheme, rest) = input
            .split_once("://")
            .ok_or_else(|| error("has no scheme (expected https://)"))?;
        let scheme = scheme.to_ascii_lowercase();
        let default_port = match scheme.as_str() {
            "https" => 443,
            "http" => 80,
            _ => return Err(error("has an unsupported scheme (expected https)")),
        };
        if rest.contains('#') {
            return Err(error("has a fragment"));
        }
        let split = rest.find(['/', '?']).unwrap_or(rest.len());
        let (authority, path) = rest.split_at(split);
        if authority.contains('@') {
            return Err(error("has credentials"));
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !port.contains(']') => (
                host,
                Some(
                    port.parse::<u16>()
                        .map_err(|_| error("has an invalid port"))?,
                ),
            ),
            _ => (authority, None),
        };
        if host.is_empty() {
            return Err(error("has no host"));
        }
        let host = host.to_ascii_lowercase();
        if scheme == "http" && !is_loopback_host(&host) {
            return Err(error("is plain http, allowed only for loopback hosts"));
        }
        if !path.starts_with('/') || path.len() < 2 {
            return Err(error("has no path"));
        }
        Ok(Self(match port {
            Some(port) if port != default_port => format!("{scheme}://{host}:{port}{path}"),
            _ => format!("{scheme}://{host}{path}"),
        }))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The last path segment, without the query: the file name the URL
    /// suggests.
    pub fn file_name(&self) -> Option<&str> {
        let path = self.0.split('?').next().unwrap_or_default();
        path.rsplit('/').next().filter(|name| !name.is_empty())
    }
}

fn is_loopback_host(host: &str) -> bool {
    if host == "localhost" {
        return true;
    }
    let bare = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);
    bare.parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

impl fmt::Display for FileUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileUrlError {
    pub input: String,
    pub reason: &'static str,
}

impl fmt::Display for FileUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "file URL {:?} {}", self.input, self.reason)
    }
}

impl Error for FileUrlError {}

/// Canonical FileObservation body v1 encoding is documented at
/// `schemas/canonical/file-observation-v1.md`.
///
/// What one download saw (§38 sub-decision 1): fetching `url`, the
/// observer got `size` bytes hashing to `sha256`. Signing it claims the
/// observer holds those bytes. Mirrors serving one file are several
/// observations of one source; a URL that later serves other bytes is
/// an observation of another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileObservationBody {
    source: ObjectId,
    url: FileUrl,
    sha256: Sha256Digest,
    size: u64,
}

impl FileObservationBody {
    pub fn new(url: FileUrl, sha256: Sha256Digest, size: u64) -> Self {
        Self {
            source: FileSourceGenesis::new(sha256).object_id(),
            url,
            sha256,
            size,
        }
    }

    /// The file source, derived from `sha256`.
    pub fn source(&self) -> &ObjectId {
        &self.source
    }

    pub fn url(&self) -> &FileUrl {
        &self.url
    }

    pub fn sha256(&self) -> &Sha256Digest {
        &self.sha256
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}

impl StatementBody for FileObservationBody {
    const TYPE: &'static str = "FileObservation";
    const VERSION: u8 = 1;
    const CHAINED: bool = true;

    fn chain_key(&self) -> Option<ChainKey> {
        Some(ChainKey::Scope(ChainScope::Object(self.source.clone())))
    }
}

impl CanonicalEncode for FileObservationBody {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, self.source.as_str());
        encode_str(out, self.url.as_str());
        encode_str(out, &self.sha256.to_string());
        encode_u64(out, self.size);
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const MES: &str = "183a40ea47ea49f8a1e3bd1b9d12e676374d64d63bc79e7bc1ae7d673dfdf25d";

    #[test]
    fn digests_have_one_spelling() {
        let digest = Sha256Digest::parse(MES).expect("digest");
        assert_eq!(digest.to_string(), MES);
        for bad in [
            &MES.to_ascii_uppercase(),
            &MES[1..],
            &format!("{MES}0"),
            &format!("g{}", &MES[1..]),
            "",
        ] {
            assert!(Sha256Digest::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_file_source_is_its_hash() {
        let digest = Sha256Digest::parse(MES).expect("digest");
        let genesis = FileSourceGenesis::new(digest);
        assert_eq!(
            genesis.object_id(),
            FileSourceGenesis::new(digest).object_id()
        );
        let other = Sha256Digest::parse(&"0".repeat(64)).expect("digest");
        assert_ne!(
            genesis.object_id(),
            FileSourceGenesis::new(other).object_id()
        );
        // The blob is the plain hash of the bytes; the object id is not.
        assert_eq!(
            Sha256Digest::from_bytes([7; 32]).blob_id(),
            BlobId::from_sha256_digest([7; 32])
        );
        assert_ne!(genesis.object_id().as_str(), digest.blob_id().as_str());
        // Neither a Git source's nor a native object's domain.
        assert_ne!(FILE_SOURCE_GENESIS_DOMAIN, crate::SOURCE_GENESIS_DOMAIN);
    }

    #[test]
    fn file_urls_normalize_scheme_host_and_port_only() {
        let url = |input: &str| FileUrl::parse(input).expect(input).to_string();
        assert_eq!(
            url("HTTPS://Mirrors.Kernel.org:443/gnu/mes/mes-0.27.1.tar.gz"),
            "https://mirrors.kernel.org/gnu/mes/mes-0.27.1.tar.gz"
        );
        assert_eq!(
            url("https://example.org/a/B.tar.gz?h=v1&x"),
            "https://example.org/a/B.tar.gz?h=v1&x"
        );
        assert_eq!(
            url("https://example.org:8443/x"),
            "https://example.org:8443/x"
        );
        assert_eq!(url("http://127.0.0.1:4000/x"), "http://127.0.0.1:4000/x");
        assert_eq!(url("http://[::1]/x"), "http://[::1]/x");
        assert_eq!(
            FileUrl::parse("https://example.org/a/b.tar.gz?x=1")
                .expect("url")
                .file_name(),
            Some("b.tar.gz")
        );
        for (input, reason) in [
            ("http://example.org/x", "loopback"),
            ("ftp://example.org/x", "unsupported scheme"),
            ("https://user:pw@example.org/x", "credentials"),
            ("https://example.org/x#frag", "fragment"),
            ("https://example.org", "no path"),
            ("https://example.org/", "no path"),
            ("https:///x", "no host"),
            ("example.org/x", "no scheme"),
        ] {
            let error = FileUrl::parse(input).expect_err(input);
            assert!(error.to_string().contains(reason), "{input}: {error}");
        }
    }

    #[test]
    fn every_observed_field_changes_the_canonical_bytes() {
        let digest = Sha256Digest::parse(MES).expect("digest");
        let url = FileUrl::parse("https://example.org/mes.tar.gz").expect("url");
        let base = FileObservationBody::new(url.clone(), digest, 10);
        assert_eq!(base.source(), &FileSourceGenesis::new(digest).object_id());
        let other_url = FileUrl::parse("https://example.org/other.tar.gz").expect("url");
        let other_digest = Sha256Digest::parse(&"1".repeat(64)).expect("digest");
        for changed in [
            FileObservationBody::new(other_url, digest, 10),
            FileObservationBody::new(url.clone(), other_digest, 10),
            FileObservationBody::new(url, digest, 11),
        ] {
            assert_ne!(changed.canonical_bytes(), base.canonical_bytes());
        }
    }
}
