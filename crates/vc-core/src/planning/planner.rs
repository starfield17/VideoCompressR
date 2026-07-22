use super::{
    choose_ratio, compute_target_video_bitrate, resolve_encoder, validate_parallel_settings,
    validate_settings,
};
use crate::model::{
    CapabilitySnapshot, DecodeAcceleration, EncodePlanItem, EncodeSettings, MediaInfo, PlanWarning,
    SkipReason,
};
use std::path::PathBuf;

pub struct PlanningInput {
    pub source: MediaInfo,
    pub output_path: PathBuf,
    pub settings: EncodeSettings,
    pub capabilities: CapabilitySnapshot,
    pub output_exists: bool,
}

pub fn plan_item(input: PlanningInput) -> Result<EncodePlanItem, String> {
    if input.source.duration <= 0.0 {
        return Err("Cannot determine media duration.".into());
    }
    if input.settings.decode_acceleration == DecodeAcceleration::VideoToolbox
        && !input.capabilities.has_hwaccel("videotoolbox")
    {
        return Err("VideoToolbox decoding was requested, but the selected FFmpeg build does not expose the videotoolbox hardware accelerator.".into());
    }
    let encoder =
        resolve_encoder(input.settings.codec, input.settings.backend, &input.capabilities)?;
    if input.settings.parallel_enabled {
        validate_parallel_settings(&input.settings, true)?;
        for backend in input
            .settings
            .parallel_backends
            .iter()
            .copied()
            .filter(|backend| *backend != crate::model::EncoderBackend::Auto)
        {
            resolve_encoder(input.settings.codec, backend, &input.capabilities)?;
        }
    }
    let ratio = choose_ratio(input.settings.codec, input.settings.ratio)?;
    let target = compute_target_video_bitrate(
        input.source.video_bitrate_bps,
        ratio,
        input.settings.min_video_kbps,
        input.settings.max_video_kbps,
    );
    let mut settings = input.settings;
    let mut warnings = Vec::new();
    if settings.encoder_preset.is_none() && !settings.parallel_enabled {
        if let Some(default) = encoder.default_preset.clone() {
            if encoder.preset_choices.is_empty()
                || encoder.preset_choices.iter().any(|value| value == &default)
            {
                settings.encoder_preset = Some(default);
            } else {
                warnings.push(PlanWarning(format!(
                    "Default encoder preset is unavailable for {}; using encoder defaults.",
                    encoder.encoder_name
                )));
            }
        }
    }
    if settings.copy_external_subtitles {
        warnings
            .push(PlanWarning("External subtitle sidecars will be copied when present.".into()));
    }
    validate_settings(
        &settings,
        &encoder,
        input.output_exists,
        &input.source.path,
        &input.output_path,
    )?;
    Ok(EncodePlanItem {
        source_path: input.source.path.clone(),
        output_path: input.output_path,
        media_info: Some(input.source),
        encoder: Some(encoder),
        settings,
        target_video_bitrate_bps: target.get(),
        warnings,
        skip_reason: None,
    })
}

pub fn skipped_item(
    source_path: PathBuf,
    output_path: PathBuf,
    settings: EncodeSettings,
    reason: impl Into<String>,
) -> EncodePlanItem {
    EncodePlanItem {
        source_path,
        output_path,
        media_info: None,
        encoder: None,
        settings,
        target_video_bitrate_bps: 0,
        warnings: Vec::new(),
        skip_reason: Some(SkipReason(reason.into())),
    }
}
