use crate::error::RuntimeError;
use crate::platform::paths::AppPaths;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolPaths {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

fn valid_file(path: &Path) -> Option<PathBuf> {
    path.is_file().then(|| path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
}

fn find_named(name: &str, app_paths: &AppPaths) -> Option<PathBuf> {
    let names =
        if cfg!(windows) { vec![format!("{name}.exe"), name.into()] } else { vec![name.into()] };
    let mut candidates = Vec::new();
    for base in [
        app_paths.root.join("FFmpeg"),
        app_paths.root.join("tools"),
        app_paths.config_dir.join("tools"),
        app_paths.root.join("resources").join("FFmpeg"),
        app_paths.root.join("resources").join("tools"),
    ] {
        candidates.push(base.join(name));
        candidates.push(base.join("bin").join(name));
        if cfg!(windows) {
            candidates.push(base.join(format!("{name}.exe")));
            candidates.push(base.join("bin").join(format!("{name}.exe")));
        }
    }
    for candidate in &candidates {
        if let Some(found) = valid_file(candidate) {
            return Some(found);
        }
    }
    if let Some(found) = std::env::var_os("PATH").and_then(|value| {
        names.iter().find_map(|binary| {
            std::env::split_paths(&value).find_map(|dir| valid_file(&dir.join(binary)))
        })
    }) {
        return Some(found);
    }
    let mut platform_candidates = Vec::new();
    #[cfg(windows)]
    {
        if let Some(scoop) = std::env::var_os("SCOOP") {
            platform_candidates
                .push(PathBuf::from(scoop).join("apps").join("ffmpeg").join("current").join("bin"));
        }
        if let Some(user_profile) = std::env::var_os("USERPROFILE") {
            platform_candidates.push(
                PathBuf::from(user_profile)
                    .join("scoop")
                    .join("apps")
                    .join("ffmpeg")
                    .join("current")
                    .join("bin"),
            );
        }
        platform_candidates.push(PathBuf::from(r"C:\ProgramData\chocolatey\bin"));
    }
    #[cfg(not(windows))]
    platform_candidates.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/home/linuxbrew/.linuxbrew/bin"),
    ]);
    platform_candidates
        .into_iter()
        .find_map(|dir| names.iter().find_map(|binary| valid_file(&dir.join(binary))))
}

fn resolve(
    explicit: Option<&Path>,
    name: &str,
    app_paths: &AppPaths,
) -> Result<PathBuf, RuntimeError> {
    if let Some(path) = explicit {
        return valid_file(path).ok_or_else(|| {
            RuntimeError::ToolDiscovery(format!(
                "Cannot find the specified {name}: {}",
                path.display()
            ))
        });
    }
    find_named(name, app_paths).ok_or_else(|| {
        RuntimeError::ToolDiscovery(format!(
            "Cannot find {name}. Checked explicit path, application tools, and PATH."
        ))
    })
}

pub fn discover_tools(
    ffmpeg: Option<&Path>,
    ffprobe: Option<&Path>,
    app_paths: &AppPaths,
) -> Result<ToolPaths, RuntimeError> {
    Ok(ToolPaths {
        ffmpeg: resolve(ffmpeg, "ffmpeg", app_paths)?,
        ffprobe: resolve(ffprobe, "ffprobe", app_paths)?,
    })
}
