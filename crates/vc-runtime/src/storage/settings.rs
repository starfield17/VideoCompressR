use super::atomic_json::{read_json, recover_corrupt, write_json_atomic};
use crate::error::RuntimeError;
use crate::platform::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use vc_core::EncodeSettings;

pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SettingsData {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(flatten)]
    settings: EncodeSettings,
}

const fn default_schema_version() -> u32 {
    SETTINGS_SCHEMA_VERSION
}

#[derive(Clone)]
pub struct SettingsStore {
    paths: AppPaths,
}

impl SettingsStore {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub fn path(&self) -> PathBuf {
        self.paths.config_dir.join("settings.json")
    }

    pub fn load(&self) -> Result<Option<EncodeSettings>, RuntimeError> {
        let path = self.path();
        if !path.exists() {
            return Ok(None);
        }
        match read_json::<SettingsData>(&path) {
            Ok(value) if value.schema_version <= SETTINGS_SCHEMA_VERSION => {
                Ok(Some(value.settings))
            }
            Ok(value) => Err(RuntimeError::Config(format!(
                "unsupported settings schema version: {}",
                value.schema_version
            ))),
            Err(error) => {
                recover_corrupt(&path)?;
                Err(error)
            }
        }
    }

    pub fn save(&self, settings: &EncodeSettings) -> Result<(), RuntimeError> {
        write_json_atomic(
            &self.path(),
            &SettingsData { schema_version: SETTINGS_SCHEMA_VERSION, settings: settings.clone() },
        )
    }
}
