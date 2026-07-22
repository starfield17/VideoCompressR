use super::{QueueItemStatus, QueueState};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct QueueMetrics {
    pub total_items: usize,
    pub queued_items: usize,
    pub running_items: usize,
    pub failed_items: usize,
    pub done_items: usize,
    pub skipped_items: usize,
    pub cancelled_items: usize,
    pub ready_items: usize,
    pub completed_items: usize,
    pub total_duration_sec: f64,
    pub estimated_saved_bytes: Option<i64>,
    pub queue_percent: f64,
    pub eta_sec: Option<f64>,
    pub current_item_id: Option<String>,
    pub current_file_name: Option<String>,
    pub current_file_percent: Option<f64>,
    pub current_speed: Option<String>,
}

pub fn compute_metrics(state: &QueueState) -> QueueMetrics {
    let mut metrics = QueueMetrics { total_items: state.items.len(), ..QueueMetrics::default() };
    let mut total_weight = 0.0;
    let mut processed_weight = 0.0;
    let mut estimated_saved_bytes = 0_i64;
    let mut has_estimate = false;
    for item in &state.items {
        let media = item.plan.media_info.as_ref();
        let duration = media.map(|value| value.duration.max(0.0)).unwrap_or(0.0);
        if let Some(media) = media {
            let audio_bitrate = match item.plan.settings.audio_mode {
                crate::model::AudioMode::Copy => media.audio_bitrate_bps,
                crate::model::AudioMode::Aac => parse_bitrate(&item.plan.settings.audio_bitrate)
                    .unwrap_or(media.audio_bitrate_bps),
            };
            let source_size = media.source_size_bytes.or_else(|| {
                (media.format_bitrate_bps > 0 && duration > 0.0)
                    .then_some((media.format_bitrate_bps as f64 * duration / 8.0) as u64)
            });
            if let Some(source_size) = source_size {
                let estimated_output =
                    (duration * (item.plan.target_video_bitrate_bps + audio_bitrate) as f64 / 8.0)
                        .round() as i64;
                if let Ok(source_size) = i64::try_from(source_size) {
                    estimated_saved_bytes = estimated_saved_bytes
                        .saturating_add(source_size.saturating_sub(estimated_output));
                    has_estimate = true;
                }
            }
        }
        let passes = item.progress.total_passes.max(1) as f64;
        let weight = duration * passes;
        total_weight += weight;
        metrics.total_duration_sec += duration;
        match item.status {
            QueueItemStatus::Queued => metrics.queued_items += 1,
            QueueItemStatus::Running => metrics.running_items += 1,
            QueueItemStatus::Failed => metrics.failed_items += 1,
            QueueItemStatus::Done => {
                metrics.done_items += 1;
                processed_weight += weight;
            }
            QueueItemStatus::Skipped => {
                metrics.skipped_items += 1;
                processed_weight += weight;
            }
            QueueItemStatus::Cancelled => metrics.cancelled_items += 1,
            QueueItemStatus::Draft => {}
        }
        if item.status == QueueItemStatus::Running {
            processed_weight += weight * item.progress.percent.clamp(0.0, 100.0) / 100.0;
            metrics.current_item_id = Some(item.item_id.clone());
            metrics.current_file_name = item
                .plan
                .source_path
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_owned);
            metrics.current_file_percent = Some(item.progress.percent);
            metrics.current_speed = item.progress.speed.clone();
            if let Some(speed) = item.progress.speed.as_deref().and_then(parse_speed) {
                metrics.eta_sec = Some((total_weight - processed_weight).max(0.0) / speed);
            }
        }
    }
    metrics.ready_items = metrics.queued_items;
    metrics.completed_items = metrics.done_items + metrics.skipped_items;
    metrics.estimated_saved_bytes = has_estimate.then_some(estimated_saved_bytes);
    metrics.queue_percent = if total_weight > 0.0 {
        (processed_weight / total_weight * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    metrics
}

fn parse_speed(value: &str) -> Option<f64> {
    let raw = value.trim().strip_suffix('x')?.trim().parse::<f64>().ok()?;
    (raw > 0.0).then_some(raw)
}

fn parse_bitrate(value: &str) -> Option<u64> {
    let value = value.trim().to_ascii_lowercase();
    let (number, multiplier) = if let Some(value) = value.strip_suffix('k') {
        (value, 1_000_f64)
    } else if let Some(value) = value.strip_suffix('m') {
        (value, 1_000_000_f64)
    } else {
        (value.as_str(), 1.0)
    };
    let parsed = number.parse::<f64>().ok()? * multiplier;
    (parsed.is_finite() && parsed > 0.0).then_some(parsed.round() as u64)
}
