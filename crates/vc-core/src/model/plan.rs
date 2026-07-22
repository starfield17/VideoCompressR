use super::{EncodeSettings, EncoderSelection, MediaInfo};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PlanWarning(pub String);

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SkipReason(pub String);

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EncodePlanItem {
    pub source_path: PathBuf,
    pub output_path: PathBuf,
    pub media_info: Option<MediaInfo>,
    pub encoder: Option<EncoderSelection>,
    pub settings: EncodeSettings,
    pub target_video_bitrate_bps: u64,
    pub warnings: Vec<PlanWarning>,
    pub skip_reason: Option<SkipReason>,
}

impl EncodePlanItem {
    pub fn is_ready(&self) -> bool {
        self.skip_reason.is_none() && self.media_info.is_some() && self.encoder.is_some()
    }
}
