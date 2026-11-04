//! Commit-message encoding of checkpoint metadata.
//!
//! ```text
//! <label>
//!
//! Artha-Kind: pre_edit
//! Artha-Tool: write_file
//! Artha-Tool-Call: toolu_01ABC
//! Artha-Session: 0192f1c4-...-...
//! ```
//!
//! Pure string functions — the most unit-testable part of the checkpoint module.

use crate::checkpoint::model::{CheckpointKind, EditContext};
use crate::store::SessionId;

/// Parsed trailers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Trailers {
    pub kind: Option<CheckpointKind>,
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
    pub session_id: Option<SessionId>,
}

/// Build the full commit message. Labels are reduced to one line and
/// truncated to 72 chars; trailer values have newlines stripped.
pub(crate) fn encode(kind: CheckpointKind, ctx: &EditContext) -> String {
    todo!()
}

/// Parse `(subject, trailers)` from a commit message; unknown trailers are ignored.
pub(crate) fn decode(message: &str) -> (String, Trailers) {
    todo!()
}
