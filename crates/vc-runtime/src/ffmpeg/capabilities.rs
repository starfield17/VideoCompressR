use super::discovery::ToolPaths;
use super::process::{ToolRequest, run_capture_exact};
use crate::error::RuntimeError;
use crate::platform::paths::AppPaths;
use crate::storage::capability_cache::{CAPABILITY_SCHEMA_VERSION, CapabilityCache};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::Duration;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use vc_core::{CapabilitySnapshot, Codec, EncoderBackend, EncoderCapability};

pub const CAPABILITY_ALGORITHM: &str = "5";

pub async fn ensure_capabilities(
    paths: &AppPaths,
    tools: &ToolPaths,
    force_refresh: bool,
) -> Result<CapabilitySnapshot, RuntimeError> {
    let cache = CapabilityCache::new(paths.clone());
    if !force_refresh {
        if let Some(value) = cache.load()? {
            if is_valid(&value, &tools.ffmpeg).await {
                return Ok(value);
            }
        }
    }
    let value = detect_capabilities(tools).await?;
    cache.save(&value)?;
    Ok(value)
}

async fn run(ffmpeg: &Path, args: &[&str]) -> Result<(i32, String, String), RuntimeError> {
    let request = ToolRequest {
        program: ffmpeg.to_path_buf(),
        args: args.iter().map(OsString::from).collect(),
        cwd: None,
    };
    let output = run_capture_exact(request, CancellationToken::new()).await?;
    if output.cancelled {
        return Err(RuntimeError::Cancelled);
    }
    Ok((output.code, output.stdout, output.stderr))
}

async fn version(ffmpeg: &Path) -> Result<String, RuntimeError> {
    let (_, stdout, stderr) = run(ffmpeg, &["-version"]).await?;
    Ok(stdout
        .lines()
        .chain(stderr.lines())
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim()
        .into())
}

fn version_digest(value: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn gpu_driver_summary() -> Option<String> {
    std::env::var("VIDEO_COMPRESSOR_GPU_DRIVER_SUMMARY")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_encoders(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let flags = fields.next()?;
            let name = fields.next()?;
            (flags.len() == 6
                && flags.chars().all(|value| value.is_ascii_uppercase() || value == '.'))
            .then(|| name.to_owned())
        })
        .collect()
}

fn parse_hwaccels(output: &str) -> Vec<String> {
    let mut values = output
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty() && !line.to_ascii_lowercase().contains("hardware acceleration methods")
        })
        .map(str::to_ascii_lowercase)
        .filter(|line| {
            line.chars().all(|value| {
                value.is_ascii_lowercase()
                    || value.is_ascii_digit()
                    || matches!(value, '.' | '_' | '+' | '-')
            })
        })
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn encoder_name(codec: Codec, backend: EncoderBackend) -> Option<&'static str> {
    match (codec, backend) {
        (Codec::Hevc, EncoderBackend::Nvenc) => Some("hevc_nvenc"),
        (Codec::Hevc, EncoderBackend::Qsv) => Some("hevc_qsv"),
        (Codec::Hevc, EncoderBackend::Amf) => Some("hevc_amf"),
        (Codec::Hevc, EncoderBackend::VideoToolbox) => Some("hevc_videotoolbox"),
        (Codec::Hevc, EncoderBackend::Cpu) => Some("libx265"),
        (Codec::Av1, EncoderBackend::Nvenc) => Some("av1_nvenc"),
        (Codec::Av1, EncoderBackend::Qsv) => Some("av1_qsv"),
        (Codec::Av1, EncoderBackend::Amf) => Some("av1_amf"),
        (Codec::Av1, EncoderBackend::Cpu) => Some("libsvtav1"),
        _ => None,
    }
}

fn default_preset(encoder: &str) -> Option<String> {
    match encoder {
        "libx265" => Some("slow"),
        "libsvtav1" => Some("5"),
        "hevc_nvenc" | "av1_nvenc" => Some("p6"),
        "hevc_qsv" | "av1_qsv" => Some("slow"),
        _ => None,
    }
    .map(str::to_owned)
}

fn fallback_presets(encoder: &str) -> Vec<String> {
    let values: &[&str] = match encoder {
        "libx265" => &[
            "ultrafast",
            "superfast",
            "veryfast",
            "faster",
            "fast",
            "medium",
            "slow",
            "slower",
            "veryslow",
            "placebo",
        ],
        "hevc_nvenc" | "av1_nvenc" => &["p1", "p2", "p3", "p4", "p5", "p6", "p7"],
        "hevc_qsv" | "av1_qsv" => {
            &["veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"]
        }
        _ => &[],
    };
    values.iter().map(|value| (*value).to_owned()).collect()
}

