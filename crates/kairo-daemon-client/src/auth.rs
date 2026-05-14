//! Wire format for the daemon write-path's signed-request
//! envelope (Phase B; see `DECISIONS.md §14` and
//! `PHASE_2_DAEMON_AUTH.md` Slice 1).
//!
//! This module is **pure**: it builds and parses the
//! `Authorization: KairoActor1 ...` header and the canonical
//! signing string the daemon verifies signatures against. No
//! network, no signing-key material, no async — those land in
//! Slice 2's client and daemon wiring.
//!
//! Slice 2 will:
//!
//! - Sign `build_signing_string(...)` with the actor's local
//!   secret key (ed25519) and stuff the resulting bytes into an
//!   [`AuthEnvelope`].
//! - Send `envelope.format()` as the request's `Authorization`
//!   header.
//! - On the daemon side, parse the header with
//!   [`AuthEnvelope::parse`], rebuild the signing string from
//!   the request, and verify the signature against the resolved
//!   actor's active key.
//!
//! Layout per `DECISIONS.md §14` sub-decisions 2–4:
//!
//! ```text
//! Authorization: KairoActor1 actor=<actor-id>, key_id=<key-id>,
//!   timestamp=<i64>, nonce=<32-hex>, signature=<base64>
//!
//! signing string:
//!   kairo-daemon-write-v1\n
//!   <METHOD>\n
//!   <path-and-query>\n
//!   <sha256-hex of body>\n
//!   <timestamp-epoch-seconds>\n
//!   <nonce-hex>\n
//! ```
//!
//! Domain prefix (`kairo-daemon-write-v1`) prevents a captured
//! signature from being replayed in any future Kairo signing
//! context. Trailing newlines eliminate concatenation ambiguity
//! between adjacent fields.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use kairo_core::ActorId;
use kairo_identity::{KeyId, SecretSigningKey};
use sha2::{Digest, Sha256};

/// Domain prefix included verbatim as the first line of every
/// signing string. Bumping this version digit invalidates every
/// signature signed against the old domain — used as a hard
/// migration boundary for future wire-format revisions.
pub const SIGNING_DOMAIN: &str = "kairo-daemon-write-v1";

/// Authentication scheme tag the daemon recognizes on the
/// `Authorization` header. The trailing `1` is the wire-format
/// version digit (sibling to `SIGNING_DOMAIN`'s `-v1`).
pub const SCHEME_TAG: &str = "KairoActor1";

/// Width of the timestamp window the daemon enforces (±this).
/// Per `DECISIONS.md §14` sub-decision 5: 60 seconds. A request
/// whose `timestamp` is more than this many seconds from `now()`
/// is rejected with `timestamp_out_of_window`.
pub const TIMESTAMP_WINDOW_SECONDS: i64 = 60;

/// Length of the per-request nonce in bytes (Phase B uses 16 —
/// 128 bits of randomness, hex-encoded to 32 ASCII characters).
pub const NONCE_LEN_BYTES: usize = 16;

/// Length of an ed25519 signature in bytes.
pub const SIGNATURE_LEN_BYTES: usize = 64;

/// Build the canonical signing string for a daemon write
/// request. The output is the exact byte sequence both client
/// and daemon sign / verify; it is *not* sent on the wire (only
/// the resulting signature is, in the `Authorization` header).
///
/// `method` is the uppercase HTTP verb (`"POST"`, `"PUT"`, …).
/// `path_and_query` is the request's `:path` exactly as it
/// appears on the wire (no normalization, no decoding).
/// `body` is the raw request body bytes — empty for `GET`s and
/// for any future body-less write verbs.
pub fn build_signing_string(
    method: &str,
    path_and_query: &str,
    body: &[u8],
    timestamp: i64,
    nonce: &[u8; NONCE_LEN_BYTES],
) -> String {
    let mut out = String::with_capacity(SIGNING_DOMAIN.len() + 256);
    out.push_str(SIGNING_DOMAIN);
    out.push('\n');
    out.push_str(method);
    out.push('\n');
    out.push_str(path_and_query);
    out.push('\n');
    out.push_str(&body_hash_hex(body));
    out.push('\n');
    out.push_str(&timestamp.to_string());
    out.push('\n');
    out.push_str(&encode_hex(nonce));
    out.push('\n');
    out
}

