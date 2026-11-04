//! Checkpoint/undo over files the agent edits, backed by a private git repo.
//!
//! # Layout
//!
//! One repo per launch directory: `~/.artha/data/snapshot/<WorkdirHash>/` is a
//! *bare* git directory whose worktree is set (`Repository::set_workdir`,
//! plus `core.worktree` in its config) to the canonical launch directory.
//! It is completely separate from any `.git` the user's project has — it
//! never reads or writes the user's repo, index, hooks, or config.
//!
//! # When commits happen
//!
//! **Never on startup or idle.** [`CheckpointStore::new`] does no I/O; the repo
//! is created on the first [`CheckpointStore::begin_edit`]. Each file-editing
//! tool call does:
//!
//! ```text
//! let guard = checkpoints.begin_edit(ctx, &[path])?;   // PreEdit commit iff those files differ from last snapshot
//! ... tool writes the file ...
//! guard.finish()?;                                      // PostEdit commit iff anything changed
//! ```
//!
//! # What is tracked
//!
//! Only paths that some tool has declared it will edit ("touched set"), not
//! the whole workdir — so a launch in `$HOME` does not snapshot gigabytes.
//! Each commit's tree = previous tree + current contents of the touched
//! files (deleted files are removed from the tree). Ignore files are not
//! consulted, since paths are explicit. Files larger than
//! [`CheckpointStore::MAX_FILE_BYTES`] are skipped and reported.
//!
//! # History
//!
//! Linear history on `refs/heads/checkpoints`. Metadata lives in commit
//! message trailers (`Artha-Kind`, `Artha-Session`, `Artha-Tool-Call`,
//! `Artha-Tool`), so the repo is self-describing even without `artha.db`.
//! Author/committer is a fixed `Artha <artha@localhost>` identity; no user
//! git config is needed or read.
//!
//! # Concurrency
//!
//! Sync API (libgit2 is blocking); async callers use `spawn_blocking`.
//! In-process: the `git2::Repository` sits behind a `Mutex`. Cross-process:
//! every mutating operation holds a [`FileLock`](crate::FileLock) on
//! `snapshot/<hash>.lock`.

mod hash;
mod model;
mod repo;
mod trailers;

pub use hash::WorkdirHash;
pub use model::{
    ChangeStatus, Checkpoint, CheckpointDiff, CheckpointFilter, CheckpointId, CheckpointKind,
    EditContext, FileChange, RestoreReport, RestoreScope,
};

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::paths::ArthaPaths;
use crate::Result;

/// Checkpoint repository for one workdir. `Send + Sync`; share via `Arc`.
pub struct CheckpointStore {
    workdir: PathBuf,
    hash: WorkdirHash,
    git_dir: PathBuf,
    /// `None` until first use (lazy init).
    repo: Mutex<Option<git2::Repository>>,
}

impl std::fmt::Debug for CheckpointStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckpointStore")
            .field("workdir", &self.workdir)
            .field("git_dir", &self.git_dir)
            .finish_non_exhaustive()
    }
}

impl CheckpointStore {
    /// Files above this size are not snapshotted (reported in `Checkpoint::skipped`).
    pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

    /// Ref holding the checkpoint history.
    pub const REF_NAME: &'static str = "refs/heads/checkpoints";

    /// Bind to `workdir` (canonicalized). **No filesystem writes.**
    pub fn new(paths: &ArthaPaths, workdir: &Path) -> Result<Self> {
        todo!("canonicalize; WorkdirHash::of; git_dir = paths.snapshot_dir(&hash)")
    }

    /// Canonical worktree path.
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// The bare repo directory (`snapshot/<hash>`).
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// The workdir's hash.
    pub fn workdir_hash(&self) -> &WorkdirHash {
        &self.hash
    }

    /// Whether the repo exists on disk (cheap; no init).
    pub fn is_initialized(&self) -> bool {
        todo!()
    }

    /// Start an edit: validate that every path is inside the workdir, add them
    /// to the touched set, and record a `PreEdit` checkpoint if their current
    /// content differs from the latest checkpoint (always on first touch, so
    /// the original content is captured). Initializes the repo if needed.
    pub fn begin_edit(&self, ctx: EditContext, files: &[PathBuf]) -> Result<EditGuard<'_>> {
        todo!()
    }

    /// Record a checkpoint of `files` right now. Returns `None` if nothing
    /// changed relative to the latest checkpoint (no empty commits).
    pub fn record(&self, ctx: &EditContext, kind: CheckpointKind, files: &[PathBuf]) -> Result<Option<Checkpoint>> {
        todo!()
    }

    /// Checkpoints newest-first. Empty (not an error) if the repo was never initialized.
    pub fn list(&self, filter: &CheckpointFilter) -> Result<Vec<Checkpoint>> {
        todo!()
    }

    /// One checkpoint by id.
    pub fn get(&self, id: &CheckpointId) -> Result<Option<Checkpoint>> {
        todo!()
    }

    /// Diff of the checkpoint's tree against the *current* worktree, limited to tracked paths.
    pub fn diff(&self, id: &CheckpointId) -> Result<CheckpointDiff> {
        todo!()
    }

    /// Restore files to their state at `id`. First records a `PreRestore`
    /// checkpoint of current state, so a restore is itself undoable. Files
    /// tracked now but absent in `id`'s tree (created after it) are deleted.
    pub fn restore(&self, id: &CheckpointId, scope: &RestoreScope) -> Result<RestoreReport> {
        todo!()
    }

    /// Delete the snapshot repo for this workdir (e.g. `/checkpoints clear`).
    pub fn destroy(self) -> Result<()> {
        todo!()
    }
}

/// RAII marker for an in-progress edit. Call [`EditGuard::finish`] after the
/// tool's write completes. Dropping without `finish` (tool errored or
/// panicked) records nothing further — the `PreEdit` baseline already
/// guarantees the original content is recoverable — and logs at `debug`.
#[must_use = "call finish() after the edit to record the post-edit checkpoint"]
pub struct EditGuard<'a> {
    store: &'a CheckpointStore,
    ctx: EditContext,
    files: Vec<PathBuf>,
    pre: Option<Checkpoint>,
    finished: bool,
}

impl<'a> EditGuard<'a> {
    /// The `PreEdit` checkpoint taken by `begin_edit`, if one was needed.
    pub fn pre_edit(&self) -> Option<&Checkpoint> {
        self.pre.as_ref()
    }

    /// Record the `PostEdit` checkpoint. Returns `(pre, post)`.
    pub fn finish(self) -> Result<(Option<Checkpoint>, Option<Checkpoint>)> {
        todo!()
    }
}

impl Drop for EditGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            tracing::debug!(files = ?self.files, tool = %self.ctx.tool_name, "edit guard dropped without finish");
        }
        let _ = self.store;
    }
}
