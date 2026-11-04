//! The `~/.artha/` on-disk layout.
//!
//! [`ArthaPaths`] is the single source of truth for every path Artha writes.
//! No other crate should join path segments under the Artha root by hand.
//!
//! ```text
//! <root>/settings.json            Settings (mode 0600: contains api_key / session token)
//! <root>/data/artha.db            SQLite (+ -wal / -shm)
//! <root>/data/sidecars/*.py       optional Python sidecars (managed by artha-finance)
//! <root>/data/snapshot/<hash>/    bare checkpoint repo per launch directory
//! <root>/plugins/state.json       plugin registry document (schema owned by artha-plugins)
//! ```
//!
//! `<root>` is `$ARTHA_HOME` if set, else `~/.artha`. Constructors perform no
//! I/O; [`ArthaPaths::ensure_dirs`] creates directories on demand.

use std::path::{Path, PathBuf};

use crate::checkpoint::WorkdirHash;
use crate::Result;

/// Resolved Artha state root and derived paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArthaPaths {
    root: PathBuf,
}

/// What [`ArthaPaths::purge`] removed.
#[derive(Debug, Clone, Default)]
pub struct PurgeReport {
    /// Paths removed (files or directory trees).
    pub removed: Vec<PathBuf>,
    /// Total bytes freed (best effort).
    pub bytes_freed: u64,
}

impl ArthaPaths {
    /// Environment variable that overrides the state root (used by tests,
    /// portable installs, and `artha doctor` diagnostics).
    pub const ENV_HOME: &'static str = "ARTHA_HOME";

    /// Resolve the root from `$ARTHA_HOME`, else `dirs::home_dir()/.artha`.
    pub fn discover() -> Result<Self> {
        todo!("env override, else dirs::home_dir().ok_or(DataError::NoHomeDir)?.join(\".artha\")")
    }

    /// Use an explicit root (tests, embedding). Performs no I/O.
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The state root itself.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<root>/settings.json`
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// `<root>/data`
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    /// `<root>/data/artha.db`
    pub fn database_file(&self) -> PathBuf {
        self.data_dir().join("artha.db")
    }

    /// `<root>/data/sidecars`
    pub fn sidecars_dir(&self) -> PathBuf {
        self.data_dir().join("sidecars")
    }

    /// `<root>/data/snapshot`
    pub fn snapshot_root(&self) -> PathBuf {
        self.data_dir().join("snapshot")
    }

    /// `<root>/data/snapshot/<hash>` — the bare checkpoint repo for one workdir.
    pub fn snapshot_dir(&self, hash: &WorkdirHash) -> PathBuf {
        self.snapshot_root().join(hash.as_str())
    }

    /// `<root>/plugins`
    pub fn plugins_dir(&self) -> PathBuf {
        self.root.join("plugins")
    }

    /// `<root>/plugins/state.json`
    pub fn plugin_state_file(&self) -> PathBuf {
        self.plugins_dir().join("state.json")
    }

    /// Create `root`, `data/`, `plugins/` if missing (mode 0700 on Unix).
    /// Does **not** create `snapshot/<hash>` — that is lazy, see `CheckpointStore`.
    pub fn ensure_dirs(&self) -> Result<()> {
        todo!("create_dir_all + set_permissions(0o700) on unix")
    }

    /// Delete all Artha state under the root (backs `artha purge`).
    ///
    /// Refuses to run if `root` is `/`, the home directory itself, or does not
    /// look like an Artha root (no `settings.json`, `data/` or `plugins/`).
    /// Callers must hold no open [`crate::Db`] on this root.
    pub fn purge(&self) -> Result<PurgeReport> {
        todo!("sanity checks, then remove_dir_all children, summing sizes")
    }
}
