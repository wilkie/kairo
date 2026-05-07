//! Two-level base58 sharding for Kairo content-addressed IDs.
//!
//! All Kairo IDs are encoded as `z` (multibase prefix) + `Qm` (sha2-256
//! multihash codec/length) + ~42 base58btc digest characters. The
//! first three characters are constant; sharding bytes are taken from
//! the random portion: level 1 = positions 3-4, level 2 = positions
//! 5-6. Yields up to 58⁴ ≈ 11.3M sparse leaf directories per record
//! type.
//!
//! This algorithm is a copy of `kairo_store::shard`. Keeping it
//! duplicated avoids cycling `kairo-git` through `kairo-store`; if a
//! third user appears, lift this to `kairo-core` and depend on it
//! everywhere instead.

use std::path::PathBuf;

use crate::GitError;

/// Returns the two-level shard directories for a Kairo ID, or
/// [`GitError::CacheInvalidObjectId`] if the id is too short or
/// splits across a non-ASCII boundary.
pub(crate) fn shard_dirs(id: &str) -> Result<(&str, &str), GitError> {
    if id.len() < 7 {
        return Err(GitError::CacheInvalidObjectId {
            id: id.to_owned(),
            reason: "id is too short to shard",
        });
    }
    if !id.is_char_boundary(3) || !id.is_char_boundary(5) || !id.is_char_boundary(7) {
        return Err(GitError::CacheInvalidObjectId {
            id: id.to_owned(),
            reason: "id contains invalid character boundaries",
        });
    }
    let level1 = &id[3..5];
    let level2 = &id[5..7];
    Ok((level1, level2))
}

/// Returns `<root>/<XX>/<YY>/<id>` for the given ID. The git cache
/// has no `type_dir` slot — pool sits at the cache root alongside
/// the sharded per-object trees.
pub(crate) fn shard_path(root: &std::path::Path, id: &str) -> Result<PathBuf, GitError> {
    let (level1, level2) = shard_dirs(id)?;
    let mut path = root.to_path_buf();
    path.push(level1);
    path.push(level2);
    path.push(id);
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_ID: &str = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";

    #[test]
    fn shards_typical_kairo_id() {
        let (l1, l2) = shard_dirs(SAMPLE_ID).expect("shard");
        assert_eq!((l1, l2), ("R8", "3z"));
    }

    #[test]
    fn rejects_short_id() {
        assert!(matches!(
            shard_dirs("zQmAB"),
            Err(GitError::CacheInvalidObjectId { .. })
        ));
    }

    #[test]
    fn builds_full_shard_path() {
        let path = shard_path(std::path::Path::new("/cache"), SAMPLE_ID).expect("path");
        assert_eq!(
            path,
            std::path::PathBuf::from(format!("/cache/R8/3z/{SAMPLE_ID}"))
        );
    }
}
