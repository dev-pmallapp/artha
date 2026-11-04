//! Low-level, crash-safe file primitives shared by every JSON document.
//!
//! - [`atomic_write`]: write to a `NamedTempFile` in the *same directory*,
//!   `fsync`, set permissions, then `rename` over the target (and `fsync` the
//!   directory on Unix). A crash leaves either the old or the new file, never
//!   a truncated one.
//! - [`FileLock`]: an advisory, cross-process exclusive lock on a sidecar
//!   `<file>.lock`, using `std::fs::File::lock` (Rust ≥ 1.89). Used for
//!   read-modify-write of JSON documents and to serialize checkpoint commits
//!   between two Artha processes in the same workdir.
//!
//! Locks are advisory and only guard Artha-vs-Artha races; they never block
//! readers that do not take the lock (plain `load` is lock-free because
//! writes are atomic renames).

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::Result;

/// Permission class for a written file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileMode {
    /// Owner read/write only (`0600` on Unix). For files holding secrets.
    Private,
    /// Process umask default (typically `0644`).
    #[default]
    Default,
}

/// Atomically replace `path` with `bytes`. Creates the parent directory if needed.
pub fn atomic_write(path: &Path, bytes: &[u8], mode: FileMode) -> Result<()> {
    todo!("NamedTempFile::new_in(parent) -> write_all -> sync_all -> chmod -> persist(path) -> fsync dir")
}

/// Exclusive advisory lock on `<target>.lock`; released on drop.
#[derive(Debug)]
pub struct FileLock {
    file: File,
    path: PathBuf,
}

impl FileLock {
    /// Lock-file path used for a given target file.
    pub fn lock_path_for(target: &Path) -> PathBuf {
        todo!("target with extension + \".lock\" appended")
    }

    /// Block until the lock is acquired, up to `timeout`, polling `try_lock`.
    /// Returns [`crate::DataError::Locked`] on timeout.
    pub fn acquire(target: &Path, timeout: Duration) -> Result<Self> {
        todo!("open/create lock file; loop try_lock with backoff until timeout")
    }

    /// Single non-blocking attempt.
    pub fn try_acquire(target: &Path) -> Result<Option<Self>> {
        todo!("File::try_lock; Ok(None) on WouldBlock")
    }

    /// The `.lock` file this guard holds.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Explicit unlock for clarity; closing the fd also releases it.
        let _ = self.file.unlock();
    }
}
