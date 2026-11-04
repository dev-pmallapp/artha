//! Serde model for `settings.json`.
//!
//! Example (local mode):
//!
//! ```json
//! {
//!   "version": 1,
//!   "mode": "local",
//!   "llm": {
//!     "id": "openai-chat",
//!     "model": "qwen2.5-32b-instruct",
//!     "base_url": "http://127.0.0.1:8080/v1",
//!     "api_key": "sk-local"
//!   },
//!   "preferences": { "log_level": "info", "permission_mode": "ask" }
//! }
//! ```
//!
//! Unknown top-level keys are preserved via `extra` so a newer Artha's
//! settings survive a round-trip through an older one.

use serde::{Deserialize, Serialize};
use url::Url;

use crate::json_store::VersionedDocument;
use crate::fsio::FileMode;
use crate::types::Timestamp;
use crate::Result;

/// Root of `settings.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Document schema version (see [`VersionedDocument`]).
    #[serde(default = "Settings::current_version")]
    pub version: u32,

    /// Which backend `artha-llm` should construct.
    #[serde(default)]
    pub mode: Mode,

    /// Local-LLM connector. Required when `mode == Local`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm: Option<LlmConnector>,

    /// Hosted-backend session. Required when `mode == Cloud`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<CloudSession>,

    /// UI / behavior preferences.
    #[serde(default)]
    pub preferences: Preferences,

    /// Unknown keys, preserved verbatim on save.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Settings {
    fn current_version() -> u32 {
        <Self as VersionedDocument>::CURRENT_VERSION
    }

    /// The connector to use in local mode, if configured.
    pub fn local_connector(&self) -> Option<&LlmConnector> {
        self.llm.as_ref()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: Self::current_version(),
            mode: Mode::default(),
            llm: None,
            session: None,
            preferences: Preferences::default(),
            extra: serde_json::Map::new(),
        }
    }
}

impl VersionedDocument for Settings {
    const CURRENT_VERSION: u32 = 1;
    const FILE_MODE: FileMode = FileMode::Private;

    /// Checks: `llm.model` non-empty; `base_url` scheme is http/https;
    /// absence of `llm` in local mode is *allowed* (first-run state) — the CLI
    /// reports it as "not configured" rather than as a corrupt file.
    fn validate(&self) -> Result<()> {
        todo!()
    }
}

/// Backend selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Talk directly to a local/self-hosted endpoint via [`LlmConnector`].
    #[default]
    Local,
    /// Talk to the hosted backend using [`CloudSession`].
    Cloud,
}

/// The local-LLM connector contract:
/// `{ id: "anthropic-messages" | "openai-chat", model, base_url, api_key }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmConnector {
    /// Wire format.
    pub id: ConnectorKind,
    /// Model name passed through to the endpoint.
    pub model: String,
    /// Endpoint base URL (e.g. `http://localhost:11434/v1`).
    pub base_url: Url,
    /// Bearer/API key. May be empty for endpoints that do not check it.
    #[serde(default)]
    pub api_key: Secret,
}

/// Supported local wire formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConnectorKind {
    /// Anthropic Messages API shape.
    #[serde(rename = "anthropic-messages")]
    AnthropicMessages,
    /// OpenAI Chat Completions API shape.
    #[serde(rename = "openai-chat")]
    OpenAiChat,
}

/// Hosted-backend login state, written by the `/v1/sessions/*` auth flow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloudSession {
    /// Session token (sent instead of an API key).
    pub token: Secret,
    /// Logged-in account email, for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Token expiry, if the backend reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<Timestamp>,
    /// Backend base URL override (defaults to the project's hosted API).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base: Option<Url>,
}

/// User preferences that persist across runs. CLI flags override per-run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// Default `--log-level` (`error|warn|info|debug|trace`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    /// Default tool-permission behavior.
    pub permission_mode: PermissionMode,
    /// Whether file-editing tools record checkpoints (default true).
    pub checkpoints_enabled: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self { log_level: None, permission_mode: PermissionMode::Ask, checkpoints_enabled: true }
    }
}

/// Tool-permission behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Prompt before side-effecting tool calls.
    #[default]
    Ask,
    /// Equivalent of `--skip-permissions` / `--yolo`, persisted.
    Skip,
}

/// A string secret: serialized verbatim (the file is 0600), but redacted in
/// `Debug` so it never lands in logs. `artha-llm` converts it to its own
/// secret type at the boundary via [`Secret::expose`].
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// Wrap a secret value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the plaintext. Call only at the point of use.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// True if no secret is set.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() { "Secret(<empty>)" } else { "Secret(***)" })
    }
}
