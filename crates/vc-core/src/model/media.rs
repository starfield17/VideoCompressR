use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct VideoFileItem {
    pub path: PathBuf,
    pub relative_path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MediaInfo {
    pub path: PathBuf,
    #[serde(default)]
    pub source_size_bytes: Option<u64>,
    pub duration: f64,
    pub format_bitrate_bps: u64,
    pub video_bitrate_bps: u64,
    pub audio_bitrate_bps: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub video_codec: String,
    pub audio_codec: Option<String>,
}
