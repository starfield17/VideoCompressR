use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct SettingsDto {
    pub codec: String,
    pub backend: String,
    #[serde(rename = "decodeAcceleration")]
    pub decode_acceleration: String,
    #[serde(rename = "parallelEnabled")]
    pub parallel_enabled: bool,
    #[serde(rename = "parallelBackends")]
    pub parallel_backends: Vec<String>,
    pub ratio: Option<f64>,
    #[ts(type = "number")]
    #[serde(rename = "minVideoKbps")]
    pub min_video_kbps: u64,
    #[ts(type = "number")]
    #[serde(rename = "maxVideoKbps")]
    pub max_video_kbps: u64,
    pub container: String,
    #[serde(rename = "audioMode")]
    pub audio_mode: String,
    #[serde(rename = "audioBitrate")]
    pub audio_bitrate: String,
    #[serde(rename = "copySubtitles")]
    pub copy_subtitles: bool,
    #[serde(rename = "copyExternalSubtitles")]
    pub copy_external_subtitles: bool,
    #[serde(rename = "twoPass")]
    pub two_pass: bool,
    #[serde(rename = "encoderPreset")]
    pub encoder_preset: Option<String>,
    #[serde(rename = "pixelFormat")]
    pub pixel_format: String,
    #[serde(rename = "maxrateFactor")]
    pub maxrate_factor: f64,
    #[serde(rename = "bufsizeFactor")]
    pub bufsize_factor: f64,
    pub overwrite: bool,
    pub recursive: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct PlanRequestDto {
    #[serde(rename = "inputPath")]
    pub input_path: String,
    #[serde(rename = "outputDir")]
    pub output_dir: Option<String>,
    pub workdir: Option<String>,
    #[serde(rename = "ffmpegPath")]
    pub ffmpeg_path: Option<String>,
    #[serde(rename = "ffprobePath")]
    pub ffprobe_path: Option<String>,
    pub settings: SettingsDto,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct PlanItemDto {
    #[serde(rename = "sourcePath")]
    pub source_path: String,
    #[serde(rename = "outputPath")]
    pub output_path: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
    #[ts(type = "number | null")]
    #[serde(rename = "sourceBitrate")]
    pub source_bitrate: Option<u64>,
    #[ts(type = "number")]
    #[serde(rename = "targetBitrate")]
    pub target_bitrate: u64,
    pub encoder: Option<String>,
    pub backend: Option<String>,
    pub warnings: Vec<String>,
    #[serde(rename = "skipReason")]
    pub skip_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct PlanResponseDto {
    pub items: Vec<PlanItemDto>,
    #[serde(rename = "ffmpegPath")]
    pub ffmpeg_path: String,
    #[serde(rename = "ffprobePath")]
    pub ffprobe_path: String,
    #[serde(rename = "inputRoot")]
    pub input_root: String,
    #[serde(rename = "outputRoot")]
    pub output_root: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct BootstrapDto {
    pub language: String,
    #[serde(rename = "defaultPresetName")]
    pub default_preset_name: Option<String>,
    #[serde(rename = "ffmpegPath")]
    pub ffmpeg_path: Option<String>,
    #[serde(rename = "ffprobePath")]
    pub ffprobe_path: Option<String>,
    pub settings: SettingsDto,
    #[serde(rename = "appSettings")]
    pub app_settings: AppSettingsDto,
    pub queue: QueueSnapshotDto,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct AppSettingsDto {
    pub language: String,
    #[serde(rename = "defaultPresetName")]
    pub default_preset_name: Option<String>,
    #[serde(rename = "keepPreviewTemp")]
    pub keep_preview_temp: bool,
    #[serde(rename = "recentPaths")]
    pub recent_paths: Vec<String>,
    #[serde(rename = "lastSourcePath")]
    pub last_source_path: String,
    #[serde(rename = "lastOutputDir")]
    pub last_output_dir: String,
    #[serde(rename = "workdirPath")]
    pub workdir_path: String,
    #[serde(rename = "ffmpegPath")]
    pub ffmpeg_path: String,
    #[serde(rename = "ffprobePath")]
    pub ffprobe_path: String,
    #[serde(rename = "logLevel")]
    pub log_level: String,
    #[serde(rename = "queueTableHeaderState")]
    pub queue_table_header_state: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct ApiErrorDto {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct QueueProgressDto {
    pub percent: f64,
    pub speed: Option<String>,
    #[serde(rename = "elapsedSec")]
    pub elapsed_sec: Option<f64>,
    #[serde(rename = "currentPass")]
    pub current_pass: u32,
    #[serde(rename = "totalPasses")]
    pub total_passes: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct QueueItemResultDto {
    pub success: bool,
    pub skipped: bool,
    #[serde(rename = "returnCode")]
    pub return_code: Option<i32>,
    #[serde(rename = "outputPath")]
    pub output_path: Option<String>,
    #[serde(rename = "logPath")]
    pub log_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct QueueItemDto {
    #[serde(rename = "itemId")]
    pub item_id: String,
    pub plan: PlanItemDto,
    pub status: String,
    pub progress: QueueProgressDto,
    pub error: Option<String>,
    pub result: Option<QueueItemResultDto>,
    #[serde(rename = "runId")]
    pub run_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct QueueMetricsDto {
    #[serde(rename = "totalItems")]
    pub total_items: usize,
    #[serde(rename = "queuedItems")]
    pub queued_items: usize,
    #[serde(rename = "runningItems")]
    pub running_items: usize,
    #[serde(rename = "failedItems")]
    pub failed_items: usize,
    #[serde(rename = "doneItems")]
    pub done_items: usize,
    #[serde(rename = "skippedItems")]
    pub skipped_items: usize,
    #[serde(rename = "cancelledItems")]
    pub cancelled_items: usize,
    #[serde(rename = "readyItems")]
    pub ready_items: usize,
    #[serde(rename = "completedItems")]
    pub completed_items: usize,
    #[serde(rename = "totalDurationSec")]
    pub total_duration_sec: f64,
    #[ts(type = "number | null")]
    #[serde(rename = "estimatedSavedBytes")]
    pub estimated_saved_bytes: Option<i64>,
    #[serde(rename = "queuePercent")]
    pub queue_percent: f64,
    #[serde(rename = "etaSec")]
    pub eta_sec: Option<f64>,
    #[serde(rename = "currentItemId")]
    pub current_item_id: Option<String>,
    #[serde(rename = "currentFileName")]
    pub current_file_name: Option<String>,
    #[serde(rename = "currentFilePercent")]
    pub current_file_percent: Option<f64>,
    #[serde(rename = "currentSpeed")]
    pub current_speed: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct QueueStateDto {
    #[serde(rename = "runState")]
    pub run_state: String,
    #[serde(rename = "activeRunId")]
    pub active_run_id: Option<String>,
    pub items: Vec<QueueItemDto>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct QueueSnapshotDto {
    pub state: QueueStateDto,
    pub metrics: QueueMetricsDto,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct ActivityEventDto {
    #[ts(type = "number")]
    pub sequence: u64,
    pub category: String,
    pub message: String,
    pub timestamp: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
#[allow(clippy::large_enum_variant)]
pub enum QueueStreamMessage {
    Snapshot(QueueSnapshotDto),
    Activity(ActivityEventDto),
    ActivityReset { events: Vec<ActivityEventDto> },
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct PreviewOptionsDto {
    #[serde(rename = "sampleMode")]
    pub sample_mode: String,
    #[serde(rename = "sampleDurationSec")]
    pub sample_duration_sec: f64,
    #[serde(rename = "customStartSec")]
    pub custom_start_sec: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct PreviewResultDto {
    pub success: bool,
    #[serde(rename = "sourcePath")]
    pub source_path: String,
    #[serde(rename = "sourceSamplePath")]
    pub source_sample_path: String,
    #[serde(rename = "encodedSamplePath")]
    pub encoded_sample_path: String,
    #[ts(type = "number")]
    #[serde(rename = "sampleSourceSize")]
    pub sample_source_size: u64,
    #[ts(type = "number")]
    #[serde(rename = "sampleEncodedSize")]
    pub sample_encoded_size: u64,
    #[serde(rename = "sampleCompressionRatio")]
    pub sample_compression_ratio: f64,
    #[ts(type = "number")]
    #[serde(rename = "estimatedFullOutputSize")]
    pub estimated_full_output_size: u64,
    pub notes: Vec<String>,
    #[serde(rename = "logPath")]
    pub log_path: Option<String>,
    #[serde(rename = "errorMessage")]
    pub error_message: Option<String>,
}
