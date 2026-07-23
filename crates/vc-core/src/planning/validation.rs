use crate::model::{EncodeSettings, EncoderSelection};

pub fn unique_parallel_backends(
    backends: &[crate::model::EncoderBackend],
) -> Vec<crate::model::EncoderBackend> {
    let mut seen = std::collections::HashSet::new();
    backends.iter().copied().filter(|backend| seen.insert(*backend)).collect()
}

pub fn validate_parallel_settings(
    settings: &EncodeSettings,
    allow_parallel: bool,
) -> Result<(), String> {
    if !settings.parallel_enabled {
        return Ok(());
    }
    if !allow_parallel {
        return Err("Parallel mode is not supported for preview.".into());
    }
    let backends = unique_parallel_backends(&settings.parallel_backends)
        .into_iter()
        .filter(|value| *value != crate::model::EncoderBackend::Auto)
        .collect::<Vec<_>>();
    if backends.is_empty() {
        return Err("Parallel mode requires at least one explicit backend.".into());
    }
    if settings.two_pass {
        return Err("Parallel mode does not support two-pass encoding.".into());
    }
    if settings.encoder_preset.is_some() {
        return Err("Parallel mode does not support a manual encoder preset.".into());
    }
    Ok(())
}

pub fn validate_settings(
    settings: &EncodeSettings,
    encoder: &EncoderSelection,
    output_exists: bool,
    source_path: &std::path::Path,
    output_path: &std::path::Path,
) -> Result<(), String> {
    validate_parallel_settings(settings, true)?;
    if settings.two_pass && !encoder.supports_two_pass {
        return Err(format!(
            "Encoder {} does not support two-pass in this implementation.",
            encoder.encoder_name
        ));
    }
    if settings.decode_acceleration == crate::model::DecodeAcceleration::VideoToolbox {
        // The capability check is made by the planner before this function.
    }
    if source_path == output_path {
        return Err(format!(
            "Output path matches the input path, refusing to overwrite source: {}",
            source_path.display()
        ));
    }
    if output_exists && !settings.overwrite {
        return Err(format!(
            "Output already exists and overwrite is disabled: {}",
            output_path.display()
        ));
    }
    if !settings.maxrate_factor.is_finite() || settings.maxrate_factor <= 0.0 {
        return Err("maxrate_factor must be greater than 0".into());
    }
    if !settings.bufsize_factor.is_finite() || settings.bufsize_factor <= 0.0 {
        return Err("bufsize_factor must be greater than 0".into());
    }
    Ok(())
}
