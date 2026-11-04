//! Generic versioned JSON document storage.
//!
//! [`JsonStore<T>`] owns the *mechanics* (atomic write, lock-protected
//! read-modify-write, permissions, version gate, unknown-file-is-default);
//! the document type `T` owns the *schema* by implementing
//! [`VersionedDocument`]. This is how `settings.json` (schema here, in
//! [`crate::settings`]) and `plugins/state.json` (schema in `artha-plugins`)
//! share one implementation without this crate knowing plugin semantics.
//!
//! On-disk convention: every document is a JSON object with a top-level
//! integer `"version"` key. A missing key is treated as version 1.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::fsio::FileMode;
use crate::Result;

/// A JSON document type that can be stored with [`JsonStore`].
pub trait VersionedDocument: Serialize + DeserializeOwned + Default {
    /// Version this build writes. Bump on any incompatible shape change.
    const CURRENT_VERSION: u32;

    /// Permissions to write the file with.
    const FILE_MODE: FileMode = FileMode::Default;

    /// Upgrade a raw document from `from_version` (< `CURRENT_VERSION`) to the
    /// current shape before deserializing. Default: no upgrades exist, so any
    /// older version is returned unchanged (serde defaults fill gaps).
    fn upgrade(from_version: u32, raw: serde_json::Value) -> Result<serde_json::Value> {
        let _ = from_version;
        Ok(raw)
    }

    /// Semantic validation after deserialization (e.g. non-empty model).
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

/// Handle to one JSON document on disk. Cheap to construct; performs no I/O until used.
#[derive(Debug, Clone)]
pub struct JsonStore<T> {
    path: PathBuf,
    lock_timeout: Duration,
    _doc: PhantomData<fn() -> T>,
}

impl<T: VersionedDocument> JsonStore<T> {
    /// Default time to wait for a concurrent writer's lock.
    pub const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(5);

    /// Bind to a path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock_timeout: Self::DEFAULT_LOCK_TIMEOUT,
            _doc: PhantomData,
        }
    }

    /// Override the lock wait timeout.
    pub fn with_lock_timeout(mut self, timeout: Duration) -> Self {
        self.lock_timeout = timeout;
        self
    }

    /// The document's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load the document; a missing file yields `T::default()`.
    ///
    /// Steps: read → parse `Value` → read `version` (default 1) → reject if
    /// `> CURRENT_VERSION` → `T::upgrade` if older → deserialize → `validate`.
    pub fn load(&self) -> Result<T> {
        todo!()
    }

    /// Like [`load`](Self::load) but distinguishes "file absent".
    pub fn load_if_exists(&self) -> Result<Option<T>> {
        todo!()
    }

    /// Validate, stamp `version = CURRENT_VERSION`, and atomically write.
    pub fn save(&self, doc: &T) -> Result<()> {
        todo!("validate -> serde_json::to_vec_pretty -> fsio::atomic_write(path, bytes, T::FILE_MODE)")
    }

    /// Lock → load → mutate → save → unlock. The closure's return value is passed through.
    /// Use this for every modification so two processes cannot lose each other's writes.
    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R> {
        todo!("FileLock::acquire(path, lock_timeout); load; f(&mut doc); save")
    }
}
