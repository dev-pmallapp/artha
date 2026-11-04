//! Public value types for the checkpoint API. No `git2` types leak out:
//! [`CheckpointId`] wraps an object id as a hex string at the boundary.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use crate::store::SessionId;
use crate::types::Timestamp;

/// A checkpoint commit id (40-hex SHA-1 object id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CheckpointId(pub(crate) git2::Oid);

impl CheckpointId {
    /// First 10 hex chars, for display.
    pub fn short(&self) -> String {
        todo!()
    }
}

impl fmt::Display for CheckpointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for CheckpointId {
    type Err = crate::DataError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        todo!("git2::Oid::from_str; full ids only — prefix lookup goes through CheckpointStore::get")
    }
}

/// Why a checkpoint was taken (stored as the `Artha-Kind` trailer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckpointKind {
    /// Before a tool modified files.
    PreEdit,
    /// After a tool modified files.
    PostEdit,
    /// Safety snapshot taken right before a restore.
    PreRestore,
}

impl CheckpointKind {
    /// Trailer / SQL text form (`pre_edit`, `post_edit`, `pre_restore`).
    pub fn as_str(self) -> &'static str {
        match self {
            CheckpointKind::PreEdit => "pre_edit",
            CheckpointKind::PostEdit => "post_edit",
            CheckpointKind::PreRestore => "pre_restore",
        }
    }
}

/// Who/what is editing — becomes commit trailers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditContext {
    /// Chat session, if the edit came from one (jobs may have none).
    pub session_id: Option<SessionId>,
    /// LLM tool-call id.
    pub tool_call_id: Option<String>,
    /// Tool name, e.g. `write_file`.
    pub tool_name: String,
    /// One-line human label for the commit subject.
    pub label: String,
}

/// A recorded checkpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct Checkpoint {
    /// Commit id.
    pub id: CheckpointId,
    /// Previous checkpoint.
    pub parent: Option<CheckpointId>,
    /// Kind (from trailer).
    pub kind: CheckpointKind,
    /// Commit subject.
    pub label: String,
    /// Session trailer.
    pub session_id: Option<SessionId>,
    /// Tool-call trailer.
    pub tool_call_id: Option<String>,
    /// Tool-name trailer.
    pub tool_name: Option<String>,
    /// Workdir-relative paths changed by this commit vs its parent.
    pub files: Vec<PathBuf>,
    /// Paths requested but not snapshotted (too large / unreadable).
    pub skipped: Vec<PathBuf>,
    /// Commit time.
    pub created_at: Timestamp,
}

/// Filter for `CheckpointStore::list`.
#[derive(Debug, Clone, Default)]
pub struct CheckpointFilter {
    /// Only checkpoints from this session.
    pub session_id: Option<SessionId>,
    /// Only checkpoints touching this workdir-relative path.
    pub path: Option<PathBuf>,
    /// Max results (0 = unlimited).
    pub limit: usize,
}

/// Which files a restore applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreScope {
    /// Every tracked path.
    All,
    /// Only these workdir-relative paths.
    Files(Vec<PathBuf>),
}

/// Result of a restore.
#[derive(Debug, Clone, Default)]
pub struct RestoreReport {
    /// Files written back.
    pub restored: Vec<PathBuf>,
    /// Files deleted because they did not exist at the checkpoint.
    pub deleted: Vec<PathBuf>,
    /// The `PreRestore` safety checkpoint (undo-the-undo).
    pub safety_checkpoint: Option<CheckpointId>,
}

/// Per-file change status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    /// Exists now, not at the checkpoint.
    Added,
    /// Content differs.
    Modified,
    /// Existed at the checkpoint, gone now.
    Deleted,
}

/// One file in a [`CheckpointDiff`].
#[derive(Debug, Clone, PartialEq)]
pub struct FileChange {
    /// Workdir-relative path.
    pub path: PathBuf,
    /// Status.
    pub status: ChangeStatus,
    /// Unified diff text; `None` for binary files.
    pub patch: Option<String>,
}

/// Checkpoint vs current worktree.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CheckpointDiff {
    /// Changed files, sorted by path.
    pub files: Vec<FileChange>,
}
