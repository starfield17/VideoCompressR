use super::EncoderBackend;
use super::{AudioMode, Codec, ContainerFormat, DecodeAcceleration};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EncodeSettings {
    pub codec: Codec,
    pub backend: EncoderBackend,
    #[serde(default)]
    pub decode_acceleration: DecodeAcceleration,
    #[serde(default)]
    pub parallel_enabled: bool,
    #[serde(default)]
    pub parallel_backends: Vec<EncoderBackend>,
    #[serde(default)]
    pub ratio: Option<super::CompressionRatio>,
    #[serde(default = "default_min_video_kbps")]
    pub min_video_kbps: u64,
    #[serde(default)]
    pub max_video_kbps: u64,
    #[serde(default)]
    pub container: ContainerFormat,
    #[serde(default)]
    pub audio_mode: AudioMode,
    #[serde(default = "default_audio_bitrate")]
    pub audio_bitrate: String,
    #[serde(default = "default_true")]
    pub copy_subtitles: bool,
    #[serde(default)]
    pub copy_external_subtitles: bool,
    #[serde(default)]
    pub two_pass: bool,
    #[serde(rename = "preset", default)]
    pub encoder_preset: Option<String>,
    #[serde(rename = "pix_fmt")]
    #[serde(default = "default_pixel_format")]
    pub pixel_format: String,
    #[serde(default = "default_maxrate_factor")]
    pub maxrate_factor: f64,
    #[serde(default = "default_bufsize_factor")]
    pub bufsize_factor: f64,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub dry_run: bool,
}

const fn default_min_video_kbps() -> u64 {
    250
}
const fn default_true() -> bool {
    true
}
fn default_audio_bitrate() -> String {
    "128k".into()
}
fn default_pixel_format() -> String {
    "yuv420p".into()
}
const fn default_maxrate_factor() -> f64 {
    1.25
}
const fn default_bufsize_factor() -> f64 {
    4.0
}

impl Default for EncodeSettings {
    fn default() -> Self {
        Self {
            codec: Codec::Hevc,
            backend: EncoderBackend::Auto,
            decode_acceleration: DecodeAcceleration::Software,
            parallel_enabled: false,
            parallel_backends: Vec::new(),
            ratio: None,
            min_video_kbps: 250,
            max_video_kbps: 0,
            container: ContainerFormat::Mp4,
            audio_mode: AudioMode::Copy,
            audio_bitrate: "128k".into(),
            copy_subtitles: true,
            copy_external_subtitles: true,
            two_pass: false,
            encoder_preset: None,
            pixel_format: "yuv420p".into(),
            maxrate_factor: 1.25,
            bufsize_factor: 4.0,
            overwrite: false,
            recursive: false,
            dry_run: false,
        }
    }
}
