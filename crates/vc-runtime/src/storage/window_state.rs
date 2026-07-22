use super::atomic_json::{read_json, recover_corrupt, write_json_atomic};
use crate::error::RuntimeError;
use crate::platform::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const WINDOW_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct WindowGeometry {
    pub width: u32,
    pub height: u32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximized: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WindowState {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub windows: BTreeMap<String, WindowGeometry>,
}

const fn default_schema_version() -> u32 {
    WINDOW_STATE_SCHEMA_VERSION
}

impl Default for WindowState {
    fn default() -> Self {
        Self { schema_version: WINDOW_STATE_SCHEMA_VERSION, windows: BTreeMap::new() }
    }
}

#[derive(Clone)]
pub struct WindowStateStore {
    paths: AppPaths,
}

impl WindowStateStore {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub fn path(&self) -> PathBuf {
        self.paths.config_dir.join("window_state.json")
    }

    pub fn load(&self) -> Result<WindowState, RuntimeError> {
        let path = self.path();
        if !path.exists() {
            return Ok(WindowState::default());
        }
        match read_json::<WindowState>(&path) {
            Ok(mut state) => {
                if state.schema_version > WINDOW_STATE_SCHEMA_VERSION {
                    return Err(RuntimeError::Config(format!(
                        "unsupported window state schema version: {}",
                        state.schema_version
                    )));
                }
                state.schema_version = WINDOW_STATE_SCHEMA_VERSION;
                Ok(state)
            }
            Err(_) => {
                recover_corrupt(&path)?;
                Ok(WindowState::default())
            }
        }
    }

    pub fn save(&self, mut state: WindowState) -> Result<(), RuntimeError> {
        state.schema_version = WINDOW_STATE_SCHEMA_VERSION;
        write_json_atomic(&self.path(), &state)
    }
}