/// Lowercase-hex SHA-256 of `body`. Always defined — empty body
/// hashes to the SHA-256 of zero bytes (a fixed constant).
pub fn body_hash_hex(body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body);
    let digest = hasher.finalize();
    encode_hex(&digest)
}

/// Generate a fresh per-request nonce. 16 random bytes from
/// the OS CSPRNG via `getrandom`. Returns `Err` only if the OS
/// random source is unavailable, which is fatal for the
/// requesting CLI invocation.
pub fn random_nonce() -> Result<[u8; NONCE_LEN_BYTES], NonceError> {
    let mut nonce = [0u8; NONCE_LEN_BYTES];
    getrandom::getrandom(&mut nonce).map_err(|error| NonceError {
        reason: error.to_string(),
    })?;
    Ok(nonce)
}

/// Failure mode of [`random_nonce`]. Wraps the platform-level
/// `getrandom` error string so callers can surface a useful
/// diagnostic.
#[derive(Debug)]
pub struct NonceError {
    pub reason: String,
}

impl std::fmt::Display for NonceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "could not generate request nonce: {}", self.reason)
    }
}

impl std::error::Error for NonceError {}

/// Build and sign a request envelope in one step. The actor must
/// match the signer's identity at the data-model layer (the
/// client passes in the same `ActorId` the signed statement
/// declares), and the `key_id` is derived from the secret's
/// public key — so the daemon's resolver sees a coherent
/// (actor, key_id, signature) triple.
///
/// `timestamp` is i64 epoch seconds (typically `Timestamp::now().seconds()`).
/// `nonce` is 16 random bytes from [`random_nonce`].
///
/// Panics only if the underlying ed25519 signature is not
/// exactly 64 bytes, which is a hard guarantee from
/// `kairo-identity::SecretSigningKey::sign` and would indicate a
/// crypto-library regression.
pub fn sign_request(
    method: &str,
    path_and_query: &str,
    body: &[u8],
    actor: ActorId,
    secret: &SecretSigningKey,
    timestamp: i64,
    nonce: [u8; NONCE_LEN_BYTES],
) -> AuthEnvelope {
    let signing = build_signing_string(method, path_and_query, body, timestamp, &nonce);
    let signature_bytes = secret.sign(signing.as_bytes());
    let signature: [u8; SIGNATURE_LEN_BYTES] = *signature_bytes.bytes();
    AuthEnvelope {
        actor,
        key_id: secret.public_key().key_id(),
        timestamp,
        nonce,
        signature,
    }
}

/// Parsed/buildable form of an `Authorization: KairoActor1 ...`
/// header. Carries the typed identifiers + raw signature bytes;
/// constructing one from request inputs is the client's job in
/// Slice 2, parsing one is the daemon's job in Slice 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthEnvelope {
    pub actor: ActorId,
    pub key_id: KeyId,
    pub timestamp: i64,
    pub nonce: [u8; NONCE_LEN_BYTES],
    pub signature: [u8; SIGNATURE_LEN_BYTES],
}

impl AuthEnvelope {
    /// Render as a complete `Authorization` header value (no
    /// `Authorization: ` prefix — the HTTP layer adds that).
    pub fn format(&self) -> String {
        format!(
            "{SCHEME_TAG} actor={}, key_id={}, timestamp={}, nonce={}, signature={}",
            self.actor,
            self.key_id,
            self.timestamp,
            encode_hex(&self.nonce),
            BASE64.encode(self.signature),
        )
    }

