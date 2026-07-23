use crate::model::{EncodePlanItem, EncoderBackend};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueItemStatus {
    Draft,
    Queued,
    Running,
    Done,
    Failed,
    Skipped,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueRunState {
    Idle,
    Running,
    PauseRequested,
    Paused,
    Cancelling,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ItemProgress {
    pub percent: f64,
    pub speed: Option<String>,
    pub elapsed_sec: Option<f64>,
    pub current_pass: u32,
    pub total_passes: u32,
}

impl Default for ItemProgress {
    fn default() -> Self {
        Self { percent: 0.0, speed: None, elapsed_sec: None, current_pass: 0, total_passes: 1 }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ItemResult {
    pub success: bool,
    pub skipped: bool,
    pub return_code: Option<i32>,
    pub output_path: Option<std::path::PathBuf>,
    pub log_path: Option<std::path::PathBuf>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JobError {
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct QueueItem {
    pub item_id: String,
    pub plan: EncodePlanItem,
    pub status: QueueItemStatus,
    pub progress: ItemProgress,
    pub error: Option<JobError>,
    pub result: Option<ItemResult>,
    pub run_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct QueueState {
    pub run_state: QueueRunState,
    #[serde(default)]
    pub active_run_id: Option<String>,
    #[serde(default)]
    pub next_item_sequence: u64,
    pub items: Vec<QueueItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueueExecutionProfile {
    Serial,
    Parallel { backends: Vec<EncoderBackend> },
}

#[allow(clippy::derivable_impls)]
impl Default for QueueRunState {
    fn default() -> Self {
        Self::Idle
    }
}
