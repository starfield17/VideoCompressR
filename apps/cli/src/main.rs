use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;
use vc_core::{
    AudioMode, Codec, CompressionRatio, ContainerFormat, DecodeAcceleration, EncodeSettings,
    EncoderBackend, PreviewOptions, PreviewSampleMode,
};
use vc_runtime::storage::presets::PresetData;
use vc_runtime::{Application, PlanRequest, RuntimeError, Translator};

#[derive(Parser, Debug)]
#[command(name = "video-compressor", version, about = "Rust video compressor")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Plan(EncodeArgs),
    Encode(EncodeArgs),
    Preview(PreviewArgs),
    Preset {
        #[command(subcommand)]
        command: PresetCommand,
    },
}

#[derive(Clone, Debug, ValueEnum)]
#[value(rename_all = "lowercase")]
enum CodecArg {
    Hevc,
    Av1,
}
#[derive(Clone, Debug, ValueEnum)]
#[value(rename_all = "lowercase")]
enum BackendArg {
    Auto,
    Cpu,
    Nvenc,
    Qsv,
    Amf,
    Videotoolbox,
}
#[derive(Clone, Debug, ValueEnum)]
#[value(rename_all = "lowercase")]
enum DecodeArg {
    Software,
    Videotoolbox,
}
#[derive(Clone, Debug, ValueEnum)]
#[value(rename_all = "lowercase")]
enum ContainerArg {
    Mkv,
    Mp4,
}
#[derive(Clone, Debug, ValueEnum)]
#[value(rename_all = "lowercase")]
enum AudioArg {
    Copy,
    Aac,
}
#[derive(Clone, Debug, ValueEnum)]
#[value(rename_all = "lowercase")]
enum SampleModeArg {
    Middle,
    Custom,
}

#[derive(Args, Debug, Clone)]
struct EncodeArgs {
    input: Option<PathBuf>,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(long)]
    workdir: Option<PathBuf>,
    #[arg(long)]
    ffmpeg: Option<PathBuf>,
    #[arg(long)]
    ffprobe: Option<PathBuf>,
    #[arg(long)]
    preset: Option<String>,
    #[arg(long, value_parser = ["en", "zh_cn"])]
    lang: Option<String>,
    #[arg(long, action = ArgAction::SetTrue)]
    recursive: bool,
    #[arg(long = "no-recursive", action = ArgAction::SetTrue)]
    no_recursive: bool,
    #[arg(long, value_enum)]
    codec: Option<CodecArg>,
    #[arg(long, value_enum)]
    backend: Option<BackendArg>,
    #[arg(long = "decode-acceleration", value_enum)]
    decode_acceleration: Option<DecodeArg>,
    #[arg(long, action = ArgAction::SetTrue)]
    parallel: bool,
    #[arg(long = "parallel-backends")]
    parallel_backends: Option<String>,
    #[arg(long)]
    ratio: Option<f64>,
    #[arg(long = "min-video-kbps")]
    min_video_kbps: Option<u64>,
    #[arg(long = "max-video-kbps")]
    max_video_kbps: Option<u64>,
    #[arg(long, value_enum)]
    container: Option<ContainerArg>,
    #[arg(long = "audio-mode", value_enum)]
    audio_mode: Option<AudioArg>,
    #[arg(long = "audio-bitrate")]
    audio_bitrate: Option<String>,
    #[arg(long = "copy-subtitles", action = ArgAction::SetTrue)]
    copy_subtitles: bool,
    #[arg(long = "no-copy-subtitles", action = ArgAction::SetTrue)]
    no_copy_subtitles: bool,
    #[arg(long = "copy-external-subtitles", action = ArgAction::SetTrue)]
    copy_external_subtitles: bool,
    #[arg(long = "no-copy-external-subtitles", action = ArgAction::SetTrue)]
    no_copy_external_subtitles: bool,
    #[arg(long = "two-pass", action = ArgAction::SetTrue)]
    two_pass: bool,
    #[arg(long = "no-two-pass", action = ArgAction::SetTrue)]
    no_two_pass: bool,
    #[arg(long, action = ArgAction::SetTrue)]
    overwrite: bool,
    #[arg(long = "no-overwrite", action = ArgAction::SetTrue)]
    no_overwrite: bool,
    #[arg(long = "encoder-preset")]
    encoder_preset: Option<String>,
    #[arg(long = "pix-fmt")]
    pixel_format: Option<String>,
    #[arg(long = "maxrate-factor")]
    maxrate_factor: Option<f64>,
    #[arg(long = "bufsize-factor")]
    bufsize_factor: Option<f64>,
    #[arg(long, action = ArgAction::SetTrue)]
    dry_run: bool,
}