    /// Parse an `Authorization` header value. The caller is
    /// responsible for stripping the `Authorization: ` HTTP
    /// prefix; this parser starts at the scheme tag.
    pub fn parse(header_value: &str) -> Result<Self, AuthHeaderError> {
        let trimmed = header_value.trim();
        let (scheme, rest) = split_scheme(trimmed)?;
        if scheme != SCHEME_TAG {
            return Err(AuthHeaderError::UnsupportedScheme(scheme.to_owned()));
        }

        let mut actor: Option<String> = None;
        let mut key_id: Option<String> = None;
        let mut timestamp: Option<i64> = None;
        let mut nonce_hex: Option<String> = None;
        let mut signature_b64: Option<String> = None;

        for raw_param in rest.split(',') {
            let param = raw_param.trim();
            if param.is_empty() {
                continue;
            }
            let (name, value) = param.split_once('=').ok_or_else(|| {
                AuthHeaderError::Malformed(format!("parameter missing '=': {param:?}"))
            })?;
            let name = name.trim();
            let value = value.trim();
            match name {
                "actor" => {
                    if actor.replace(value.to_owned()).is_some() {
                        return Err(AuthHeaderError::Malformed(
                            "duplicate `actor` parameter".to_owned(),
                        ));
                    }
                }
                "key_id" => {
                    if key_id.replace(value.to_owned()).is_some() {
                        return Err(AuthHeaderError::Malformed(
                            "duplicate `key_id` parameter".to_owned(),
                        ));
                    }
                }
                "timestamp" => {
                    let parsed: i64 = value.parse().map_err(|_| {
                        AuthHeaderError::Malformed(format!(
                            "timestamp {value:?} is not a valid i64"
                        ))
                    })?;
                    if timestamp.replace(parsed).is_some() {
                        return Err(AuthHeaderError::Malformed(
                            "duplicate `timestamp` parameter".to_owned(),
                        ));
                    }
                }
                "nonce" => {
                    if nonce_hex.replace(value.to_owned()).is_some() {
                        return Err(AuthHeaderError::Malformed(
                            "duplicate `nonce` parameter".to_owned(),
                        ));
                    }
                }
                "signature" => {
                    if signature_b64.replace(value.to_owned()).is_some() {
                        return Err(AuthHeaderError::Malformed(
                            "duplicate `signature` parameter".to_owned(),
                        ));
                    }
                }
                other => {
                    return Err(AuthHeaderError::Malformed(format!(
                        "unknown parameter {other:?}"
                    )));
                }
            }
        }

        let actor_str = actor.ok_or_else(|| {
            AuthHeaderError::Malformed("missing `actor` parameter".to_owned())
        })?;
        let key_id_str = key_id.ok_or_else(|| {
            AuthHeaderError::Malformed("missing `key_id` parameter".to_owned())
        })?;
        let timestamp = timestamp.ok_or_else(|| {
            AuthHeaderError::Malformed("missing `timestamp` parameter".to_owned())
        })?;
        let nonce_hex = nonce_hex.ok_or_else(|| {
            AuthHeaderError::Malformed("missing `nonce` parameter".to_owned())
        })?;
        let signature_b64 = signature_b64.ok_or_else(|| {
            AuthHeaderError::Malformed("missing `signature` parameter".to_owned())
        })?;

        let actor = ActorId::new(actor_str.clone()).map_err(|error| {
            AuthHeaderError::Malformed(format!(
                "invalid actor id {actor_str:?}: {error}"
            ))
        })?;
        let key_id = KeyId::new(key_id_str);
        let nonce = decode_hex_array::<NONCE_LEN_BYTES>(&nonce_hex).map_err(|error| {
            AuthHeaderError::Malformed(format!("nonce decode: {error}"))
        })?;
        let signature_bytes = BASE64.decode(&signature_b64).map_err(|error| {
            AuthHeaderError::Malformed(format!("signature base64 decode: {error}"))
        })?;
        let signature: [u8; SIGNATURE_LEN_BYTES] =
            signature_bytes.as_slice().try_into().map_err(|_| {
                AuthHeaderError::Malformed(format!(
                    "signature must be {SIGNATURE_LEN_BYTES} bytes, got {}",
                    signature_bytes.len()
                ))
            })?;

        Ok(Self {
            actor,
            key_id,
            timestamp,
            nonce,
            signature,
        })
    }
}

/// Typed parse errors for `AuthEnvelope::parse`. The daemon
/// middleware maps these onto the `401 unauthorized` error codes
/// in `PHASE_2_DAEMON_AUTH.md §2.3`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthHeaderError {
    /// Scheme tag does not match `SCHEME_TAG`. The carried string
    /// is whatever the client *did* send, so operators can tell
    /// at a glance whether they're hitting a wire-format
    /// mismatch.
    UnsupportedScheme(String),
    /// Anything else — missing required field, duplicated field,
    /// unknown field, base64 / hex / integer parse failure,
    /// mis-sized byte buffer.
    Malformed(String),
}

