//! Per-record advisory file locking for read-modify-write index files.
//!
//! `with_index_lock(path, body)` acquires an exclusive advisory lock
//! on `<path>.lock` (a zero-byte sentinel file), runs `body`, and
//! drops the lock when `body` returns. The lock subject is a sidecar
//! `.lock` file rather than the index file itself: the index file may
//! not exist on the first write, so we'd race against the write while
//! creating it. The sidecar gives a stable lock target.
//!
//! ## Locking strategy
//!
//! - **Per-record, not per-shard or store-wide.** Two unrelated writes
//!   (different actors, different objects) take different lock files
//!   and don't block each other. Shard- or store-wide alternatives
//!   would serialize unrelated work for no gain in this layer.
//! - **Writers only.** Reads don't acquire the lock — `atomic_write`
//!   + `fs::rename` already gives readers a consistent snapshot of
//!   one prior write. Holding a shared lock around reads would just
//!   add contention without improving the guarantee.
//! - **Bounded retry, then error.** `try_lock_exclusive` in a loop
//!   with 50 ms sleep and a 2 s deadline. Expired contention surfaces
//!   as `StoreError::LockTimeout` rather than blocking forever — that
//!   way deadlocks surface fast in tests and operators.
//!
//! ## Cross-platform behavior
//!
//! `fs2` wraps POSIX `flock(2)` and Windows `LockFileEx`. Both release
//! the lock when the file descriptor / handle closes, so a crashed
//! writer doesn't leave a stuck lock — the OS reaps it as part of
//! tearing the process down.

use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs2::FileExt;

use crate::error::StoreError;

/// How long we'll wait for a contended lock before erroring.
const LOCK_DEADLINE: Duration = Duration::from_secs(2);

/// How long to sleep between retries while waiting for a contended lock.
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// Acquire an exclusive advisory lock on `<path>.lock`, run `body`,
/// then release the lock by dropping the sentinel file handle. The
/// sentinel file is created if missing (with the parent directory).
///
/// Returns whatever `body` returns. If the lock can't be acquired
/// within [`LOCK_DEADLINE`], returns
/// [`StoreError::LockTimeout`] without invoking `body`.
pub(crate) fn with_index_lock<T>(
    path: &Path,
    body: impl FnOnce() -> Result<T, StoreError>,
) -> Result<T, StoreError> {
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
    // The body runs while we hold the lock. The lock is released when
    // `lock_file` drops at the end of this function — that drop runs
    // even if `body` returns Err, so contended writers always see a
    // released lock.
    body()
}

fn lock_path_for(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_owned();
    os.push(".lock");
    PathBuf::from(os)
}

/// Try-lock loop with a bounded deadline. Returns `Ok(())` on
/// acquisition or `StoreError::LockTimeout` on expiry.
fn acquire_exclusive(file: &File, lock_path: &Path) -> Result<(), StoreError> {
    let deadline = Instant::now() + LOCK_DEADLINE;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(()),
            Err(error) if would_block(&error) => {
                if Instant::now() >= deadline {
                    return Err(StoreError::LockTimeout {
                        path: lock_path.to_path_buf(),
                    });
                }
                std::thread::sleep(LOCK_RETRY_INTERVAL);
            }
            Err(error) => return Err(StoreError::Unavailable(error)),
        }
    }
}

/// `fs2::FileExt::try_lock_exclusive` returns `WouldBlock` on POSIX,
/// `PermissionDenied` on some Windows configurations. Treat both as
/// "contended, retry."
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
        // Two threads each call with_index_lock on the same path with
        // a body that increments a shared counter. After both return,
        // the counter must equal 2 (no lost increments) and the two
        // critical sections must not have overlapped.
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
                    with_index_lock(&path, || {
                        // Verify nobody else is inside.
                        {
                            let mut held = in_critical.lock().expect("lock");
                            assert!(!*held, "two threads inside critical section");
                            *held = true;
                        }
                        // Hold the lock briefly so the other thread is
                        // forced to retry. 100ms is long enough that
                        // try_lock_exclusive returns WouldBlock several
                        // times in the contender's retry loop.
                        std::thread::sleep(Duration::from_millis(100));
                        *counter.lock().expect("counter") += 1;
                        {
                            let mut held = in_critical.lock().expect("lock");
                            *held = false;
                        }
                        Ok(())
                    })
                    .expect("with_index_lock");
                });
            }
        });

        assert_eq!(*counter.lock().expect("counter"), 2);
    }

    #[test]
    fn lock_creates_sidecar_file() {
        // First call to with_index_lock on a previously-unused path
        // creates the sidecar `.lock` file. The data file itself is
        // never touched here — the lock module doesn't know what the
        // data file looks like.
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("nested/dir/data.json");
        with_index_lock(&path, || Ok(())).expect("acquire");

        let lock_path = lock_path_for(&path);
        assert!(lock_path.exists(), "lock sidecar should exist");
    }
}
