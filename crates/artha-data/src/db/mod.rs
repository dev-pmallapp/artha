//! The shared `artha.db` handle.
//!
//! # Concurrency model
//!
//! - **One `rusqlite::Connection` per process**, behind a `std::sync::Mutex`,
//!   inside an `Arc` ([`Db`] is `Clone + Send + Sync`). Artha's write volume is
//!   tiny (chat messages, job state, a few memory rows) and every statement is
//!   sub-millisecond, so a pool buys nothing but complexity and would make
//!   `BEGIN IMMEDIATE` job-claiming semantics harder to reason about.
//! - **WAL journal mode + `busy_timeout`** so *multiple processes* (a TUI
//!   session plus `artha jobs tick` from system cron plus `artha ask`) can
//!   share the file: readers never block writers, writers wait up to the
//!   timeout for each other.
//! - **Sync API.** Closures run on the caller's thread while holding the
//!   mutex. From async code, use [`Db::call`] (feature `async`), which moves
//!   the closure onto tokio's blocking pool so the reactor is never stalled.
//! - Mutex poisoning is recovered (`into_inner`): a panic inside a closure
//!   drops the open `Transaction`, which rolls back, leaving the connection
//!   valid.
//!
//! # Extension by sibling crates
//!
//! Siblings pass their [`MigrationSet`]s to [`Db::open`] and then use
//! [`Db::read`] / [`Db::write`] with plain `rusqlite` (re-exported as
//! `artha_data::rusqlite`). Their closure error type `E` only needs
//! `From<DataError>`, so they keep their own error enums.

mod pragmas;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{Connection, Transaction};

use crate::migrate::{AppliedMigration, MigrationReport, MigrationSet};
use crate::{DataError, Result};

/// Options for [`Db::open`].
#[derive(Debug, Clone)]
pub struct DbOptions {
    /// How long a writer waits for another process's write lock (SQLite `busy_timeout`).
    pub busy_timeout: Duration,
    /// Open read-only (e.g. `artha doctor` without `--fix`). Migrations are
    /// *checked* but not applied; pending ones produce an error.
    pub read_only: bool,
    /// Copy the DB to `artha.db.bak-<unix_ms>` via the SQLite backup API
    /// before applying any pending migration.
    pub backup_before_migrate: bool,
}

impl Default for DbOptions {
    fn default() -> Self {
        Self {
            busy_timeout: Duration::from_secs(5),
            read_only: false,
            backup_before_migrate: true,
        }
    }
}

/// Cloneable, thread-safe handle to the Artha database.
#[derive(Debug, Clone)]
pub struct Db {
    inner: Arc<DbInner>,
}

#[derive(Debug)]
struct DbInner {
    conn: Mutex<Connection>,
    /// `None` for in-memory databases.
    path: Option<PathBuf>,
}

impl Db {
    /// Open (creating if needed) the database at `path`, apply connection
    /// pragmas, then run migrations: [`crate::CORE_MIGRATIONS`] first, then
    /// each of `schemas` in the order given.
    ///
    /// `artha-cli` is the composition root: it collects every crate's
    /// `MigrationSet` and calls this once per process.
    pub fn open(path: &Path, options: DbOptions, schemas: &[&'static MigrationSet]) -> Result<Self> {
        todo!("create parent dir; Connection::open_with_flags; pragmas::apply; migrate::Migrator::new(core + schemas).run")
    }

    /// In-memory database with the same migrations (unit tests).
    pub fn open_in_memory(schemas: &[&'static MigrationSet]) -> Result<Self> {
        todo!()
    }

    /// Filesystem path, or `None` when in-memory.
    pub fn path(&self) -> Option<&Path> {
        self.inner.path.as_deref()
    }

    /// Run `f` inside a **deferred** transaction (read snapshot). The
    /// transaction is committed (a no-op for pure reads) if `f` returns `Ok`.
    pub fn read<R, E>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<R, E>) -> Result<R, E>
    where
        E: From<DataError>,
    {
        todo!("lock; conn.transaction_with_behavior(Deferred); f(&tx); tx.commit()")
    }

    /// Run `f` inside a `BEGIN IMMEDIATE` transaction: the write lock is taken
    /// up front, so read-then-write logic (e.g. claiming the next due job)
    /// cannot race another process. Commits on `Ok`, rolls back on `Err`/panic.
    pub fn write<R, E>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<R, E>) -> Result<R, E>
    where
        E: From<DataError>,
    {
        todo!("lock; conn.transaction_with_behavior(Immediate); f(&tx); tx.commit()")
    }

    /// Raw connection access without a transaction (pragmas, `VACUUM`).
    pub fn with_conn<R, E>(&self, f: impl FnOnce(&mut Connection) -> Result<R, E>) -> Result<R, E>
    where
        E: From<DataError>,
    {
        todo!()
    }

    /// Async wrapper: runs `f` via [`Db::write`] on `tokio::task::spawn_blocking`.
    /// A panicked/cancelled task maps to [`DataError::TaskJoin`].
    #[cfg(feature = "async")]
    pub async fn call<R, E>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> Result<R, E> + Send + 'static,
    ) -> Result<R, E>
    where
        R: Send + 'static,
        E: From<DataError> + Send + 'static,
    {
        todo!("let db = self.clone(); spawn_blocking(move || db.write(f)).await")
    }

    /// Ledger rows from `_artha_migrations`, for `artha doctor`.
    pub fn applied_migrations(&self) -> Result<Vec<AppliedMigration>> {
        todo!()
    }

    /// `PRAGMA integrity_check` + `PRAGMA foreign_key_check`; empty vec = healthy.
    pub fn health_check(&self) -> Result<Vec<String>> {
        todo!()
    }

    /// Re-run migrations for late-registered sets (rare; e.g. a plugin-provided schema).
    pub fn migrate_additional(&self, schemas: &[&'static MigrationSet]) -> Result<MigrationReport> {
        todo!()
    }
}
