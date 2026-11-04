//! Index from conversation position to checkpoint commit (`core_checkpoint_link`).
//!
//! Written by `artha-core` right after `CheckpointStore` returns a
//! checkpoint; read when the user rewinds to a message. Kept separate from
//! [`crate::checkpoint`] so the git layer has no database dependency.

use crate::checkpoint::{CheckpointId, CheckpointKind, WorkdirHash};
use crate::db::Db;
use crate::store::session::{MessageSeq, SessionId};
use crate::types::Timestamp;
use crate::Result;

/// A `core_checkpoint_link` row.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckpointLink {
    /// Commit in the snapshot repo.
    pub checkpoint: CheckpointId,
    /// Which snapshot repo.
    pub workdir_hash: WorkdirHash,
    /// Owning session, if any.
    pub session_id: Option<SessionId>,
    /// Message the edit belongs to.
    pub message_seq: Option<MessageSeq>,
    /// Tool call that caused it.
    pub tool_call_id: Option<String>,
    /// Pre-edit / post-edit / pre-restore.
    pub kind: CheckpointKind,
    /// When recorded.
    pub created_at: Timestamp,
}

/// Repository for checkpoint links.
#[derive(Debug, Clone)]
pub struct CheckpointLinkStore {
    db: Db,
}

impl CheckpointLinkStore {
    /// Wrap a database handle.
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Insert (idempotent on `commit_oid`).
    pub fn insert(&self, link: &CheckpointLink) -> Result<()> {
        todo!()
    }

    /// Earliest `PreEdit` checkpoint at or after `seq` in a session — the
    /// state to restore when rewinding the conversation to just before `seq`.
    pub fn first_pre_edit_from(&self, session: SessionId, seq: MessageSeq) -> Result<Option<CheckpointLink>> {
        todo!()
    }

    /// All links for a session, oldest first.
    pub fn for_session(&self, session: SessionId) -> Result<Vec<CheckpointLink>> {
        todo!()
    }
}
