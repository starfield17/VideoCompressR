use super::atomic_json::{read_json, recover_corrupt, write_json_atomic};
use crate::error::RuntimeError;
use crate::platform::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const APP_CONFIG_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct AppConfig {
    pub schema_version: u32,
    pub default_preset_name: Option<String>,
    pub keep_preview_temp: bool,
    pub recent_paths: Vec<String>,
    pub language: String,
    pub last_source_path: String,
    pub last_output_dir: String,
    pub workdir_path: String,
    pub ffmpeg_path: String,
    pub ffprobe_path: String,
    pub log_level: String,
    pub queue_table_header_state: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: APP_CONFIG_SCHEMA_VERSION,
            default_preset_name: Some("default_hevc".into()),
            keep_preview_temp: true,
            recent_paths: Vec::new(),
            language: "en".into(),
            last_source_path: String::new(),
            last_output_dir: String::new(),
            workdir_path: String::new(),
            ffmpeg_path: String::new(),
            ffprobe_path: String::new(),
            log_level: "info".into(),
            queue_table_header_state: String::new(),
        }
    }
}

impl AppConfig {
    pub fn path(paths: &AppPaths) -> PathBuf {
        paths.config_dir.join("app_config.json")
    }

    fn legacy_path(paths: &AppPaths) -> PathBuf {
        // The Python application kept app_config.json below workdir.
        paths.workdir.join("app_config.json")
    }

    fn normalize(mut value: Self) -> Result<Self, RuntimeError> {
        if value.schema_version > APP_CONFIG_SCHEMA_VERSION {
            return Err(RuntimeError::Config(format!(
                "unsupported app config schema version: {}",
                value.schema_version
            )));
        }
        value.schema_version = APP_CONFIG_SCHEMA_VERSION;
        if value.language != "zh_cn" {
            value.language = "en".into();
        }
        Ok(value)
    }

    fn migration_backup(path: &std::path::Path) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        path.with_extension(format!("migrated-{stamp}.json"))
    }

    pub fn load(paths: &AppPaths) -> Result<Self, RuntimeError> {
        let path = Self::path(paths);
        if path.exists() {
            return match read_json(&path) {
                Ok(value) => Self::normalize(value),
                Err(_) => {
                    recover_corrupt(&path)?;
                    Ok(Self::default())
                }
            };
        }

        let legacy = Self::legacy_path(paths);
        if legacy.exists() {
            let value = match read_json::<Self>(&legacy) {
                Ok(value) => Self::normalize(value)?,
                Err(_) => {
                    recover_corrupt(&legacy)?;
                    return Ok(Self::default());
                }
            };
            // Preserve the source before writing the new schema.
            std::fs::copy(&legacy, Self::migration_backup(&legacy))?;
            value.save(paths)?;
            return Ok(value);
        }
        Ok(Self::default())
    }

    pub fn save(&self, paths: &AppPaths) -> Result<(), RuntimeError> {
        let mut value = self.clone();
        value.schema_version = APP_CONFIG_SCHEMA_VERSION;
        write_json_atomic(&Self::path(paths), &value)
    }
}
