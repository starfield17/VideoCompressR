use super::atomic_json::{read_json, recover_corrupt, write_json_atomic};
use crate::error::RuntimeError;
use crate::platform::paths::AppPaths;
use std::path::PathBuf;
use vc_core::CapabilitySnapshot;

pub const CAPABILITY_SCHEMA_VERSION: u32 = 5;

#[derive(Clone)]
pub struct CapabilityCache {
    paths: AppPaths,
}

impl CapabilityCache {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }
    fn path(&self) -> PathBuf {
        self.paths.cache_dir.join("capabilities.json")
    }
    pub fn load(&self) -> Result<Option<CapabilitySnapshot>, RuntimeError> {
        let path = self.path();
        if !path.exists() {
            return Ok(None);
        }
        match read_json(&path) {
            Ok(value) => Ok(Some(value)),
            Err(_) => {
                recover_corrupt(&path)?;
                Ok(None)
            }
        }
    }
    pub fn save(&self, value: &CapabilitySnapshot) -> Result<(), RuntimeError> {
        write_json_atomic(&self.path(), value)
    }
}
