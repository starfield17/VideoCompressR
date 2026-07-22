use super::atomic_json::{read_json, recover_corrupt, write_json_atomic};
use crate::error::RuntimeError;
use crate::platform::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use vc_core::{
    AudioMode, Codec, CompressionRatio, ContainerFormat, DecodeAcceleration, EncodeSettings,
    EncoderBackend,
};

pub const PRESET_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PresetData {
    #[serde(default = "default_preset_schema_version")]
    pub schema_version: u32,
    pub codec: Codec,
    pub backend: EncoderBackend,
    #[serde(default)]
    pub decode_acceleration: DecodeAcceleration,
    #[serde(default)]
    pub parallel_enabled: bool,
    #[serde(default)]
    pub parallel_backends: Vec<EncoderBackend>,
    pub ratio: Option<CompressionRatio>,
    pub min_video_kbps: u64,
    pub max_video_kbps: u64,
    pub container: ContainerFormat,
    pub audio_mode: AudioMode,
    pub audio_bitrate: String,
    pub copy_subtitles: bool,
    #[serde(default)]
    pub copy_external_subtitles: bool,
    pub two_pass: bool,
    #[serde(rename = "preset", default)]
    pub encoder_preset: Option<String>,
    #[serde(rename = "pix_fmt")]
    pub pixel_format: String,
    pub maxrate_factor: f64,
    pub bufsize_factor: f64,
}

const fn default_preset_schema_version() -> u32 {
    1
}

impl From<&EncodeSettings> for PresetData {
    fn from(value: &EncodeSettings) -> Self {
        Self {
            schema_version: PRESET_SCHEMA_VERSION,
            codec: value.codec,
            backend: value.backend,
            decode_acceleration: value.decode_acceleration,
            parallel_enabled: value.parallel_enabled,
            parallel_backends: value.parallel_backends.clone(),
            ratio: value.ratio,
            min_video_kbps: value.min_video_kbps,
            max_video_kbps: value.max_video_kbps,
            container: value.container,
            audio_mode: value.audio_mode,
            audio_bitrate: value.audio_bitrate.clone(),
            copy_subtitles: value.copy_subtitles,
            copy_external_subtitles: value.copy_external_subtitles,
            two_pass: value.two_pass,
            encoder_preset: value.encoder_preset.clone(),
            pixel_format: value.pixel_format.clone(),
            maxrate_factor: value.maxrate_factor,
            bufsize_factor: value.bufsize_factor,
        }
    }
}

impl TryFrom<PresetData> for EncodeSettings {
    type Error = RuntimeError;
    fn try_from(value: PresetData) -> Result<Self, Self::Error> {
        if value.schema_version > PRESET_SCHEMA_VERSION {
            return Err(RuntimeError::Config(format!(
                "unsupported preset schema version: {}",
                value.schema_version
            )));
        }
        if value.ratio.is_some_and(|ratio| ratio.get() <= 0.0) {
            return Err(RuntimeError::Config("ratio must be greater than 0".into()));
        }
        let encoder_preset = value.encoder_preset.and_then(|value| {
            let normalized = value.trim().to_owned();
            (!normalized.is_empty()).then_some(normalized)
        });
        Ok(Self {
            codec: value.codec,
            backend: value.backend,
            decode_acceleration: value.decode_acceleration,
            parallel_enabled: value.parallel_enabled,
            parallel_backends: value.parallel_backends,
            ratio: value.ratio,
            min_video_kbps: value.min_video_kbps,
            max_video_kbps: value.max_video_kbps,
            container: value.container,
            audio_mode: value.audio_mode,
            audio_bitrate: value.audio_bitrate,
            copy_subtitles: value.copy_subtitles,
            copy_external_subtitles: value.copy_external_subtitles,
            two_pass: value.two_pass,
            encoder_preset,
            pixel_format: value.pixel_format,
            maxrate_factor: value.maxrate_factor,
            bufsize_factor: value.bufsize_factor,
            ..EncodeSettings::default()
        })
    }
}

#[derive(Clone)]
pub struct PresetStore {
    paths: AppPaths,
}

impl PresetStore {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub fn ensure_defaults(&self) -> Result<(), RuntimeError> {
        for (name, source) in [
            ("default_hevc", include_str!("../../../../config/presets/default_hevc.json")),
            ("default_av1", include_str!("../../../../config/presets/default_av1.json")),
        ] {
            let path = self.path(name)?;
            if path.exists() {
                continue;
            }
            let mut data: PresetData = serde_json::from_str(source)?;
            data.schema_version = PRESET_SCHEMA_VERSION;
            write_json_atomic(&path, &data)?;
        }
        Ok(())
    }
    fn path(&self, name: &str) -> Result<PathBuf, RuntimeError> {
        if !name
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '_' | '-'))
            || name.is_empty()
        {
            return Err(RuntimeError::Config("invalid preset name".into()));
        }
        Ok(self.paths.presets_dir.join(format!("{name}.json")))
    }
    pub fn list(&self) -> Result<Vec<String>, RuntimeError> {
        let mut names = std::fs::read_dir(&self.paths.presets_dir)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                (path.extension().and_then(|value| value.to_str()) == Some("json"))
                    .then(|| path.file_stem()?.to_str().map(str::to_owned))
                    .flatten()
            })
            .collect::<Vec<_>>();
        names.sort();
        Ok(names)
    }
    pub fn load(&self, name: &str) -> Result<EncodeSettings, RuntimeError> {
        let path = self.path(name)?;
        if !path.exists() {
            return Err(RuntimeError::Config(format!("Preset does not exist: {name}")));
        }
        match read_json::<PresetData>(&path) {
            Ok(data) => data.try_into(),
            Err(error) => {
                recover_corrupt(&path)?;
                Err(error)
            }
        }
    }
    pub fn save(&self, name: &str, settings: &EncodeSettings) -> Result<PathBuf, RuntimeError> {
        let path = self.path(name)?;
        write_json_atomic(&path, &PresetData::from(settings))?;
        Ok(path)
    }
    pub fn delete(&self, name: &str) -> Result<(), RuntimeError> {
        let path = self.path(name)?;
        if !path.exists() {
            return Err(RuntimeError::Config(format!("Preset does not exist: {name}")));
        }
        std::fs::remove_file(path)?;
        Ok(())
    }
    pub fn load_path(&self, path: &Path) -> Result<EncodeSettings, RuntimeError> {
        read_json::<PresetData>(path)?.try_into()
    }
}
