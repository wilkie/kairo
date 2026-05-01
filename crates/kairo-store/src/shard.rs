use std::path::PathBuf;

/// Returns the two-level shard directories for a Kairo ID.
///
/// All Kairo IDs are encoded as `z` (multibase prefix) + `Qm` (sha2-256
/// multihash codec/length) + ~42 base58btc digest characters. The first three
/// characters are constant; sharding bytes are taken from the random portion.
///
/// Level 1 = ID positions 3-4, level 2 = positions 5-6. Yields up to 58⁴ ≈
/// 11.3M sparse leaf directories per record type.
pub(crate) fn shard_dirs(id: &str) -> Result<(&str, &str), ShardError> {
    if id.len() < 7 {
        return Err(ShardError::IdTooShort);
    }
    if !id.is_char_boundary(3) || !id.is_char_boundary(5) || !id.is_char_boundary(7) {
        return Err(ShardError::InvalidId);
    }
    let level1 = &id[3..5];
    let level2 = &id[5..7];
    Ok((level1, level2))
}

/// Returns `<root>/<type_dir>/<XX>/<YY>/<id><suffix>` for the given ID.
pub(crate) fn shard_path(
    root: &std::path::Path,
    type_dir: &str,
    id: &str,
    suffix: &str,
) -> Result<PathBuf, ShardError> {
    let (level1, level2) = shard_dirs(id)?;
    let mut path = root.to_path_buf();
    path.push(type_dir);
    path.push(level1);
    path.push(level2);
    path.push(format!("{id}{suffix}"));
    Ok(path)
}

#[derive(Debug)]
pub(crate) enum ShardError {
    IdTooShort,
    InvalidId,
}

impl std::fmt::Display for ShardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IdTooShort => f.write_str("id is too short to shard"),
            Self::InvalidId => f.write_str("id contains invalid character boundaries"),
        }
    }
}

impl std::error::Error for ShardError {}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn shards_typical_kairo_id() -> TestResult {
        let id = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";
        assert_eq!(
            shard_dirs(id).map_err(|error| error.to_string())?,
            ("R8", "3z")
        );
        Ok(())
    }

    #[test]
    fn rejects_short_id() {
        assert!(matches!(shard_dirs("zQmAB"), Err(ShardError::IdTooShort)));
    }

    #[test]
    fn builds_full_shard_path() -> TestResult {
        let id = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";
        let path = shard_path(std::path::Path::new("/store"), "statements", id, ".json")
            .map_err(|error| error.to_string())?;
        assert_eq!(
            path,
            std::path::PathBuf::from(format!("/store/statements/R8/3z/{id}.json"))
        );
        Ok(())
    }
}
