//! Per-actor advisory file locking for keystore writes.
//!
//! Mirrors `kairo_store::lock`: an exclusive advisory lock on a
//! `<key-file>.lock` sidecar serializes concurrent writers without
//! blocking readers. See that module for the locking-strategy
//! rationale; the only difference here is that the lock subject is
//! per-actor (the `<actor-id>.json` key file) rather than per-record
//! index file.

use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs2::FileExt;

use crate::error::KeystoreError;

const LOCK_DEADLINE: Duration = Duration::from_secs(2);
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// Acquire an exclusive advisory lock on `<path>.lock`, run `body`,
/// then release the lock by dropping the sentinel file handle.
pub(crate) fn with_key_lock<T>(
    path: &Path,
    body: impl FnOnce() -> Result<T, KeystoreError>,
) -> Result<T, KeystoreError> {
    let lock_path = lock_path_for(path);
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;

    acquire_exclusive(&lock_file, &lock_path)?;
    body()
}

fn lock_path_for(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_owned();
    os.push(".lock");
    PathBuf::from(os)
}

fn acquire_exclusive(file: &File, lock_path: &Path) -> Result<(), KeystoreError> {
    let deadline = Instant::now() + LOCK_DEADLINE;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(()),
            Err(error) if would_block(&error) => {
                if Instant::now() >= deadline {
                    return Err(KeystoreError::LockTimeout {
                        path: lock_path.to_path_buf(),
                    });
                }
                std::thread::sleep(LOCK_RETRY_INTERVAL);
            }
            Err(error) => return Err(KeystoreError::Unavailable(error)),
        }
    }
}

fn would_block(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::WouldBlock | ErrorKind::PermissionDenied
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Barrier;
    use tempfile::TempDir;

    #[test]
    fn lock_serializes_concurrent_writers() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("counter");
        let counter = Arc::new(std::sync::Mutex::new(0u32));
        let in_critical = Arc::new(std::sync::Mutex::new(false));
        let barrier = Arc::new(Barrier::new(2));

        std::thread::scope(|scope| {
            for _ in 0..2 {
                let path = path.clone();
                let counter = Arc::clone(&counter);
                let in_critical = Arc::clone(&in_critical);
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    with_key_lock(&path, || {
                        {
                            let mut held = in_critical.lock().expect("lock");
                            assert!(!*held, "two threads inside critical section");
                            *held = true;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                        *counter.lock().expect("counter") += 1;
                        {
                            let mut held = in_critical.lock().expect("lock");
                            *held = false;
                        }
                        Ok(())
                    })
                    .expect("with_key_lock");
                });
            }
        });

        assert_eq!(*counter.lock().expect("counter"), 2);
    }

    #[test]
    fn lock_creates_sidecar_file() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("nested/dir/key.json");
        with_key_lock(&path, || Ok(())).expect("acquire");

        let lock_path = lock_path_for(&path);
        assert!(lock_path.exists(), "lock sidecar should exist");
    }
}
