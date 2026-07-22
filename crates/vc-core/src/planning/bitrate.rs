use crate::model::{BitrateBps, Codec, CompressionRatio};

pub const DEFAULT_MIN_VIDEO_KBPS: u64 = 250;

pub fn choose_ratio(codec: Codec, ratio: Option<CompressionRatio>) -> Result<f64, String> {
    if let Some(value) = ratio {
        if !value.get().is_finite() || value.get() <= 0.0 {
            return Err("ratio must be greater than 0".into());
        }
        return Ok(value.get());
    }
    Ok(match codec {
        Codec::Hevc => 0.76,
        Codec::Av1 => 0.64,
    })
}

pub fn compute_target_video_bitrate(
    source_video_bps: u64,
    ratio: f64,
    min_video_kbps: u64,
    max_video_kbps: u64,
) -> BitrateBps {
    let scaled = (source_video_bps as f64 * ratio).round().max(0.0) as u64;
    let min = min_video_kbps.saturating_mul(1000);
    let max = max_video_kbps.saturating_mul(1000);
    let mut target = scaled.max(min);
    if max_video_kbps > 0 {
        target = target.min(max);
    }
    BitrateBps::new(target.max(50_000))
}
