use super::process::{ToolRequest, run_capture_exact};
use crate::error::RuntimeError;
use std::ffi::OsString;
use std::path::Path;
use tokio_util::sync::CancellationToken;
use vc_core::MediaInfo;

pub async fn ffprobe_json(ffprobe: &Path, input: &Path) -> Result<serde_json::Value, RuntimeError> {
    let mut args = ["-v", "error", "-print_format", "json", "-show_format", "-show_streams"]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    args.push(input.as_os_str().to_os_string());
    let request = ToolRequest { program: ffprobe.to_path_buf(), args, cwd: None };
    let output = run_capture_exact(request, CancellationToken::new()).await?;
    if output.cancelled {
        return Err(RuntimeError::Cancelled);
    }
    if output.code != 0 {
        return Err(RuntimeError::Probe(if output.stderr.trim().is_empty() {
            format!("ffprobe exited with {}", output.code)
        } else {
            output.stderr.clone()
        }));
    }
    serde_json::from_str(&output.stdout)
        .map_err(|error| RuntimeError::Probe(format!("ffprobe did not return valid JSON: {error}")))
}

fn parse_f64(value: Option<&serde_json::Value>) -> Option<f64> {
    value.and_then(|value| {
        value.as_str().and_then(|raw| raw.parse().ok()).or_else(|| value.as_f64())
    })
}
fn parse_u64(value: Option<&serde_json::Value>) -> u64 {
    parse_f64(value).filter(|value| *value > 0.0).map(|value| value as u64).unwrap_or(0)
}

fn fps(stream: &serde_json::Map<String, serde_json::Value>) -> Option<f64> {
    for key in ["avg_frame_rate", "r_frame_rate"] {
        let Some(raw) = stream.get(key).and_then(|value| value.as_str()) else {
            continue;
        };
        if raw == "0/0" || raw == "N/A" {
            continue;
        }
        if let Some((num, den)) = raw.split_once('/') {
            let denominator = den.parse::<f64>().ok()?;
            if denominator != 0.0 {
                return num.parse::<f64>().ok().map(|value| value / denominator);
            }
        }
        if let Ok(value) = raw.parse::<f64>() {
            return Some(value);
        }
    }
    None
}

pub async fn probe_media_info(ffprobe: &Path, input: &Path) -> Result<MediaInfo, RuntimeError> {
    let data = ffprobe_json(ffprobe, input).await?;
    let streams = data
        .get("streams")
        .and_then(|value| value.as_array())
        .ok_or_else(|| RuntimeError::Probe("ffprobe response has no streams".into()))?;
    let video = streams
        .iter()
        .find(|value| value.get("codec_type").and_then(|item| item.as_str()) == Some("video"))
        .and_then(|value| value.as_object())
        .ok_or_else(|| {
            RuntimeError::Probe(format!("No video stream found in: {}", input.display()))
        })?;
    let audios = streams
        .iter()
        .filter(|value| value.get("codec_type").and_then(|item| item.as_str()) == Some("audio"))
        .collect::<Vec<_>>();
    let format = data.get("format").and_then(|value| value.as_object());
    let duration = parse_f64(format.and_then(|item| item.get("duration")))
        .or_else(|| parse_f64(video.get("duration")))
        .unwrap_or(0.0);
    if duration <= 0.0 {
        return Err(RuntimeError::Probe(format!(
            "Cannot determine media duration for: {}",
            input.display()
        )));
    }
    let audio_bitrate = audios.iter().map(|value| parse_u64(value.get("bit_rate"))).sum::<u64>();
    let source_size_bytes = std::fs::metadata(input)?.len();
    let mut format_bitrate = parse_u64(format.and_then(|item| item.get("bit_rate")));
    if format_bitrate == 0 {
        format_bitrate = ((source_size_bytes as f64 * 8.0 / duration).round() as u64).max(1);
    }
    let mut video_bitrate = parse_u64(video.get("bit_rate"));
    if video_bitrate == 0 {
        video_bitrate = format_bitrate
            .saturating_sub(audio_bitrate)
            .max((format_bitrate as f64 * 0.85) as u64)
            .max(300_000);
    }
    Ok(MediaInfo {
        path: input.to_path_buf(),
        source_size_bytes: Some(source_size_bytes),
        duration,
        format_bitrate_bps: format_bitrate,
        video_bitrate_bps: video_bitrate,
        audio_bitrate_bps: audio_bitrate,
        width: video.get("width").and_then(|value| value.as_u64()).map(|value| value as u32),
        height: video.get("height").and_then(|value| value.as_u64()).map(|value| value as u32),
        fps: fps(video),
        video_codec: video
            .get("codec_name")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .into(),
        audio_codec: audios
            .first()
            .and_then(|value| value.get("codec_name"))
            .and_then(|value| value.as_str())
            .map(str::to_owned),
    })
}