#[derive(Args, Debug, Clone)]
struct PreviewArgs {
    #[command(flatten)]
    encode: EncodeArgs,
    #[arg(long = "sample-mode", value_enum, default_value = "middle")]
    sample_mode: SampleModeArg,
    #[arg(long = "sample-duration", default_value_t = 30.0)]
    sample_duration_sec: f64,
    #[arg(long = "sample-start")]
    custom_start_sec: Option<f64>,
}

#[derive(Subcommand, Debug, Clone)]
enum PresetCommand {
    List {
        #[arg(long, value_parser = ["en", "zh_cn"])]
        lang: Option<String>,
    },
    Load {
        name: String,
        #[arg(long, value_parser = ["en", "zh_cn"])]
        lang: Option<String>,
    },
    Delete {
        name: String,
        #[arg(long, value_parser = ["en", "zh_cn"])]
        lang: Option<String>,
    },
    Save {
        name: String,
        #[command(flatten)]
        options: Box<EncodeArgs>,
    },
}

fn backend(value: BackendArg) -> EncoderBackend {
    match value {
        BackendArg::Auto => EncoderBackend::Auto,
        BackendArg::Cpu => EncoderBackend::Cpu,
        BackendArg::Nvenc => EncoderBackend::Nvenc,
        BackendArg::Qsv => EncoderBackend::Qsv,
        BackendArg::Amf => EncoderBackend::Amf,
        BackendArg::Videotoolbox => EncoderBackend::VideoToolbox,
    }
}
fn codec(value: CodecArg) -> Codec {
    match value {
        CodecArg::Hevc => Codec::Hevc,
        CodecArg::Av1 => Codec::Av1,
    }
}
fn decode(value: DecodeArg) -> DecodeAcceleration {
    match value {
        DecodeArg::Software => DecodeAcceleration::Software,
        DecodeArg::Videotoolbox => DecodeAcceleration::VideoToolbox,
    }
}
fn container(value: ContainerArg) -> ContainerFormat {
    match value {
        ContainerArg::Mkv => ContainerFormat::Mkv,
        ContainerArg::Mp4 => ContainerFormat::Mp4,
    }
}
fn audio(value: AudioArg) -> AudioMode {
    match value {
        AudioArg::Copy => AudioMode::Copy,
        AudioArg::Aac => AudioMode::Aac,
    }
}

