//! `~/.artha/settings.json`: model ([`model`]) and store ([`store`]).
//!
//! The model is plain serde data with no behavior beyond validation; turning
//! an [`LlmConnector`] into a live provider is `artha-llm`'s job, and turning
//! CLI flags/env vars into overrides is `artha-cli`'s job. This crate never
//! reads environment variables for settings values.

mod model;
mod store;

pub use model::{
    CloudSession, ConnectorKind, LlmConnector, Mode, PermissionMode, Preferences, Secret, Settings,
};
pub use store::SettingsStore;
