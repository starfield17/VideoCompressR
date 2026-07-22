use crate::model::{CapabilitySnapshot, Codec, EncoderBackend, EncoderSelection};

pub const AUTO_BACKEND_PRIORITY: [EncoderBackend; 5] = [
    EncoderBackend::Nvenc,
    EncoderBackend::Qsv,
    EncoderBackend::Amf,
    EncoderBackend::VideoToolbox,
    EncoderBackend::Cpu,
];

pub fn encoder_candidates(codec: Codec) -> Vec<(EncoderBackend, &'static str)> {
    let mut values = vec![
        (
            EncoderBackend::Nvenc,
            match codec {
                Codec::Hevc => "hevc_nvenc",
                Codec::Av1 => "av1_nvenc",
            },
        ),
        (
            EncoderBackend::Qsv,
            match codec {
                Codec::Hevc => "hevc_qsv",
                Codec::Av1 => "av1_qsv",
            },
        ),
        (
            EncoderBackend::Amf,
            match codec {
                Codec::Hevc => "hevc_amf",
                Codec::Av1 => "av1_amf",
            },
        ),
    ];
    if codec == Codec::Hevc {
        values.push((EncoderBackend::VideoToolbox, "hevc_videotoolbox"));
    }
    values.push((
        EncoderBackend::Cpu,
        match codec {
            Codec::Hevc => "libx265",
            Codec::Av1 => "libsvtav1",
        },
    ));
    values
}

pub fn resolve_encoder(
    codec: Codec,
    backend: EncoderBackend,
    capabilities: &CapabilitySnapshot,
) -> Result<EncoderSelection, String> {
    let candidates = encoder_candidates(codec);
    let find = |wanted: EncoderBackend| {
        let expected = candidates.iter().find(|(candidate, _)| *candidate == wanted)?;
        capabilities
            .for_codec(codec)
            .iter()
            .find(|item| item.backend == wanted && item.encoder == expected.1)
    };
    let selected = if backend == EncoderBackend::Auto {
        AUTO_BACKEND_PRIORITY.iter().find_map(|candidate| find(*candidate)).ok_or_else(|| {
            format!("No usable {} encoder was found on this machine.", codec.as_str())
        })?
    } else {
        let expected =
            candidates.iter().find(|(candidate, _)| *candidate == backend).ok_or_else(|| {
                format!("Backend {} does not support codec {}.", backend.as_str(), codec.as_str())
            })?;
        find(backend).ok_or_else(|| {
            format!(
                "Requested encoder {} is not usable with the current FFmpeg/hardware.",
                expected.1
            )
        })?
    };
    Ok(EncoderSelection {
        codec,
        backend: selected.backend,
        encoder_name: selected.encoder.clone(),
        supports_two_pass: selected.supports_two_pass,
        default_preset: selected.default_preset.clone(),
        preset_choices: selected.presets.clone(),
    })
}
