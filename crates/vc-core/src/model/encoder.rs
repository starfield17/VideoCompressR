use super::{Codec, EncoderBackend};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EncoderCapability {
    pub backend: EncoderBackend,
    pub encoder: String,
    #[serde(default)]
    pub supports_two_pass: bool,
    #[serde(default)]
    pub default_preset: Option<String>,
    #[serde(default)]
    pub presets: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CapabilitySnapshot {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub ffmpeg_path: PathBuf,
    #[serde(default)]
    pub ffmpeg_mtime_ns: u128,
    #[serde(default)]
    pub ffmpeg_size_bytes: u64,
    #[serde(default)]
    pub ffmpeg_version: String,
    #[serde(default)]
    pub ffmpeg_version_digest: String,
    #[serde(default)]
    pub platform_os: String,
    #[serde(default)]
    pub platform_arch: String,
    #[serde(default)]
    pub capability_algorithm: String,
    #[serde(default)]
    pub gpu_driver_summary: Option<String>,
    #[serde(default)]
    pub detected_at: String,
    #[serde(default)]
    pub hwaccels: Vec<String>,
    #[serde(default)]
    pub codecs: BTreeMap<String, Vec<EncoderCapability>>,
}

const fn default_schema_version() -> u32 {
    1
}

impl Default for CapabilitySnapshot {
    fn default() -> Self {
        Self {
            schema_version: 1,
            ffmpeg_path: PathBuf::new(),
            ffmpeg_mtime_ns: 0,
            ffmpeg_size_bytes: 0,
            ffmpeg_version: String::new(),
            ffmpeg_version_digest: String::new(),
            platform_os: String::new(),
            platform_arch: String::new(),
            capability_algorithm: String::new(),
            gpu_driver_summary: None,
            detected_at: String::new(),
            hwaccels: Vec::new(),
            codecs: BTreeMap::new(),
        }
    }
}

impl CapabilitySnapshot {
    pub fn for_codec(&self, codec: Codec) -> &[EncoderCapability] {
        self.codecs.get(codec.as_str()).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn has_hwaccel(&self, name: &str) -> bool {
        self.hwaccels.iter().any(|item| item.eq_ignore_ascii_case(name))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EncoderSelection {
    pub codec: Codec,
    pub backend: EncoderBackend,
    pub encoder_name: String,
    pub supports_two_pass: bool,
    pub default_preset: Option<String>,
    #[serde(default)]
    pub preset_choices: Vec<String>,
}
