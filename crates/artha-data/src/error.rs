//! Error type for all of `artha-data`.
//!
//! One crate-wide enum, built with `thiserror`, `#[non_exhaustive]` so new
//! variants are not breaking. Variants carry the *path* or *namespace* that
//! failed, because "permission denied" without a filename is useless in
//! `artha doctor` output. Sibling crates wrap [`DataError`] in their own
//! error types via `#[from]`; `artha-cli` is the only place it becomes
//! `anyhow::Error`.

use std::path::PathBuf;

/// Convenience alias used throughout the crate.
pub type Result<T, E = DataError> = std::result::Result<T, E>;

/// Every failure `artha-data` can report.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DataError {
    /// Filesystem error, annotated with the path involved.
    #[error("I/O error at {path}: {source}")]
    Io {
        /// Path being read, written, created or removed.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// A JSON document (settings.json, plugins/state.json) failed to parse or serialize.
    #[error("invalid JSON in {path}: {source}")]
    Json {
        /// The document's path.
        path: PathBuf,
        /// Underlying serde error (includes line/column).
        #[source]
        source: serde_json::Error,
    },

    /// A JSON document declares a `version` newer than this binary understands.
    #[error("{path} has version {found}, but this build supports at most {supported}; upgrade Artha")]
    UnsupportedDocumentVersion {
        /// The document's path.
        path: PathBuf,
        /// Version found on disk.
        found: u32,
        /// Highest version this build can read.
        supported: u32,
    },

    /// Settings parsed but are semantically invalid (e.g. empty model name).
    #[error("invalid settings: {0}")]
    InvalidSettings(String),

    /// Raw SQLite error.
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// A schema migration could not be applied or verified.
    #[error("migration {namespace}#{version}: {kind}")]
    Migration {
        /// Owning namespace (e.g. `core`, `jobs`).
        namespace: String,
        /// Migration version within the namespace.
        version: u32,
        /// What went wrong.
        kind: MigrationErrorKind,
    },

    /// Raw libgit2 error from the checkpoint repo.
    #[error("checkpoint repository error: {0}")]
    Git(#[from] git2::Error),

    /// A checkpoint operation was rejected (e.g. path outside the workdir).
    #[error("checkpoint: {0}")]
    Checkpoint(String),

    /// The user's home directory could not be determined and `ARTHA_HOME` is unset.
    #[error("cannot determine home directory; set ARTHA_HOME")]
    NoHomeDir,

    /// A requested row/checkpoint/document does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// A non-blocking lock attempt found the lock held by another process.
    #[error("{0} is locked by another Artha process")]
    Locked(PathBuf),

    /// A blocking-pool task panicked or was cancelled (async feature).
    #[error("background database task failed: {0}")]
    TaskJoin(String),
}

/// Detail for [`DataError::Migration`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MigrationErrorKind {
    /// The SQL text of an already-applied migration changed since it was applied.
    #[error("checksum mismatch: applied migration was edited after release (recorded {recorded}, compiled {compiled})")]
    ChecksumMismatch {
        /// Checksum stored in the ledger.
        recorded: String,
        /// Checksum of the SQL compiled into this binary.
        compiled: String,
    },
    /// The database has a migration this binary does not know about (DB from a newer Artha).
    #[error("database is newer than this build (unknown applied migration); upgrade Artha")]
    UnknownApplied,
    /// Versions in a `MigrationSet` are not 1, 2, 3, … without gaps or duplicates.
    #[error("migration versions must be contiguous starting at 1")]
    NonContiguous,
    /// Namespace is not `[a-z][a-z0-9_]*` or is registered twice.
    #[error("invalid or duplicate namespace")]
    BadNamespace,
    /// The migration SQL itself failed.
    #[error("SQL failed: {0}")]
    Sql(#[source] rusqlite::Error),
}

impl DataError {
    /// Helper for building [`DataError::Io`] with a path, for `map_err`.
    pub fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> DataError {
        let path = path.into();
        move |source| DataError::Io { path, source }
    }

    /// True for SQLite `SQLITE_BUSY`/`SQLITE_LOCKED` after the busy timeout
    /// expired — callers (e.g. `artha-jobs` tick) may retry or skip the cycle.
    pub fn is_busy(&self) -> bool {
        todo!("match Sqlite(SqliteFailure(e, _)) where e.code is DatabaseBusy | DatabaseLocked")
    }
}
