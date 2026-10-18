//! `KairoNode1` request signatures (`specs/DECISIONS.md` §25
//! sub-decision 3): how a node signs a request to an endpoint that
//! acts on who is asking, and the header that carries it.
//!
//! ```text
//! Authorization: KairoNode1 node=<actor-id>, key_id=<key-id>,
//!   timestamp=<i64>, nonce=<32-hex>, signature=<base64>
//!
//! signing string:
//!   kairo-federation-request-v1\n
//!   <METHOD>\n
//!   <receiver base URL>\n
//!   <path-and-query>\n
//!   <sha256-hex of body>\n
//!   <timestamp>\n
//!   <nonce-hex>\n
//! ```
//!
//! Modeled on the daemon's `KairoActor1` (§14). The receiver's
//! normalized base URL is signed, so a request signed for one node
//! can't be replayed at another. This module is pure: the receiver's
//! key lookup, window, and nonce cache live with the server.

use std::fmt;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use kairo_core::ActorId;
use kairo_identity::{KeyId, SecretSigningKey};
use sha2::{Digest, Sha256};

/// First line of every signing string.
pub const NODE_REQUEST_DOMAIN: &str = "kairo-federation-request-v1";

/// Scheme tag on the `Authorization` header.
pub const NODE_SCHEME_TAG: &str = "KairoNode1";

/// A request's `timestamp` may be this many seconds either side of
/// the receiver's clock, allowing for skew across the internet.
pub const NODE_REQUEST_WINDOW_SECONDS: i64 = 300;

/// Per-request nonce length: 16 random bytes, 32 hex characters.
pub const NODE_NONCE_LEN: usize = 16;

const SIGNATURE_LEN: usize = 64;

/// The exact bytes both sides sign and verify. `receiver_base_url` is
/// the receiver's normalized base URL (its node document's
/// `base_url`); `path_and_query` is the request target as sent.
pub fn node_request_signing_string(
    method: &str,
    receiver_base_url: &str,
    path_and_query: &str,
    body: &[u8],
    timestamp: i64,
    nonce: &[u8; NODE_NONCE_LEN],
) -> String {
    let lines = [
        NODE_REQUEST_DOMAIN,
        method,
        receiver_base_url,
        path_and_query,
        &encode_hex(&Sha256::digest(body)),
        &timestamp.to_string(),
        &encode_hex(nonce),
    ];
    let mut out = String::new();
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// A fresh nonce from the OS random source.
pub fn random_node_nonce() -> Result<[u8; NODE_NONCE_LEN], getrandom::Error> {
    let mut nonce = [0; NODE_NONCE_LEN];
    getrandom::getrandom(&mut nonce)?;
    Ok(nonce)
}

/// What a request a node signs is made of.
#[derive(Debug, Clone, Copy)]
pub struct NodeRequest<'a> {
    pub method: &'a str,
    pub receiver_base_url: &'a str,
    pub path_and_query: &'a str,
    pub body: &'a [u8],
}

impl NodeRequest<'_> {
    pub fn signing_string(&self, timestamp: i64, nonce: &[u8; NODE_NONCE_LEN]) -> String {
        node_request_signing_string(
            self.method,
            self.receiver_base_url,
            self.path_and_query,
            self.body,
            timestamp,
            nonce,
        )
    }
}

/// A parsed or built `Authorization: KairoNode1 ...` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeAuthorization {
    pub node: ActorId,
    pub key_id: KeyId,
    pub timestamp: i64,
    pub nonce: [u8; NODE_NONCE_LEN],
    pub signature: [u8; SIGNATURE_LEN],
}

impl NodeAuthorization {
    /// Sign `request` as `node` with `key`, its active operational key.
    pub fn sign(
        request: &NodeRequest<'_>,
        node: ActorId,
        key: &SecretSigningKey,
        timestamp: i64,
        nonce: [u8; NODE_NONCE_LEN],
    ) -> Self {
        let signing = request.signing_string(timestamp, &nonce);
        Self {
            node,
            key_id: key.public_key().key_id(),
            timestamp,
            nonce,
            signature: *key.sign(signing.as_bytes()).bytes(),
        }
    }