impl std::fmt::Display for AuthHeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedScheme(scheme) => write!(
                f,
                "unsupported authorization scheme {scheme:?}; expected {SCHEME_TAG}",
            ),
            Self::Malformed(reason) => write!(f, "malformed authorization header: {reason}"),
        }
    }
}

impl std::error::Error for AuthHeaderError {}

fn split_scheme(input: &str) -> Result<(&str, &str), AuthHeaderError> {
    match input.split_once(' ') {
        Some((scheme, rest)) => Ok((scheme, rest.trim_start())),
        None => Err(AuthHeaderError::Malformed(
            "authorization header has no scheme/params separator".to_owned(),
        )),
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(hex_char(byte >> 4));
        out.push(hex_char(byte & 0x0f));
    }
    out
}

fn hex_char(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'a' + (nibble - 10)),
        _ => '?',
    }
}

fn decode_hex_array<const N: usize>(input: &str) -> Result<[u8; N], String> {
    if input.len() != N * 2 {
        return Err(format!(
            "expected {expected_chars} hex characters, got {actual}",
            expected_chars = N * 2,
            actual = input.len()
        ));
    }
    let mut out = [0u8; N];
    let bytes = input.as_bytes();
    for i in 0..N {
        let hi = decode_hex_nibble(bytes[i * 2])?;
        let lo = decode_hex_nibble(bytes[i * 2 + 1])?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn decode_hex_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(format!("invalid hex character {:?}", char::from(byte))),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    //! Wire-format primitives are pinned by byte-level fixtures
    //! here so a future refactor can't silently rotate any field
    //! order, encoding, or domain string. Phase B's verifier
    //! depends on these exact bytes — if a test changes, the
    //! daemon's accepted signatures change.
    use super::*;

    /// Fixed actor id derived from a constant digest so the
    /// bytes are real base58btc (`ActorId::new` enforces strict
    /// base58btc + multihash framing). The corresponding key id
    /// is similarly synthetic — `KeyId` doesn't validate, but
    /// we still want a stable string fixture.
    fn fixture_actor() -> ActorId {
        ActorId::from_sha256_digest([0x42; 32])
    }

    fn fixture_actor_str() -> String {
        fixture_actor().to_string()
    }

    const FIXTURE_KEY_ID: &str = "kairo-key:fixture-phase-b";

    fn fixture_nonce() -> [u8; NONCE_LEN_BYTES] {
        // Distinct bytes 0..16 so any byte-ordering bug in the
        // hex encoder lights up.
        [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ]
    }

    fn fixture_signature() -> [u8; SIGNATURE_LEN_BYTES] {
        let mut sig = [0u8; SIGNATURE_LEN_BYTES];
        for (i, byte) in sig.iter_mut().enumerate() {
            let scaled = u8::try_from(i & 0xff).unwrap_or(0);
            *byte = scaled.wrapping_mul(7).wrapping_add(0x11);
        }
        sig
    }

    fn fixture_envelope() -> AuthEnvelope {
        AuthEnvelope {
            actor: fixture_actor(),
            key_id: KeyId::new(FIXTURE_KEY_ID.to_owned()),
            timestamp: 1_700_000_000,
            nonce: fixture_nonce(),
            signature: fixture_signature(),
        }
    }

    // ---- body_hash_hex ----

    #[test]
    fn body_hash_of_empty_body_is_sha256_of_zero_bytes() {
        // `SHA-256("")` = e3b0c4… — the only fixed point worth
        // hard-coding; anything else risks copy-pasting a hash
        // from elsewhere and missing a domain-confusion bug.
        let hash = body_hash_hex(&[]);
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn body_hash_changes_with_body() {
        assert_ne!(body_hash_hex(b"a"), body_hash_hex(b"b"));
        assert_ne!(body_hash_hex(b"ab"), body_hash_hex(b"ba"));
    }

    // ---- build_signing_string ----

    #[test]
    fn signing_string_pins_field_order_and_trailing_newlines() {
        // Fixed inputs → fixed bytes. If this assertion drifts,
        // every Phase B signature on the wire becomes
        // un-verifiable. Treat it like a wire-format pin.
        let signing = build_signing_string(
            "POST",
            "/api/v1/statements",
            b"{}",
            1_700_000_000,
            &fixture_nonce(),
        );
        let body_hash = body_hash_hex(b"{}");
        let expected = format!(
            "kairo-daemon-write-v1\nPOST\n/api/v1/statements\n{body_hash}\n1700000000\n000102030405060708090a0b0c0d0e0f\n"
        );
        assert_eq!(signing, expected);
    }

    #[test]
    fn signing_string_empty_body_uses_sha256_of_empty() {
        let signing = build_signing_string("POST", "/x", &[], 0, &[0; NONCE_LEN_BYTES]);
        assert!(
            signing.contains("\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n"),
            "expected empty-body SHA-256 in signing string: {signing}",
        );
    }

    #[test]
    fn signing_string_path_is_byte_verbatim() {
        // The contract is: `path_and_query` is whatever the
        // client put on the wire. Percent-encoding and query
        // ordering belong to the client, not the canonicalizer.
        let signing = build_signing_string(
            "POST",
            "/api/v1/statements?a=1&b=2",
            &[],
            0,
            &[0; NONCE_LEN_BYTES],
        );
        assert!(
            signing.contains("\n/api/v1/statements?a=1&b=2\n"),
            "path round-trips verbatim: {signing}",
        );
    }

    // ---- format() / parse() round-trip ----

    #[test]
    fn envelope_round_trips_through_format_parse() {
        let original = fixture_envelope();
        let header = original.format();
        let parsed = AuthEnvelope::parse(&header).expect("parse");
        assert_eq!(parsed, original);
    }

    #[test]
    fn format_emits_scheme_and_field_order() {
        let header = fixture_envelope().format();
        assert!(
            header.starts_with("KairoActor1 "),
            "header missing scheme tag: {header}"
        );
        // Pinning the canonical field order keeps the format
        // diffable across builds; parsing tolerates reorders.
        let after_scheme = &header["KairoActor1 ".len()..];
        let positions = [
            "actor=",
            "key_id=",
            "timestamp=",
            "nonce=",
            "signature=",
        ]
        .iter()
        .map(|needle| after_scheme.find(needle).expect("needle"))
        .collect::<Vec<_>>();
        let sorted = {
            let mut s = positions.clone();
            s.sort();
            s
        };
        assert_eq!(positions, sorted, "canonical field order drifted: {header}");
    }

    #[test]
    fn parse_tolerates_reordered_fields() {
        // The wire format pins the *canonical* order; the parser
        // must still accept any ordering so future clients (and
        // off-the-shelf HTTP middleware) can shuffle them.
        let header = format!(
            "KairoActor1 signature={}, nonce={}, timestamp=1700000000, key_id={}, actor={}",
            BASE64.encode(fixture_signature()),
            encode_hex(&fixture_nonce()),
            FIXTURE_KEY_ID,
            fixture_actor_str(),
        );
        let parsed = AuthEnvelope::parse(&header).expect("parse");
        assert_eq!(parsed, fixture_envelope());
    }

    #[test]
    fn parse_tolerates_leading_and_trailing_whitespace() {
        let header = format!("  {}  ", fixture_envelope().format());
        AuthEnvelope::parse(&header).expect("parse");
    }

    // ---- parse failure modes ----

    #[test]
    fn parse_rejects_unsupported_scheme() {
        let result = AuthEnvelope::parse("Bearer abc.def");
        match result {
            Err(AuthHeaderError::UnsupportedScheme(scheme)) => assert_eq!(scheme, "Bearer"),
            other => panic!("expected UnsupportedScheme, got {other:?}"),
        }
    }

    #[test]
    fn parse_rejects_missing_actor() {
        let header = format!(
            "KairoActor1 key_id={}, timestamp=0, nonce={}, signature={}",
            FIXTURE_KEY_ID,
            encode_hex(&fixture_nonce()),
            BASE64.encode(fixture_signature()),
        );
        assert!(matches!(
            AuthEnvelope::parse(&header),
            Err(AuthHeaderError::Malformed(_))
        ));
    }

    #[test]
    fn parse_rejects_duplicate_field() {
        let header = format!(
            "KairoActor1 actor={a}, actor={a}, key_id={k}, timestamp=0, nonce={n}, signature={s}",
            a = fixture_actor_str(),
            k = FIXTURE_KEY_ID,
            n = encode_hex(&fixture_nonce()),
            s = BASE64.encode(fixture_signature()),
        );
        assert!(matches!(
            AuthEnvelope::parse(&header),
            Err(AuthHeaderError::Malformed(_))
        ));
    }

    #[test]
    fn parse_rejects_unknown_field() {
        let header = format!(
            "KairoActor1 actor={a}, key_id={k}, timestamp=0, nonce={n}, signature={s}, expires=10",
            a = fixture_actor_str(),
            k = FIXTURE_KEY_ID,
            n = encode_hex(&fixture_nonce()),
            s = BASE64.encode(fixture_signature()),
        );
        assert!(matches!(
            AuthEnvelope::parse(&header),
            Err(AuthHeaderError::Malformed(_))
        ));
    }

    #[test]
    fn parse_rejects_invalid_actor_id() {
        let header = format!(
            "KairoActor1 actor=not!valid, key_id={k}, timestamp=0, nonce={n}, signature={s}",
            k = FIXTURE_KEY_ID,
            n = encode_hex(&fixture_nonce()),
            s = BASE64.encode(fixture_signature()),
        );
        assert!(matches!(
            AuthEnvelope::parse(&header),
            Err(AuthHeaderError::Malformed(_))
        ));
    }

    #[test]
    fn parse_rejects_bad_timestamp() {
        let header = format!(
            "KairoActor1 actor={a}, key_id={k}, timestamp=not-an-int, nonce={n}, signature={s}",
            a = fixture_actor_str(),
            k = FIXTURE_KEY_ID,
            n = encode_hex(&fixture_nonce()),
            s = BASE64.encode(fixture_signature()),
        );
        assert!(matches!(
            AuthEnvelope::parse(&header),
            Err(AuthHeaderError::Malformed(_))
        ));
    }

    #[test]
    fn parse_rejects_short_nonce() {
        let header = format!(
            "KairoActor1 actor={a}, key_id={k}, timestamp=0, nonce=00, signature={s}",
            a = fixture_actor_str(),
            k = FIXTURE_KEY_ID,
            s = BASE64.encode(fixture_signature()),
        );
        assert!(matches!(
            AuthEnvelope::parse(&header),
            Err(AuthHeaderError::Malformed(_))
        ));
    }

    #[test]
    fn parse_rejects_non_hex_nonce() {
        let header = format!(
            "KairoActor1 actor={a}, key_id={k}, timestamp=0, nonce=zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz, signature={s}",
            a = fixture_actor_str(),
            k = FIXTURE_KEY_ID,
            s = BASE64.encode(fixture_signature()),
        );
        assert!(matches!(
            AuthEnvelope::parse(&header),
            Err(AuthHeaderError::Malformed(_))
        ));
    }

    #[test]
    fn parse_rejects_signature_with_wrong_length() {
        let short_sig = BASE64.encode([0u8; 32]);
        let header = format!(
            "KairoActor1 actor={a}, key_id={k}, timestamp=0, nonce={n}, signature={short_sig}",
            a = fixture_actor_str(),
            k = FIXTURE_KEY_ID,
            n = encode_hex(&fixture_nonce()),
        );
        assert!(matches!(
            AuthEnvelope::parse(&header),
            Err(AuthHeaderError::Malformed(_))
        ));
    }

    #[test]
    fn parse_rejects_bad_base64_signature() {
        let header = format!(
            "KairoActor1 actor={a}, key_id={k}, timestamp=0, nonce={n}, signature=!!!not-base64!!!",
            a = fixture_actor_str(),
            k = FIXTURE_KEY_ID,
            n = encode_hex(&fixture_nonce()),
        );
        assert!(matches!(
            AuthEnvelope::parse(&header),
            Err(AuthHeaderError::Malformed(_))
        ));
    }

    #[test]
    fn parse_rejects_parameter_without_equals() {
        let header = "KairoActor1 actor".to_owned();
        assert!(matches!(
            AuthEnvelope::parse(&header),
            Err(AuthHeaderError::Malformed(_))
        ));
    }

    #[test]
    fn parse_rejects_header_with_no_separator() {
        assert!(matches!(
            AuthEnvelope::parse("KairoActor1"),
            Err(AuthHeaderError::Malformed(_))
        ));
    }
}
