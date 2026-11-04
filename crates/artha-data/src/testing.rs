//! Test helpers, available to this crate's tests and (with feature
//! `test-util`) to sibling crates' `[dev-dependencies]`.
//!
//! Every helper is backed by a fresh `tempfile::TempDir`, so tests never
//! touch the real `~/.artha` and can run in parallel without `ARTHA_HOME`
//! (environment variables are process-global; tests must not set them).

use std::path::PathBuf;

use tempfile::TempDir;

use crate::db::Db;
use crate::migrate::MigrationSet;
use crate::paths::ArthaPaths;
use crate::Result;

/// An isolated Artha root plus a separate fake launch directory.
#[derive(Debug)]
pub struct TestEnv {
    /// Owns the temp tree; deleted on drop.
    pub dir: TempDir,
    /// Paths rooted at `dir/home/.artha`.
    pub paths: ArthaPaths,
    /// `dir/work` — use as the checkpoint workdir.
    pub workdir: PathBuf,
}

impl TestEnv {
    /// Create the temp tree (`ensure_dirs` already called).
    pub fn new() -> Result<Self> {
        todo!()
    }

    /// File-backed DB at `paths.database_file()` with the given extra schemas.
    pub fn db(&self, schemas: &[&'static MigrationSet]) -> Result<Db> {
        todo!()
    }
}

/// Assert every table/index/trigger/view created by `set` (as listed in
/// `sqlite_schema` after applying it alone to an empty DB) is prefixed
/// `<namespace>_`. Sibling crates call this from a unit test on their set.
pub fn assert_objects_prefixed(set: &'static MigrationSet) {
    todo!()
}

/// Apply `set` to a fresh in-memory DB twice (second run must be a no-op) and
/// check the ledger. Sibling crates call this from a unit test on their set.
pub fn assert_migrations_idempotent(set: &'static MigrationSet) {
    todo!()
}