    /// The header value, without `Authorization: `.
    pub fn format(&self) -> String {
        format!(
            "{NODE_SCHEME_TAG} node={}, key_id={}, timestamp={}, nonce={}, signature={}",
            self.node,
            self.key_id,
            self.timestamp,
            encode_hex(&self.nonce),
            STANDARD.encode(self.signature),
        )
    }

    /// Parse a header value. Every parameter is required once; an
    /// unknown one is refused.
    pub fn parse(value: &str) -> Result<Self, NodeAuthHeaderError> {
        let malformed = |reason: String| NodeAuthHeaderError::Malformed(reason);
        let (scheme, params) = value
            .trim()
            .split_once(' ')
            .ok_or_else(|| malformed("no parameters after the scheme".to_owned()))?;
        if scheme != NODE_SCHEME_TAG {
            return Err(NodeAuthHeaderError::UnsupportedScheme(scheme.to_owned()));
        }
        const NAMES: [&str; 5] = ["node", "key_id", "timestamp", "nonce", "signature"];
        let mut values: [Option<&str>; 5] = [None; 5];
        for param in params.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (name, value) = param
                .split_once('=')
                .ok_or_else(|| malformed(format!("parameter without '=': {param:?}")))?;
            let index = NAMES
                .iter()
                .position(|known| *known == name.trim())
                .ok_or_else(|| malformed(format!("unknown parameter {:?}", name.trim())))?;
            if values[index].replace(value.trim()).is_some() {
                return Err(malformed(format!("duplicate parameter {:?}", NAMES[index])));
            }
        }
        let [Some(node), Some(key_id), Some(timestamp), Some(nonce), Some(signature)] = values
        else {
            let missing = NAMES
                .iter()
                .zip(values)
                .find_map(|(name, value)| value.is_none().then_some(*name))
                .unwrap_or_default();
            return Err(malformed(format!("missing parameter {missing:?}")));
        };
        let node = ActorId::new(node.to_owned())
            .map_err(|error| malformed(format!("node {node:?}: {error}")))?;
        let timestamp = timestamp
            .parse()
            .map_err(|_| malformed(format!("timestamp {timestamp:?} is not an integer")))?;
        let nonce = decode_hex::<NODE_NONCE_LEN>(nonce)
            .ok_or_else(|| malformed(format!("nonce {nonce:?} is not 32 hex characters")))?;
        let signature = STANDARD
            .decode(signature)
            .ok()
            .and_then(|bytes| <[u8; SIGNATURE_LEN]>::try_from(bytes.as_slice()).ok())
            .ok_or_else(|| malformed("signature is not 64 base64-encoded bytes".to_owned()))?;
        Ok(Self {
            node,
            key_id: KeyId::new(key_id.to_owned()),
            timestamp,
            nonce,
            signature,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeAuthHeaderError {
    /// The header names another scheme (the value it named).
    UnsupportedScheme(String),
    Malformed(String),
}

impl fmt::Display for NodeAuthHeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedScheme(scheme) => {
                write!(
                    f,
                    "authorization scheme {scheme:?} is not {NODE_SCHEME_TAG}"
                )
            }
            Self::Malformed(reason) => write!(f, "malformed {NODE_SCHEME_TAG} header: {reason}"),
        }
    }
}

impl std::error::Error for NodeAuthHeaderError {}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// Lowercase hex only, so one nonce has one spelling in the replay
/// cache.
fn decode_hex<const N: usize>(input: &str) -> Option<[u8; N]> {
    let digits = input.as_bytes();
    if digits.len() != N * 2 {
        return None;
    }
    let nibble = |digit: u8| match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        _ => None,
    };
    let mut out = [0; N];
    let (pairs, _) = digits.as_chunks::<2>();
    for (byte, [high, low]) in out.iter_mut().zip(pairs) {
        *byte = (nibble(*high)? << 4) | nibble(*low)?;
    }
    Some(out)
}
