//! Chat sessions and message history (`core_session`, `core_message`).
//!
//! Message `content` is opaque JSON: `artha-core` serializes its own message
//! type (text blocks, tool calls, tool results) into it. This keeps
//! `artha-data` free of any dependency on `artha-llm`/`artha-core` types and
//! lets the message shape evolve without schema migrations.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::Db;
use crate::settings::Mode;
use crate::types::Timestamp;
use crate::Result;

/// Session identifier (UUIDv7: time-ordered, so `ORDER BY id` ≈ creation order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub Uuid);

impl SessionId {
    /// Fresh v7 id.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for SessionId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

/// 0-based position of a message within its session.
pub type MessageSeq = u32;

/// Message author role (mirrors the `CHECK` constraint on `core_message.role`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// System prompt.
    System,
    /// Human turn.
    User,
    /// Model turn (may contain tool calls).
    Assistant,
    /// Tool result.
    Tool,
}

impl Role {
    /// SQL text form.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// A `core_session` row.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    /// Primary key.
    pub id: SessionId,
    /// User- or model-assigned title.
    pub title: Option<String>,
    /// Canonical directory Artha was launched from.
    pub workdir: PathBuf,
    /// Backend mode at creation.
    pub mode: Mode,
    /// Connector id / `"cloud"`.
    pub provider: Option<String>,
    /// Model name.
    pub model: Option<String>,
    /// Session this one was forked from.
    pub parent_id: Option<SessionId>,
    /// Hidden from default listings.
    pub archived: bool,
    /// Free-form JSON for core-level extras (e.g. system-prompt hash).
    pub metadata: serde_json::Value,
    /// Creation time.
    pub created_at: Timestamp,
    /// Last message append / rename time.
    pub updated_at: Timestamp,
}

/// Input for [`SessionStore::create`].
#[derive(Debug, Clone)]
pub struct NewSession {
    /// Optional initial title.
    pub title: Option<String>,
    /// Launch directory (canonicalized by the store).
    pub workdir: PathBuf,
    /// Backend mode.
    pub mode: Mode,
    /// Connector id / `"cloud"`.
    pub provider: Option<String>,
    /// Model name.
    pub model: Option<String>,
    /// Fork parent.
    pub parent_id: Option<SessionId>,
    /// Initial metadata (`{}` if `Null`).
    pub metadata: serde_json::Value,
}

/// Listing row for `--resume` pickers.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSummary {
    /// Session.
    pub session: Session,
    /// Number of messages.
    pub message_count: u32,
    /// First user message, truncated, for display when `title` is `None`.
    pub preview: Option<String>,
}

/// Filter for [`SessionStore::list`].
#[derive(Debug, Clone)]
pub struct SessionFilter {
    /// Restrict to one launch directory.
    pub workdir: Option<PathBuf>,
    /// Include archived sessions.
    pub include_archived: bool,
    /// Max rows (newest first).
    pub limit: u32,
}

impl Default for SessionFilter {
    fn default() -> Self {
        Self { workdir: None, include_archived: false, limit: 50 }
    }
}

/// A `core_message` row.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredMessage {
    /// Rowid.
    pub id: i64,
    /// Owning session.
    pub session_id: SessionId,
    /// Position in session.
    pub seq: MessageSeq,
    /// Author.
    pub role: Role,
    /// Opaque message body owned by artha-core.
    pub content: serde_json::Value,
    /// Prompt tokens, if reported.
    pub input_tokens: Option<u32>,
    /// Completion tokens, if reported.
    pub output_tokens: Option<u32>,
    /// Insert time.
    pub created_at: Timestamp,
}

/// Input for [`SessionStore::append`].
#[derive(Debug, Clone)]
pub struct NewMessage {
    /// Author.
    pub role: Role,
    /// Opaque message body.
    pub content: serde_json::Value,
    /// Prompt tokens.
    pub input_tokens: Option<u32>,
    /// Completion tokens.
    pub output_tokens: Option<u32>,
}

/// Repository for sessions and messages.
#[derive(Debug, Clone)]
pub struct SessionStore {
    db: Db,
}

impl SessionStore {
    /// Wrap a database handle.
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Insert a session with a fresh id.
    pub fn create(&self, new: NewSession) -> Result<Session> {
        todo!()
    }

    /// Fetch one session.
    pub fn get(&self, id: SessionId) -> Result<Option<Session>> {
        todo!()
    }

    /// Newest non-archived session in `workdir` (backs `-c/--continue`).
    pub fn most_recent(&self, workdir: &Path) -> Result<Option<Session>> {
        todo!()
    }

    /// Sessions newest-first (backs `-r/--resume`).
    pub fn list(&self, filter: &SessionFilter) -> Result<Vec<SessionSummary>> {
        todo!()
    }

    /// Set or clear the title.
    pub fn rename(&self, id: SessionId, title: Option<&str>) -> Result<()> {
        todo!()
    }

    /// Archive / unarchive.
    pub fn set_archived(&self, id: SessionId, archived: bool) -> Result<()> {
        todo!()
    }

    /// Delete a session and (via `ON DELETE CASCADE`) its messages.
    pub fn delete(&self, id: SessionId) -> Result<()> {
        todo!()
    }

    /// Append messages atomically, assigning consecutive `seq` values after
    /// the current max, and bump `updated_at`. One transaction per call so a
    /// whole assistant turn (assistant msg + tool results) lands together.
    pub fn append(&self, id: SessionId, messages: &[NewMessage]) -> Result<Vec<StoredMessage>> {
        todo!()
    }

    /// All messages in order.
    pub fn messages(&self, id: SessionId) -> Result<Vec<StoredMessage>> {
        todo!()
    }

    /// Messages with `seq > after` (incremental reload).
    pub fn messages_after(&self, id: SessionId, after: MessageSeq) -> Result<Vec<StoredMessage>> {
        todo!()
    }

    /// Delete messages with `seq > keep_through`; returns rows removed (rewind).
    pub fn truncate_after(&self, id: SessionId, keep_through: MessageSeq) -> Result<u64> {
        todo!()
    }

    /// Copy a session's first `through + 1` messages into a new child session.
    pub fn fork(&self, id: SessionId, through: MessageSeq) -> Result<Session> {
        todo!()
    }
}
