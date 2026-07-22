use crate::error::RuntimeError;
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub root: PathBuf,
    pub config_dir: PathBuf,
    pub presets_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub workdir: PathBuf,
    pub previews_dir: PathBuf,
    pub temp_dir: PathBuf,
}

impl AppPaths {
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            config_dir: root.join("config"),
            presets_dir: root.join("config").join("presets"),
            cache_dir: root.join("cache"),
            logs_dir: root.join("logs"),
            workdir: root.join("workdir"),
            previews_dir: root.join("previews"),
            temp_dir: root.join("temp"),
            root,
        }
    }

    pub fn current() -> Self {
        if let Some(value) = std::env::var_os("VIDEO_COMPRESSOR_DATA_DIR") {
            return Self::from_root(value);
        }
        if let Some(project) = ProjectDirs::from("com", "VideoCompressR", "Video Compressor") {
            return Self {
                root: project.data_dir().to_path_buf(),
                config_dir: project.config_dir().to_path_buf(),
                presets_dir: project.config_dir().join("presets"),
                cache_dir: project.cache_dir().to_path_buf(),
                logs_dir: project.data_dir().join("logs"),
                workdir: project.data_dir().join("workdir"),
                previews_dir: project.data_dir().join("previews"),
                temp_dir: project.data_dir().join("temp"),
            };
        }
        Self::from_root(PathBuf::from(".video-compressor"))
    }

    pub fn ensure(&self) -> Result<(), RuntimeError> {
        for directory in [
            &self.root,
            &self.config_dir,
            &self.presets_dir,
            &self.cache_dir,
            &self.logs_dir,
            &self.workdir,
            &self.previews_dir,
            &self.temp_dir,
        ] {
            std::fs::create_dir_all(directory)?;
        }
        Ok(())
    }

    pub fn with_workdir(&self, workdir: Option<&Path>) -> PathBuf {
        workdir.map(Path::to_path_buf).unwrap_or_else(|| self.workdir.clone())
    }

    pub fn for_workdir(&self, workdir: &Path) -> Self {
        let mut value = self.clone();
        value.workdir = workdir.to_path_buf();
        value.logs_dir = workdir.join("logs");
        value.previews_dir = workdir.join("preview");
        value.temp_dir = workdir.join("temp");
        value
    }
}
