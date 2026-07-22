use crate::error::RuntimeError;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, RuntimeError> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), RuntimeError> {
    let parent =
        path.parent().ok_or_else(|| RuntimeError::Config("JSON path has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let temp_path: PathBuf = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|name| name.to_str()).unwrap_or("config"),
        Uuid::new_v4()
    ));
    let bytes = serde_json::to_vec_pretty(value)?;
    let result = (|| {
        use std::io::Write;
        let mut file = std::fs::File::create(&temp_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        match std::fs::rename(&temp_path, path) {
            Ok(()) => {}
            Err(error) => {
                #[cfg(windows)]
                {
                    // Windows does not replace an existing file with rename.
                    // The temporary file is complete and synced before this fallback.
                    if path.exists() {
                        std::fs::remove_file(path)?;
                        std::fs::rename(&temp_path, path)?;
                    } else {
                        return Err(error);
                    }
                }
                #[cfg(not(windows))]
                return Err(error);
            }
        }
        Ok::<(), std::io::Error>(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result.map_err(RuntimeError::from)
}

pub fn recover_corrupt(path: &Path) -> Result<Option<PathBuf>, RuntimeError> {
    if !path.exists() {
        return Ok(None);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let broken = path.with_extension(format!("broken-{stamp}"));
    std::fs::rename(path, &broken)?;
    Ok(Some(broken))
}