fn merge_options(
    mut settings: EncodeSettings,
    args: &EncodeArgs,
) -> Result<EncodeSettings, RuntimeError> {
    if let Some(value) = &args.codec {
        settings.codec = codec(value.clone());
    }
    if let Some(value) = &args.backend {
        settings.backend = backend(value.clone());
    }
    if let Some(value) = &args.decode_acceleration {
        settings.decode_acceleration = decode(value.clone());
    }
    if args.recursive {
        settings.recursive = true;
    }
    if args.no_recursive {
        settings.recursive = false;
    }
    if args.parallel {
        settings.parallel_enabled = true;
    }
    if let Some(raw) = &args.parallel_backends {
        settings.parallel_backends = raw
            .split(',')
            .filter(|value| !value.trim().is_empty())
            .map(|value| match value.trim().to_ascii_lowercase().as_str() {
                "auto" => Ok(EncoderBackend::Auto),
                "cpu" => Ok(EncoderBackend::Cpu),
                "nvenc" => Ok(EncoderBackend::Nvenc),
                "qsv" => Ok(EncoderBackend::Qsv),
                "amf" => Ok(EncoderBackend::Amf),
                "videotoolbox" => Ok(EncoderBackend::VideoToolbox),
                _ => Err(RuntimeError::Planning(format!("invalid parallel backend: {value}"))),
            })
            .collect::<Result<Vec<_>, _>>()?;
    }
    if let Some(value) = args.ratio {
        settings.ratio = Some(
            CompressionRatio::new(value)
                .map_err(|error| RuntimeError::Planning(error.to_string()))?,
        );
    }
    if let Some(value) = args.min_video_kbps {
        settings.min_video_kbps = value;
    }
    if let Some(value) = args.max_video_kbps {
        settings.max_video_kbps = value;
    }
    if let Some(value) = &args.container {
        settings.container = container(value.clone());
    }
    if let Some(value) = &args.audio_mode {
        settings.audio_mode = audio(value.clone());
    }
    if let Some(value) = &args.audio_bitrate {
        settings.audio_bitrate = value.clone();
    }
    if args.copy_subtitles {
        settings.copy_subtitles = true;
    }
    if args.no_copy_subtitles {
        settings.copy_subtitles = false;
    }
    if args.copy_external_subtitles {
        settings.copy_external_subtitles = true;
    }
    if args.no_copy_external_subtitles {
        settings.copy_external_subtitles = false;
    }
    if args.two_pass {
        settings.two_pass = true;
    }
    if args.no_two_pass {
        settings.two_pass = false;
    }
    if args.overwrite {
        settings.overwrite = true;
    }
    if args.no_overwrite {
        settings.overwrite = false;
    }
    if let Some(value) = &args.encoder_preset {
        if settings.backend == EncoderBackend::Auto {
            return Err(RuntimeError::Planning(
                "--encoder-preset cannot be used with --backend auto.".into(),
            ));
        }
        settings.encoder_preset = Some(value.clone());
    }
    if let Some(value) = &args.pixel_format {
        settings.pixel_format = value.clone();
    }
    if let Some(value) = args.maxrate_factor {
        settings.maxrate_factor = value;
    }
    if let Some(value) = args.bufsize_factor {
        settings.bufsize_factor = value;
    }
    settings.dry_run = args.dry_run;
    if settings.backend == EncoderBackend::Auto
        && args.encoder_preset.is_none()
        && settings.encoder_preset.is_some()
    {
        eprintln!("Warning: inherited encoder preset ignored because backend is auto.");
        settings.encoder_preset = None;
    }
    Ok(settings)
}

fn request(args: &EncodeArgs, settings: EncodeSettings) -> Result<PlanRequest, RuntimeError> {
    Ok(PlanRequest {
        input_path: args.input.clone().ok_or_else(|| {
            RuntimeError::Planning("an input file or directory is required for this command".into())
        })?,
        output_dir: args.output.clone(),
        workdir: args.workdir.clone(),
        ffmpeg_path: args.ffmpeg.clone(),
        ffprobe_path: args.ffprobe.clone(),
        settings,
        force_capability_refresh: false,
    })
}

