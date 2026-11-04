//! artha-data — settings file, SQLite persistence, and checkpoint repository access.
//!
//! This crate owns *where* and *how* Artha's local state is stored; it does not
//! own the business meaning of most of that state. Concretely:
//!
//! - [`paths`] — the `~/.artha/` layout ([`ArthaPaths`]), including `ARTHA_HOME`
//!   override and `artha purge` support.
//! - [`fsio`] + [`json_store`] — crash-safe, lock-protected JSON documents
//!   ([`JsonStore`]) used for `settings.json` and (with a schema owned by
//!   `artha-plugins`) `plugins/state.json`.
//! - [`settings`] — the `settings.json` model ([`Settings`], [`LlmConnector`]).
//! - [`db`] + [`migrate`] — the shared `artha.db` handle ([`Db`]) and a
//!   namespaced, forward-only migration runner that lets sibling crates ship
//!   their own tables ([`MigrationSet`]).
//! - [`store`] — repositories for the tables this crate owns itself (chat
//!   sessions/messages, generic key-value, checkpoint links).
//! - [`checkpoint`] — the lazily-initialized per-workdir snapshot git repo used
//!   for checkpoint/undo of agent file edits ([`CheckpointStore`]).
//!
//! The API is synchronous. Async callers use [`Db::call`] (feature `async`) or
//! `tokio::task::spawn_blocking`. See `docs/HLD.md` and `docs/LLD.md`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
// Design-phase skeleton: bodies are `todo!()`. Remove once implemented.
#![allow(unused_variables, dead_code, unused_imports)]

pub mod checkpoint;
pub mod db;
pub mod error;
pub mod fsio;
pub mod json_store;
pub mod migrate;
pub mod paths;
pub mod settings;
pub mod store;
pub mod types;

#[cfg(any(test, feature = "test-util"))]
pub mod testing;

pub use checkpoint::{
    ChangeStatus, Checkpoint, CheckpointDiff, CheckpointFilter, CheckpointId, CheckpointKind,
    CheckpointStore, EditContext, EditGuard, FileChange, RestoreReport, RestoreScope, WorkdirHash,
};
pub use db::{Db, DbOptions};
pub use error::{DataError, MigrationErrorKind, Result};
pub use fsio::{FileLock, FileMode};
pub use json_store::{JsonStore, VersionedDocument};
pub use migrate::{AppliedMigration, Migration, MigrationReport, MigrationSet, CORE_MIGRATIONS};
pub use paths::{ArthaPaths, PurgeReport};
pub use settings::{
    CloudSession, ConnectorKind, LlmConnector, Mode, PermissionMode, Preferences, Secret, Settings,
    SettingsStore,
};
pub use store::{
    CheckpointLink, CheckpointLinkStore, KvStore, MessageSeq, NewMessage, NewSession, Role,
    Session, SessionFilter, SessionId, SessionStore, SessionSummary, StoredMessage,
};
pub use types::Timestamp;

/// Re-export of the exact `rusqlite` version this crate links, so sibling
/// crates writing their own queries against [`Db`] never pull in a second,
/// incompatible copy (two `libsqlite3-sys` versions cannot link together).
pub use rusqlite;
