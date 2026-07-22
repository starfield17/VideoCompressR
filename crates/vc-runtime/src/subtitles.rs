use crate::error::RuntimeError;
use std::path::{Path, PathBuf};

pub const SIDECAR_EXTENSIONS: &[&str] =
    &["srt", "ass", "ssa", "vtt", "sub", "idx", "sup", "ttml", "dfxp", "smi", "sami", "usf"];

pub fn discover_external_subtitles(source: &Path) -> Result<Vec<PathBuf>, RuntimeError> {
    let stem = source.file_stem().and_then(|value| value.to_str()).unwrap_or_default();
    let mut matches = std::fs::read_dir(source.parent().unwrap_or_else(|| Path::new(".")))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.extension().and_then(|value| value.to_str()).is_some_and(|value| {
                SIDECAR_EXTENSIONS.iter().any(|item| item.eq_ignore_ascii_case(value))
            })
        })
        .filter(|path| {
            path.file_stem().and_then(|value| value.to_str()).is_some_and(|value| {
                value == stem
                    || path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .is_some_and(|name| name.starts_with(&format!("{stem}.")))
            })
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|path| {
        path.file_name().map(|value| value.to_string_lossy().to_ascii_lowercase())
    });
    Ok(matches)
}

pub fn copy_external_subtitles(
    source: &Path,
    output: &Path,
    overwrite: bool,
) -> Result<(Vec<PathBuf>, Vec<String>), RuntimeError> {
    let mut copied = Vec::new();
    let mut warnings = Vec::new();
    for subtitle in discover_external_subtitles(source)? {
        let source_stem = source.file_stem().and_then(|value| value.to_str()).unwrap_or_default();
        let subtitle_name =
            subtitle.file_name().and_then(|value| value.to_str()).unwrap_or_default();
        let suffix = subtitle_name
            .strip_prefix(source_stem)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                subtitle
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(|value| format!(".{value}"))
                    .unwrap_or_default()
            });
        let target = output.with_file_name(format!(
            "{}{}",
            output.file_stem().and_then(|value| value.to_str()).unwrap_or("output"),
            suffix
        ));
        if target.exists() && !overwrite {
            warnings.push(format!(
                "External subtitle exists and overwrite is disabled: {}",
                target.display()
            ));
            continue;
        }
        if let Err(error) = std::fs::copy(&subtitle, &target) {
            warnings
                .push(format!("Failed to copy external subtitle {}: {error}", subtitle.display()));
        } else {
            copied.push(target);
        }
    }
    Ok((copied, warnings))
}