fn print_plan(plan: &vc_runtime::EncodePlan, translator: &Translator) {
    println!(
        "{}",
        translator.format("cli.plan_header", &[("count", &plan.items.len().to_string())])
    );
    for item in &plan.items {
        if let Some(reason) = &item.skip_reason {
            println!(
                "{} {}\n  {}: {}\n  {}: {}",
                translator.text("cli.plan_skip"),
                item.source_path.display(),
                translator.text("cli.reason"),
                reason.0,
                translator.text("cli.output"),
                item.output_path.display()
            );
            continue;
        }
        let media = item.media_info.as_ref();
        let encoder = item.encoder.as_ref();
        println!("{} {}", translator.text("cli.plan_ready"), item.source_path.display());
        println!(
            "  {}: {}",
            translator.text("cli.resolution"),
            media
                .and_then(|value| value.width.zip(value.height).map(|(w, h)| format!("{w}x{h}")))
                .unwrap_or_else(|| "n/a".into())
        );
        println!(
            "  FPS: {}",
            media
                .and_then(|value| value.fps)
                .map(|value| format!("{value:.3}"))
                .unwrap_or_else(|| "n/a".into())
        );
        println!(
            "  {}: {} kbps",
            translator.text("cli.source_bitrate"),
            media.map(|value| value.video_bitrate_bps / 1000).unwrap_or_default()
        );
        println!(
            "  {}: {} kbps",
            translator.text("cli.target_bitrate"),
            item.target_video_bitrate_bps / 1000
        );
        println!(
            "  {}: {} ({})",
            translator.text("cli.encoder"),
            encoder.map(|value| value.encoder_name.as_str()).unwrap_or("n/a"),
            encoder.map(|value| value.backend.as_str()).unwrap_or("n/a")
        );
        println!("  {}: {}", translator.text("cli.output"), item.output_path.display());
        for warning in &item.warnings {
            println!("  Note: {}", warning.0);
        }
    }
    println!(
        "{}: {}\n{}: {}\n{}: {}",
        translator.text("cli.ffmpeg"),
        plan.ffmpeg_path.display(),
        translator.text("cli.ffprobe"),
        plan.ffprobe_path.display(),
        translator.text("cli.output_root"),
        plan.output_root.display()
    );
}

fn print_results(results: &[vc_runtime::ExecutionResult]) {
    for result in results {
        if result.item_result.skipped {
            println!(
                "[SKIPPED] {}\n  Reason: {}",
                result.source_path.display(),
                result.item_result.error.as_deref().unwrap_or("")
            );
        } else if result.item_result.success {
            println!("[OK] {} -> {}", result.source_path.display(), result.output_path.display());
        } else {
            println!(
                "[FAILED] {}\n  Reason: {}",
                result.source_path.display(),
                result.item_result.error.as_deref().unwrap_or("")
            );
        }
        for path in &result.copied_external_subtitle_paths {
            println!("  External subtitle copied: {}", path.display());
        }
        for warning in &result.external_subtitle_warnings {
            println!("  External subtitle warning: {warning}");
        }
        if let Some(path) = &result.item_result.log_path {
            println!("  Log path: {}", path.display());
        }
    }
}

async fn execute_command(
    app: &Application,
    args: EncodeArgs,
    encode: bool,
) -> Result<i32, RuntimeError> {
    if args.input.is_none() {
        return Err(RuntimeError::Planning(
            "an input file or directory is required for this command".into(),
        ));
    }
    let base = if let Some(name) = &args.preset {
        app.presets.load(name)?
    } else {
        app.default_settings()?
    };
    let settings = merge_options(base, &args)?;
    let plan = app.plan(request(&args, settings.clone())?).await?;
    let config = app.config()?;
    let language = args.lang.as_deref().unwrap_or(config.language.as_str());
    let translator = app.translator(language)?;
    print_plan(&plan, &translator);
    if !encode || settings.dry_run {
        return Ok(0);
    }
    let token = CancellationToken::new();
    let worker = tokio::spawn({
        let app = app.clone();
        let plan = plan.clone();
        let token = token.clone();
        async move { app.encode(&plan, token).await }
    });
    let results = tokio::select! { value = worker => value.map_err(|error| RuntimeError::Encode(error.to_string()))??, signal = tokio::signal::ctrl_c() => { let _ = signal; token.cancel(); return Err(RuntimeError::Cancelled); } };
    print_results(&results);
    Ok(if results.iter().all(|value| value.item_result.success || value.item_result.skipped) {
        0
    } else {
        5
    })
}

