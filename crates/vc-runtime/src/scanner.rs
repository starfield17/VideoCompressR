use crate::error::RuntimeError;
use std::path::{Path, PathBuf};
use vc_core::VideoFileItem;

pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts", "m2ts", "mts", "mpg", "mpeg",
    "3gp", "ogv",
];

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| VIDEO_EXTENSIONS.iter().any(|item| item.eq_ignore_ascii_case(value)))
}

pub fn collect_video_files(
    input: &Path,
    recursive: bool,
) -> Result<Vec<VideoFileItem>, RuntimeError> {
    let input = input.canonicalize().map_err(|error| {
        RuntimeError::Planning(format!("Cannot access input path {}: {error}", input.display()))
    })?;
    if input.is_file() {
        if !is_video(&input) {
            return Err(RuntimeError::Planning(format!(
                "Input file is not a supported video format: {}",
                input.display()
            )));
        }
        return Ok(vec![VideoFileItem {
            relative_path: PathBuf::from(input.file_name().unwrap_or_default()),
            path: input,
        }]);
    }
    if !input.is_dir() {
        return Err(RuntimeError::Planning(format!(
            "Input path does not exist: {}",
            input.display()
        )));
    }
    let mut values = Vec::new();
    collect_dir(&input, &input, recursive, &mut values)?;
    values.sort_by_key(|item| item.relative_path.to_string_lossy().to_ascii_lowercase());
    Ok(values)
}

fn collect_dir(
    base: &Path,
    root: &Path,
    recursive: bool,
    values: &mut Vec<VideoFileItem>,
) -> Result<(), RuntimeError> {
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() && recursive {
            collect_dir(base, &path, true, values)?;
        }
        if path.is_file() && is_video(&path) {
            values.push(VideoFileItem {
                relative_path: path.strip_prefix(base).unwrap_or(&path).to_path_buf(),
                path: path.canonicalize()?,
            });
        }
    }
    Ok(())
}
