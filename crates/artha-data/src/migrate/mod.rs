//! Namespaced, forward-only schema migrations.
//!
//! Each crate that persists data in `artha.db` owns a **namespace** (`core`,
//! `jobs`, `memory`, `portfolio`, `monitor`, …) and exports one
//! `pub static <NAME>_MIGRATIONS: MigrationSet`. Rules:
//!
//! 1. Namespace matches `[a-z][a-z0-9_]*`; every table/index/trigger it
//!    creates is prefixed `<namespace>_` (checked in tests via
//!    `testing::assert_objects_prefixed`).
//! 2. Versions are contiguous from 1. Each migration is SQL text, usually
//!    `include_str!("../migrations/<ns>/NNNN_name.sql")`.
//! 3. Applied migrations are immutable: the SHA-256 of the SQL is recorded in
//!    `_artha_migrations` and re-verified on every open.
//! 4. Foreign keys may point into the same namespace or into `core` only —
//!    never into another sibling — because `core` is always migrated first
//!    and sibling order is otherwise unspecified.
//! 5. No down-migrations. A pre-migration backup (`artha.db.bak-<ms>`) is the
//!    rollback story.
//!
//! Each pending migration runs in its own `BEGIN IMMEDIATE` transaction
//! together with its ledger insert, so a crash leaves it either fully applied
//! and recorded or not at all. Two processes starting simultaneously
//! serialize on the write lock; the loser re-reads the ledger and finds
//! nothing pending.

mod builtin;

pub use self::builtin::CORE_MIGRATIONS;

use rusqlite::Connection;

use crate::types::Timestamp;
use crate::Result;

/// The ledger table name. Created by the migrator itself, outside any namespace.
pub const LEDGER_TABLE: &str = "_artha_migrations";

/// DDL for the ledger (idempotent).
pub(crate) const LEDGER_DDL: &str = "\
CREATE TABLE IF NOT EXISTS _artha_migrations (
    namespace   TEXT    NOT NULL,
    version     INTEGER NOT NULL,
    name        TEXT    NOT NULL,
    checksum    TEXT    NOT NULL,
    applied_at  INTEGER NOT NULL,
    PRIMARY KEY (namespace, version)
) STRICT;";

/// One schema step.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// 1-based, contiguous within its set.
    pub version: u32,
    /// Short snake_case label (matches the SQL file name).
    pub name: &'static str,
    /// SQL batch executed with `execute_batch`.
    pub sql: &'static str,
}

/// All migrations owned by one namespace.
#[derive(Debug)]
pub struct MigrationSet {
    /// Owning namespace; also the required table-name prefix.
    pub namespace: &'static str,
    /// Ordered by `version`.
    pub migrations: &'static [Migration],
}

impl MigrationSet {
    /// `const` constructor for `static` declarations in sibling crates.
    pub const fn new(namespace: &'static str, migrations: &'static [Migration]) -> Self {
        Self { namespace, migrations }
    }

    /// Highest version in the set (0 if empty).
    pub fn latest_version(&self) -> u32 {
        todo!()
    }

    /// Check namespace syntax and version contiguity.
    pub fn validate(&self) -> Result<()> {
        todo!()
    }
}

/// A row of `_artha_migrations`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedMigration {
    /// Namespace.
    pub namespace: String,
    /// Version.
    pub version: u32,
    /// Name at time of application.
    pub name: String,
    /// Lowercase hex SHA-256 of the SQL.
    pub checksum: String,
    /// When it was applied.
    pub applied_at: Timestamp,
}

/// What a migration run did.
#[derive(Debug, Clone, Default)]
pub struct MigrationReport {
    /// `(namespace, version)` pairs applied in this run, in order.
    pub applied: Vec<(String, u32)>,
    /// Backup file written before applying, if any.
    pub backup: Option<std::path::PathBuf>,
}

/// Applies a list of [`MigrationSet`]s to a connection.
pub(crate) struct Migrator<'a> {
    sets: Vec<&'a MigrationSet>,
}

impl<'a> Migrator<'a> {
    /// `CORE_MIGRATIONS` is always prepended; duplicates are rejected.
    pub(crate) fn new(extra: &[&'a MigrationSet]) -> Result<Self> {
        todo!("validate each set; reject duplicate namespaces")
    }

    /// Pending `(set, migration)` pairs, after verifying checksums of applied ones
    /// and rejecting ledger rows unknown to this build.
    pub(crate) fn pending(&self, conn: &Connection) -> Result<Vec<(&'a MigrationSet, &'a Migration)>> {
        todo!()
    }

    /// Ensure ledger, compute pending, optionally back up, apply each in its own
    /// IMMEDIATE transaction (re-checking the ledger inside it).
    pub(crate) fn run(&self, conn: &mut Connection, backup_to: Option<&std::path::Path>) -> Result<MigrationReport> {
        todo!()
    }

    /// Read-only variant for `DbOptions::read_only`: errors if anything is pending.
    pub(crate) fn verify(&self, conn: &Connection) -> Result<()> {
        todo!()
    }
}

/// Lowercase hex SHA-256 of a migration's SQL text.
pub(crate) fn checksum(sql: &str) -> String {
    todo!("sha2::Sha256::digest(sql.as_bytes()) -> hex")
}
