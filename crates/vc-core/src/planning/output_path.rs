use crate::model::{Codec, ContainerFormat, VideoFileItem};
use std::path::{Path, PathBuf};

pub fn choose_output_root(
    input: &Path,
    input_is_file: bool,
    explicit: Option<&Path>,
    codec: Codec,
) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    if input_is_file {
        return input.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
    }
    let name = input.file_name().and_then(|value| value.to_str()).unwrap_or("input");
    input
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{name}_compressed_{}", codec.as_str()))
}

pub fn build_output_path(
    item: &VideoFileItem,
    input_root: &Path,
    input_is_directory: bool,
    output_root: &Path,
    codec: Codec,
    container: ContainerFormat,
) -> PathBuf {
    let relative_parent = if input_is_directory {
        item.path
            .parent()
            .and_then(|parent| parent.strip_prefix(input_root).ok())
            .unwrap_or_else(|| Path::new(""))
    } else {
        Path::new("")
    };
    let stem = item.path.file_stem().and_then(|value| value.to_str()).unwrap_or("item");
    output_root.join(relative_parent).join(format!(
        "{stem}_{}.{}",
        codec.as_str(),
        container.as_str()
    ))
}
