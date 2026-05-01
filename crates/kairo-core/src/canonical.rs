/// Trait for types with deterministic Kairo canonical bytes.
pub trait CanonicalEncode {
    fn encode_canonical(&self, out: &mut Vec<u8>);

    fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        self.encode_canonical(&mut bytes);
        bytes
    }
}

pub fn encode_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

pub fn encode_u16(out: &mut Vec<u8>, value: u16) {
    out.extend(value.to_be_bytes());
}

pub fn encode_u32(out: &mut Vec<u8>, value: u32) {
    out.extend(value.to_be_bytes());
}

pub fn encode_bytes(out: &mut Vec<u8>, value: &[u8]) {
    encode_len(out, value.len());
    out.extend(value);
}

pub fn encode_str(out: &mut Vec<u8>, value: &str) {
    encode_bytes(out, value.as_bytes());
}

pub fn encode_option<T>(
    out: &mut Vec<u8>,
    value: Option<&T>,
    encode_value: impl FnOnce(&mut Vec<u8>, &T),
) {
    match value {
        Some(value) => {
            encode_u8(out, 1);
            encode_value(out, value);
        }
        None => encode_u8(out, 0),
    }
}

pub fn encode_list<T>(
    out: &mut Vec<u8>,
    values: &[T],
    mut encode_value: impl FnMut(&mut Vec<u8>, &T),
) {
    encode_len(out, values.len());
    for value in values {
        encode_value(out, value);
    }
}

#[allow(clippy::cast_possible_truncation)]
fn encode_len(out: &mut Vec<u8>, len: usize) {
    debug_assert!(len <= u32::MAX as usize);
    encode_u32(out, len as u32);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_integers_big_endian() {
        let mut bytes = Vec::new();
        encode_u8(&mut bytes, 0x7f);
        encode_u16(&mut bytes, 0x1234);
        encode_u32(&mut bytes, 0x1234_5678);

        assert_eq!(bytes, vec![0x7f, 0x12, 0x34, 0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn encodes_bytes_with_u32_length_prefix() {
        let mut bytes = Vec::new();
        encode_bytes(&mut bytes, &[0xaa, 0xbb]);

        assert_eq!(bytes, vec![0, 0, 0, 2, 0xaa, 0xbb]);
    }

    #[test]
    fn encodes_strings_as_utf8_bytes() {
        let mut bytes = Vec::new();
        encode_str(&mut bytes, "kai");

        assert_eq!(bytes, vec![0, 0, 0, 3, b'k', b'a', b'i']);
    }

    #[test]
    fn encodes_options() {
        let mut none_bytes = Vec::new();
        encode_option::<u8>(&mut none_bytes, None, |out, value| encode_u8(out, *value));

        let mut some_bytes = Vec::new();
        encode_option(&mut some_bytes, Some(&9), |out, value| {
            encode_u8(out, *value)
        });

        assert_eq!(none_bytes, vec![0]);
        assert_eq!(some_bytes, vec![1, 9]);
    }

    #[test]
    fn encodes_lists() {
        let mut bytes = Vec::new();
        encode_list(&mut bytes, &[1, 2, 3], |out, value| encode_u8(out, *value));

        assert_eq!(bytes, vec![0, 0, 0, 3, 1, 2, 3]);
    }
}