async fn preview_command(app: &Application, args: PreviewArgs) -> Result<i32, RuntimeError> {
    if args.encode.input.is_none() {
        return Err(RuntimeError::Planning("an input file is required for preview".into()));
    }
    let base = if let Some(name) = &args.encode.preset {
        app.presets.load(name)?
    } else {
        app.default_settings()?
    };
    let settings = merge_options(base, &args.encode)?;
    if settings.parallel_enabled {
        return Err(RuntimeError::Planning("Preview does not support parallel mode.".into()));
    }
    let options = PreviewOptions {
        sample_mode: match args.sample_mode {
            SampleModeArg::Middle => PreviewSampleMode::Middle,
            SampleModeArg::Custom => PreviewSampleMode::Custom,
        },
        sample_duration_sec: args.sample_duration_sec,
        custom_start_sec: args.custom_start_sec,
    };
    let result =
        app.preview(request(&args.encode, settings)?, options, CancellationToken::new()).await?;
    if !result.success {
        println!("[FAILED] {}", result.error_message.as_deref().unwrap_or("Preview failed"));
        return Ok(5);
    }
    let config = app.config()?;
    let language = args.encode.lang.as_deref().unwrap_or(config.language.as_str());
    let translator = app.translator(language)?;
    println!(
        "{} {}\n  {}: {}\n  {}: {}\n  {}: {:.3}\n  {}: {} bytes",
        translator.text("cli.preview_success"),
        result.job.source_path.display(),
        translator.text("cli.sample_source"),
        result.job.source_sample_path.display(),
        translator.text("cli.sample_encoded"),
        result.job.encoded_sample_path.display(),
        translator.text("cli.sample_ratio"),
        result.sample_compression_ratio,
        translator.text("cli.estimated_output"),
        result.estimated_full_output_size
    );
    Ok(0)
}

async fn preset_command(app: &Application, command: PresetCommand) -> Result<i32, RuntimeError> {
    match command {
        PresetCommand::List { .. } => {
            for name in app.presets.list()? {
                println!("{name}");
            }
        }
        PresetCommand::Load { name, .. } => {
            let data = PresetData::from(&app.presets.load(&name)?);
            println!("{}", serde_json::to_string_pretty(&data)?);
        }
        PresetCommand::Delete { name, .. } => {
            app.presets.delete(&name)?;
            println!("Preset '{name}' deleted");
        }
        PresetCommand::Save { name, options } => {
            let base = if let Some(preset) = &options.preset {
                app.presets.load(preset)?
            } else {
                app.default_settings()?
            };
            let settings = merge_options(base, &options)?;
            let path = app.presets.save(&name, &settings)?;
            println!("Preset '{name}' saved to {}", path.display());
        }
    }
    Ok(0)
}

fn exit_code(error: &RuntimeError) -> i32 {
    match error {
        RuntimeError::Cancelled => 130,
        RuntimeError::ToolDiscovery(_) | RuntimeError::Capability(_) => 3,
        RuntimeError::Planning(_) | RuntimeError::Probe(_) => 4,
        RuntimeError::Encode(_) => 5,
        _ => 2,
    }
}

async fn run() -> i32 {
    let cli = Cli::parse();
    let app = match Application::current() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("Error: {error}");
            return exit_code(&error);
        }
    };
    let result = match cli.command {
        Command::Plan(args) => execute_command(&app, args, false).await,
        Command::Encode(args) => execute_command(&app, args, true).await,
        Command::Preview(args) => preview_command(&app, args).await,
        Command::Preset { command } => preset_command(&app, command).await,
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Error: {error}");
            exit_code(&error)
        }
    }
}

#[tokio::main]
async fn main() {
    std::process::exit(run().await);
}
