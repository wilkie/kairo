use std::fmt;
use std::str::FromStr;

use sha2::{Digest, Sha256};

use crate::IdError;

const MAX_ID_LEN: usize = 256;
const SHA2_256_MULTIHASH_CODE: u8 = 0x12;
const SHA2_256_DIGEST_LEN: u8 = 0x20;
const SHA2_256_MULTIHASH_LEN: usize = 34;
const BASE58BTC_PREFIX: u8 = b'z';
const BASE58_ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn validate_id_payload(value: &str) -> Result<(), IdError> {
    if value.is_empty() {
        return Err(IdError::Empty);
    }

    if value.len() > MAX_ID_LEN {
        return Err(IdError::TooLong { max: MAX_ID_LEN });
    }

    let Some(encoded) = value.as_bytes().strip_prefix(&[BASE58BTC_PREFIX]) else {
        return Err(IdError::InvalidEncoding);
    };

    let decoded = decode_base58btc(encoded)?;

    if decoded.len() != SHA2_256_MULTIHASH_LEN
        || decoded[0] != SHA2_256_MULTIHASH_CODE
        || decoded[1] != SHA2_256_DIGEST_LEN
    {
        return Err(IdError::InvalidEncoding);
    }

    Ok(())
}

fn id_payload_from_bytes(domain: &[u8], bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(bytes);
    let digest = hasher.finalize();

    id_payload_from_sha256_digest(digest.into())
}

fn id_payload_from_sha256_digest(digest: [u8; 32]) -> String {
    let mut multihash = Vec::with_capacity(SHA2_256_MULTIHASH_LEN);
    multihash.push(SHA2_256_MULTIHASH_CODE);
    multihash.push(SHA2_256_DIGEST_LEN);
    multihash.extend(digest);

    let mut payload = String::with_capacity(1 + 46);
    payload.push(char::from(BASE58BTC_PREFIX));
    payload.push_str(&encode_base58btc(&multihash));
    payload
}

fn encode_base58btc(bytes: &[u8]) -> String {
    let mut digits = Vec::<u8>::new();

    for byte in bytes {
        let mut carry = u32::from(*byte);

        for digit in digits.iter_mut().rev() {
            carry += u32::from(*digit) << 8;
            *digit = base58_digit(carry % 58);
            carry /= 58;
        }

        while carry > 0 {
            let next = base58_digit(carry % 58);
            digits.insert(0, next);
            carry /= 58;
        }
    }

    for _ in bytes.iter().take_while(|byte| **byte == 0) {
        digits.insert(0, 0);
    }

    digits
        .into_iter()
        .map(|digit| char::from(BASE58_ALPHABET[usize::from(digit)]))
        .collect()
}

#[allow(clippy::cast_possible_truncation)]
fn base58_digit(value: u32) -> u8 {
    debug_assert!(value < 58);
    value as u8
}

fn decode_base58btc(encoded: &[u8]) -> Result<Vec<u8>, IdError> {
    if encoded.is_empty() {
        return Err(IdError::InvalidEncoding);
    }

    let mut bytes = Vec::<u8>::new();

    for encoded_byte in encoded {
        let value = BASE58_ALPHABET
            .iter()
            .position(|alphabet_byte| alphabet_byte == encoded_byte)
            .ok_or(IdError::InvalidEncoding)?;

        let mut carry = u32::try_from(value).map_err(|_| IdError::InvalidEncoding)?;

        for byte in bytes.iter_mut().rev() {
            carry += u32::from(*byte) * 58;
            *byte = u8::try_from(carry & 0xff).map_err(|_| IdError::InvalidEncoding)?;
            carry >>= 8;
        }

        while carry > 0 {
            let next = u8::try_from(carry & 0xff).map_err(|_| IdError::InvalidEncoding)?;
            bytes.insert(0, next);
            carry >>= 8;
        }
    }

    let leading_zeroes = encoded
        .iter()
        .take_while(|byte| **byte == BASE58_ALPHABET[0])
        .count();

    if leading_zeroes > 0 {
        let mut with_zeroes = vec![0; leading_zeroes];
        with_zeroes.extend(bytes);
        Ok(with_zeroes)
    } else {
        Ok(bytes)
    }
}

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
                let value = value.into();
                validate_id_payload(&value)?;
                Ok(Self(value))
            }

            pub fn from_bytes(domain: &[u8], bytes: &[u8]) -> Self {
                Self(id_payload_from_bytes(domain, bytes))
            }

            pub fn from_sha256_digest(digest: [u8; 32]) -> Self {
                Self(id_payload_from_sha256_digest(digest))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = IdError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
    };
}

id_type!(ObjectId);
id_type!(SnapshotId);
id_type!(StatementId);
id_type!(BlobId);
id_type!(ActorId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_bare_payloads() {
        let valid_object_id = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";
        assert_eq!(
            ObjectId::new(valid_object_id).map(|id| (id.as_str().to_owned(), id.to_string())),
            Ok((valid_object_id.to_owned(), valid_object_id.to_owned()))
        );
    }

    #[test]
    fn rejects_empty_payloads() {
        assert_eq!(ObjectId::new(""), Err(IdError::Empty));
    }

    #[test]
    fn rejects_punctuation_in_payloads() {
        assert_eq!(ObjectId::new("obj_abc"), Err(IdError::InvalidEncoding));
        assert_eq!(
            ObjectId::new("kairo:object:abc"),
            Err(IdError::InvalidEncoding)
        );
    }

    #[test]
    fn rejects_non_multihash_payloads() {
        assert_eq!(
            ObjectId::new("z6MkObject123"),
            Err(IdError::InvalidEncoding)
        );
    }

    #[test]
    fn generates_payload_from_sha256_digest() {
        let id = ObjectId::from_sha256_digest([0; 32]);
        assert_eq!(
            id.to_string(),
            "zQmNLei78zWmzUdbeRB3CiUfAizWUrbeeZh5K1rhAQKCh51"
        );
        assert_eq!(ObjectId::new(id.to_string()), Ok(id));
    }

    #[test]
    fn generates_payload_from_domain_and_bytes() {
        let id = ObjectId::from_bytes(b"kairo.test.v1", b"payload");
        assert_eq!(ObjectId::new(id.to_string()), Ok(id));
    }
}
