use super::{EncodePlanItem, PreviewSampleMode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PreviewOptions {
    pub sample_mode: PreviewSampleMode,
    pub sample_duration_sec: f64,
    pub custom_start_sec: Option<f64>,
}

impl Default for PreviewOptions {
    fn default() -> Self {
        Self {
            sample_mode: PreviewSampleMode::Middle,
            sample_duration_sec: 30.0,
            custom_start_sec: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SampleWindow {
    pub start_sec: f64,
    pub duration_sec: f64,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PreviewJob {
    pub source_path: PathBuf,
    pub source_sample_path: PathBuf,
    pub encoded_sample_path: PathBuf,
    pub window: SampleWindow,
    pub plan_item: EncodePlanItem,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PreviewResult {
    pub job: PreviewJob,
    pub success: bool,
    pub source_sample_size: u64,
    pub encoded_sample_size: u64,
    pub sample_compression_ratio: f64,
    pub estimated_full_output_size: u64,
    pub notes: Vec<String>,
    pub log_path: Option<PathBuf>,
    pub error_message: Option<String>,
}

pub fn choose_sample_window(
    duration_sec: f64,
    options: &PreviewOptions,
) -> Result<SampleWindow, String> {
    if !duration_sec.is_finite() || duration_sec <= 0.0 {
        return Err("Source duration must be greater than 0.".into());
    }
    if !options.sample_duration_sec.is_finite() || options.sample_duration_sec <= 0.0 {
        return Err("Preview sample duration must be greater than 0.".into());
    }
    let mut notes = Vec::new();
    let sample_duration = if options.sample_duration_sec > duration_sec {
        notes
            .push("Sample duration exceeds source duration, using the full source instead.".into());
        duration_sec
    } else {
        options.sample_duration_sec
    };
    let start_sec = match options.sample_mode {
        PreviewSampleMode::Middle => (duration_sec - sample_duration).max(0.0) / 2.0,
        PreviewSampleMode::Custom => {
            let requested = options.custom_start_sec.unwrap_or(0.0);
            let max_start = (duration_sec - sample_duration).max(0.0);
            let clamped = requested.clamp(0.0, max_start);
            if clamped != requested {
                notes.push("Custom preview start was out of range and has been clamped.".into());
            }
            clamped
        }
    };
    Ok(SampleWindow { start_sec, duration_sec: sample_duration, notes })
}
