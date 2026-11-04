//! Stable directory name for a workdir's snapshot repo.
//!
//! `WorkdirHash = hex(sha256(canonical_path_bytes))[..16]` — 64 bits is
//! ample for the number of directories one user launches from. The full
//! canonical path is also written to the repo config (`artha.workdir`) and
//! checked on open, so a collision or a moved `~/.artha` is detected rather
//! than silently mixing histories. On Windows the path is lowercased before
//! hashing (case-insensitive filesystem).

use std::path::Path;

use serde::{Deserialize, Serialize};

/// 16-hex-char workdir identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkdirHash(String);

impl WorkdirHash {
    /// Length in hex characters.
    pub const LEN: usize = 16;

    /// Hash an already-canonicalized path.
    pub fn of(canonical_workdir: &Path) -> Self {
        todo!("sha2::Sha256 over OsStr bytes (unix) / UTF-16 lowercased (windows); hex; truncate")
    }

    /// Wrap an existing hash string (e.g. from `core_checkpoint_link`); validates length/charset.
    pub fn parse(s: &str) -> crate::Result<Self> {
        todo!()
    }

    /// The hex string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
