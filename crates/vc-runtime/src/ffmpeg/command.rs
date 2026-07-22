use super::process::ToolRequest;
use crate::error::RuntimeError;
use crate::platform::paths::AppPaths;
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::Path;
use vc_core::{AudioMode, ContainerFormat, DecodeAcceleration, EncodePlanItem};

fn os(value: impl AsRef<std::ffi::OsStr>) -> OsString {
    value.as_ref().to_os_string()
}
fn null_sink() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

fn video_args(item: &EncodePlanItem) -> Result<Vec<OsString>, RuntimeError> {
    let encoder = item
        .encoder
        .as_ref()
        .ok_or_else(|| RuntimeError::Encode("plan item has no encoder".into()))?;
    let settings = &item.settings;
    let target = item.target_video_bitrate_bps;
    let mut args = vec![
        os("-c:v"),
        os(&encoder.encoder_name),
        os("-b:v"),
        os(target.to_string()),
        os("-pix_fmt"),
        os(&settings.pixel_format),
    ];
    if encoder.encoder_name != "libsvtav1" {
        args.extend([
            os("-maxrate"),
            os((target as f64 * settings.maxrate_factor).round().max(0.0).to_string()),
            os("-bufsize"),
            os((target as f64 * settings.bufsize_factor).round().max(0.0).to_string()),
        ]);
    }
    if let Some(preset) = &settings.encoder_preset {
        args.extend([os("-preset"), os(preset)]);
    }
    if matches!(encoder.encoder_name.as_str(), "libx265" | "hevc_videotoolbox") {
        args.extend([os("-tag:v"), os("hvc1")]);
    }
    if encoder.encoder_name == "libx265" {
        args.extend([os("-x265-params"), os("log-level=error")]);
    }
    if encoder.encoder_name == "hevc_videotoolbox" {
        args.extend([os("-allow_sw"), os("0")]);
    }
    Ok(args)
}

fn audio_args(item: &EncodePlanItem) -> Vec<OsString> {
    match item.settings.audio_mode {
        AudioMode::Copy => vec![os("-map"), os("0:a?"), os("-c:a"), os("copy")],
        AudioMode::Aac => vec![
            os("-map"),
            os("0:a?"),
            os("-c:a"),
            os("aac"),
            os("-b:a"),
            os(&item.settings.audio_bitrate),
        ],
    }
}

fn subtitle_args(item: &EncodePlanItem) -> Vec<OsString> {
    if !item.settings.copy_subtitles {
        return Vec::new();
    }
    match item.settings.container {
        ContainerFormat::Mkv => vec![os("-map"), os("0:s?"), os("-c:s"), os("copy")],
        ContainerFormat::Mp4 => vec![os("-map"), os("0:s?"), os("-c:s"), os("mov_text")],
    }
}

fn common_output_args(item: &EncodePlanItem) -> Vec<OsString> {
    let mut args = vec![os("-map_metadata"), os("0"), os("-map_chapters"), os("0")];
    if item.settings.container == ContainerFormat::Mp4 {
        args.extend([os("-movflags"), os("+faststart")]);
    }
    args
}

fn base(item: &EncodePlanItem, ffmpeg: &Path, input: &Path, overwrite: bool) -> Vec<OsString> {
    let mut args = vec![
        os("-hide_banner"),
        os("-nostats"),
        os("-progress"),
        os("pipe:1"),
        os("-stats_period"),
        os("0.5"),
        os(if overwrite { "-y" } else { "-n" }),
    ];
    if item.settings.decode_acceleration == DecodeAcceleration::VideoToolbox {
        args.extend([os("-hwaccel"), os("videotoolbox")]);
    }
    args.extend([os("-i"), os(input)]);
    let _ = ffmpeg;
    args
}

pub fn render_encode_commands(
    ffmpeg: &Path,
    item: &EncodePlanItem,
    paths: &AppPaths,
    input: Option<&Path>,
    output: Option<&Path>,
    stage: &str,
) -> Result<Vec<ToolRequest>, RuntimeError> {
    let source = input.unwrap_or(&item.source_path);
    let final_output = output.unwrap_or(&item.output_path);
    let overwrite = item.settings.overwrite || stage == "preview";
    let input_base = base(item, ffmpeg, source, overwrite);
    let video = video_args(item)?;
    let audio = audio_args(item);
    let subtitles = subtitle_args(item);
    let common = common_output_args(item);
    let mut requests = Vec::new();
    let passlog = passlog_path(paths, item, stage);
    if item.settings.two_pass
        && item.encoder.as_ref().is_some_and(|encoder| encoder.supports_two_pass)
    {
        let mut pass1 = input_base.clone();
        pass1.extend([os("-map"), os("0:v:0")]);
        pass1.extend(video.clone());
        pass1.extend([
            os("-an"),
            os("-sn"),
            os("-dn"),
            os("-pass"),
            os("1"),
            os("-passlogfile"),
            os(&passlog),
            os("-f"),
            os("null"),
            os(null_sink()),
        ]);
        let mut pass2 = input_base;
        pass2.extend([os("-map"), os("0:v:0")]);
        pass2.extend(audio);
        pass2.extend(subtitles);
        pass2.extend(video);
        pass2.extend(common);
        pass2.extend([os("-pass"), os("2"), os("-passlogfile"), os(&passlog), os(final_output)]);
        requests.push(ToolRequest { program: ffmpeg.to_path_buf(), args: pass1, cwd: None });
        requests.push(ToolRequest { program: ffmpeg.to_path_buf(), args: pass2, cwd: None });
    } else {
        let mut args = input_base;
        args.extend([os("-map"), os("0:v:0")]);
        args.extend(audio);
        args.extend(subtitles);
        args.extend(video);
        args.extend(common);
        args.push(os(final_output));
        requests.push(ToolRequest { program: ffmpeg.to_path_buf(), args, cwd: None });
    }
    Ok(requests)
}

pub fn passlog_path(paths: &AppPaths, item: &EncodePlanItem, stage: &str) -> std::path::PathBuf {
    paths.temp_dir.join(format!("{}-{stage}.ffpass", safe_token(&item.source_path)))
}

pub fn cleanup_passlog(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    let Some(prefix) = path.file_name().and_then(|value| value.to_str()) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let candidate = entry.path();
        let matches = candidate
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name == prefix || name.starts_with(&format!("{prefix}.")));
        if matches {
            let _ = std::fs::remove_file(candidate);
        }
    }
}

pub fn render_preview_extract(ffmpeg: &Path, job: &vc_core::PreviewJob) -> ToolRequest {
    let mut args = vec!["-hide_banner", "-nostats", "-progress", "pipe:1", "-y", "-ss"]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    args.push(OsString::from(format!("{:.3}", job.window.start_sec)));
    args.extend([
        OsString::from("-t"),
        OsString::from(format!("{:.3}", job.window.duration_sec)),
        OsString::from("-i"),
        os(job.source_path.as_os_str()),
        OsString::from("-map"),
        OsString::from("0:v:0"),
        OsString::from("-map"),
        OsString::from("0:a?"),
        OsString::from("-map"),
        OsString::from("0:s?"),
        OsString::from("-c"),
        OsString::from("copy"),
        os(job.source_sample_path.as_os_str()),
    ]);
    ToolRequest { program: ffmpeg.to_path_buf(), args, cwd: None }
}

fn safe_token(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("item")
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() || matches!(value, '.' | '_' | '-') {
                value
            } else {
                '_'
            }
        })
        .collect::<String>();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    let digest = format!("{:x}", hasher.finish());
    format!("{stem}-{digest}")
}
