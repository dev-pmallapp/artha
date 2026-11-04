//! [`SettingsStore`]: `settings.json` load/save on top of [`JsonStore`].

use crate::json_store::JsonStore;
use crate::paths::ArthaPaths;
use crate::settings::model::{CloudSession, LlmConnector, Settings};
use crate::Result;

/// Read/write access to `~/.artha/settings.json`.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    inner: JsonStore<Settings>,
}

impl SettingsStore {
    /// Bind to `paths.settings_file()`. No I/O.
    pub fn new(paths: &ArthaPaths) -> Self {
        Self { inner: JsonStore::new(paths.settings_file()) }
    }

    /// Load (missing file ⇒ defaults). Called once by `artha-cli` at startup.
    pub fn load(&self) -> Result<Settings> {
        self.inner.load()
    }

    /// Overwrite with `settings`.
    pub fn save(&self, settings: &Settings) -> Result<()> {
        self.inner.save(settings)
    }

    /// Locked read-modify-write.
    pub fn update<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> Result<R> {
        self.inner.update(f)
    }

    /// Set the local connector and switch to local mode.
    pub fn set_local_connector(&self, connector: LlmConnector) -> Result<()> {
        todo!("update(|s| {{ s.mode = Mode::Local; s.llm = Some(connector) }})")
    }

    /// Store a cloud session after login (does not change `mode`).
    pub fn set_cloud_session(&self, session: CloudSession) -> Result<()> {
        todo!()
    }

    /// Remove the cloud session (logout).
    pub fn clear_cloud_session(&self) -> Result<()> {
        todo!()
    }

    /// Underlying generic store.
    pub fn json_store(&self) -> &JsonStore<Settings> {
        &self.inner
    }
}