async fn smoke(ffmpeg: &Path, encoder: &str) -> bool {
    let mut args = vec![
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=256x256:rate=1",
        "-frames:v",
        "1",
        "-an",
        "-c:v",
        encoder,
    ];
    if encoder == "hevc_videotoolbox" {
        args.extend(["-allow_sw", "0"]);
    }
    let null_output = if cfg!(windows) { "NUL" } else { "-" };
    args.extend(["-f", "null", null_output]);
    matches!(timeout(Duration::from_secs(10), run(ffmpeg, &args)).await, Ok(Ok((0, _, _))))
}

async fn detect_capabilities(tools: &ToolPaths) -> Result<CapabilitySnapshot, RuntimeError> {
    let version = version(&tools.ffmpeg).await?;
    let (_, encoder_output, encoder_error) =
        run(&tools.ffmpeg, &["-hide_banner", "-encoders"]).await?;
    let encoders = parse_encoders(&format!("{encoder_output}\n{encoder_error}"));
    let (_, hw_output, hw_error) = run(&tools.ffmpeg, &["-hide_banner", "-hwaccels"]).await?;
    let hwaccels = parse_hwaccels(&format!("{hw_output}\n{hw_error}"));
    let mut codecs: BTreeMap<String, Vec<EncoderCapability>> = BTreeMap::new();
    for codec in [Codec::Hevc, Codec::Av1] {
        let mut values = Vec::new();
        for backend in [
            EncoderBackend::Nvenc,
            EncoderBackend::Qsv,
            EncoderBackend::Amf,
            EncoderBackend::VideoToolbox,
            EncoderBackend::Cpu,
        ] {
            let Some(encoder) = encoder_name(codec, backend) else {
                continue;
            };
            if !encoders.iter().any(|value| value == encoder)
                || !smoke(&tools.ffmpeg, encoder).await
            {
                continue;
            }
            let default = default_preset(encoder);
            values.push(EncoderCapability {
                backend,
                encoder: encoder.into(),
                supports_two_pass: encoder == "libx265",
                default_preset: default,
                presets: fallback_presets(encoder),
            });
        }
        codecs.insert(codec.as_str().into(), values);
    }
    let metadata = std::fs::metadata(&tools.ffmpeg)?;
    let platform_os = std::env::consts::OS.to_owned();
    let platform_arch = std::env::consts::ARCH.to_owned();
    let gpu_driver_summary = gpu_driver_summary();
    let ffmpeg_version_digest = version_digest(&version);
    Ok(CapabilitySnapshot {
        schema_version: CAPABILITY_SCHEMA_VERSION,
        ffmpeg_path: tools.ffmpeg.canonicalize()?,
        ffmpeg_mtime_ns: metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        ffmpeg_size_bytes: metadata.len(),
        ffmpeg_version: version,
        ffmpeg_version_digest,
        platform_os,
        platform_arch,
        capability_algorithm: CAPABILITY_ALGORITHM.into(),
        gpu_driver_summary,
        detected_at: format!("{:?}", std::time::SystemTime::now()),
        hwaccels,
        codecs,
    })
}

async fn is_valid(value: &CapabilitySnapshot, ffmpeg: &Path) -> bool {
    if value.schema_version != CAPABILITY_SCHEMA_VERSION {
        return false;
    }
    let Ok(canonical) = ffmpeg.canonicalize() else {
        return false;
    };
    if value.ffmpeg_path != canonical {
        return false;
    }
    let Ok(metadata) = std::fs::metadata(ffmpeg) else {
        return false;
    };
    if value.ffmpeg_size_bytes != metadata.len()
        || value.platform_os != std::env::consts::OS
        || value.platform_arch != std::env::consts::ARCH
        || value.capability_algorithm != CAPABILITY_ALGORITHM
        || value.gpu_driver_summary != gpu_driver_summary()
    {
        return false;
    }
    if value.ffmpeg_mtime_ns
        != metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|value| value.as_nanos())
            .unwrap_or_default()
    {
        return false;
    }
    version(ffmpeg).await.ok().is_some_and(|value_now| {
        value_now == value.ffmpeg_version
            && version_digest(&value_now) == value.ffmpeg_version_digest
    })
}
