use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio_util::sync::CancellationToken;
use vc_core::{
    EncodeSettings, EncoderBackend, PreviewJob, PreviewOptions, PreviewSampleMode,
    choose_sample_window,
};
use vc_runtime::ActivityHub;
use vc_runtime::AppPaths;
use vc_runtime::execution::{ProgressEvent, ProgressSink, execute_item, execute_preview};
use vc_runtime::ffmpeg::command::render_encode_commands;
use vc_runtime::ffmpeg::process::{OutputStream, ToolRequest, run_capture, run_streaming};
use vc_runtime::ffmpeg::progress::{ProgressParser, progress_percent};
use vc_runtime::planning::{PlanRequest, PlanningService};
use vc_runtime::storage::app_config::AppConfig;
use vc_runtime::storage::presets::PresetStore;
use vc_runtime::storage::settings::SettingsStore;
use vc_runtime::storage::window_state::{WindowGeometry, WindowStateStore};

fn fake(name: &str) -> PathBuf {
    let filename = if cfg!(windows) { format!("{name}.ps1") } else { format!("{name}.sh") };
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/fake-tools").join(filename)
}

#[test]
fn progress_parser_handles_split_machine_protocol_lines() {
    let mut parser = ProgressParser::default();
    assert!(parser.push("out_time_us=500").is_empty());
    let updates = parser.push("000\nspeed=2x\nprogress=end\n");
    assert_eq!(updates.len(), 1);
    assert!(updates[0].is_end);
    assert_eq!(progress_percent(&updates[0], Some(1.0)), Some(50.0));
}

#[tokio::test]
async fn fake_executable_covers_capture_and_streaming_pipe_contract() {
    let request = ToolRequest {
        program: fake("fake-ffmpeg"),
        args: vec![OsString::from("-version")],
        cwd: None,
    };
    let (code, stdout, _) = run_capture(request, CancellationToken::new()).await.expect("capture");
    assert_eq!(code, 0);
    assert!(stdout.contains("fake-1.0"));

    let lines = Arc::new(Mutex::new(Vec::new()));
    let sink = lines.clone();
    let request = ToolRequest {
        program: fake("fake-ffmpeg"),
        args: vec![OsString::from("-progress"), OsString::from("pipe:1")],
        cwd: None,
    };
    let result = run_streaming(request, CancellationToken::new(), move |line| {
        sink.lock().expect("line lock").push((line.stream, line.text))
    })
    .await
    .expect("stream");
    assert_eq!(result.code, 0);
    let captured = lines.lock().expect("line lock");
    assert!(
        captured
            .iter()
            .any(|(stream, line)| *stream == OutputStream::Stdout && line == "progress=end")
    );
}

#[tokio::test]
async fn fake_hang_is_stopped_by_process_tree_cancellation() {
    let token = CancellationToken::new();
    let worker_token = token.clone();
    let request = ToolRequest { program: fake("fake-hang"), args: Vec::new(), cwd: None };
    let worker = tokio::spawn(async move { run_streaming(request, worker_token, |_| {}).await });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    token.cancel();
    let result = worker.await.expect("cancel worker").expect("process result");
    assert!(result.cancelled);
}

#[tokio::test]
async fn capture_hang_is_cancellable_too() {
    let token = CancellationToken::new();
    let worker_token = token.clone();
    let request = ToolRequest { program: fake("fake-hang"), args: Vec::new(), cwd: None };
    let worker = tokio::spawn(async move { run_capture(request, worker_token).await });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    token.cancel();
    let result = worker.await.expect("cancel capture");
    assert!(matches!(result, Err(vc_runtime::RuntimeError::Cancelled)));
}

#[tokio::test]
async fn fake_nonzero_exit_and_stderr_are_preserved() {
    let request = ToolRequest { program: fake("fake-fail"), args: Vec::new(), cwd: None };
    let lines = Arc::new(Mutex::new(Vec::new()));
    let sink = lines.clone();
    let result = run_streaming(request, CancellationToken::new(), move |line| {
        sink.lock().expect("line lock").push((line.stream, line.text))
    })
    .await
    .expect("process result");
    assert_eq!(result.code, 17);
    assert!(
        lines
            .lock()
            .expect("line lock")
            .iter()
            .any(|(stream, text)| *stream == OutputStream::Stderr && text.contains("fake failure"))
    );
}

#[tokio::test]
async fn fake_tools_produce_a_real_runtime_plan_without_reference_tree() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let source = temp.path().join("movie with space.mp4");
    std::fs::write(&source, b"fixture").expect("source");
    let service = PlanningService::new(paths.clone());
    let settings = EncodeSettings {
        backend: EncoderBackend::Cpu,
        overwrite: true,
        ..EncodeSettings::default()
    };
    let plan = service
        .plan(PlanRequest {
            input_path: source.clone(),
            output_dir: Some(temp.path().join("out")),
            workdir: None,
            ffmpeg_path: Some(fake("fake-ffmpeg")),
            ffprobe_path: Some(fake("fake-ffprobe")),
            settings,
            force_capability_refresh: true,
        })
        .await
        .expect("plan");
    assert_eq!(plan.items.len(), 1);
    assert!(plan.items[0].is_ready());
    assert_eq!(plan.items[0].encoder.as_ref().expect("encoder").encoder_name, "libx265");
    let commands =
        render_encode_commands(&plan.ffmpeg_path, &plan.items[0], &paths, None, None, "encode")
            .expect("commands");
    let flattened = commands[0]
        .args
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(flattened.windows(2).any(|window| window == ["-progress", "pipe:1"]));
    assert!(flattened.iter().any(|value| value.contains("movie with space.mp4")));
}

#[tokio::test]
async fn failed_overwrite_restores_the_original_output() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let source = temp.path().join("movie.mp4");
    std::fs::write(&source, b"fixture").expect("source");
    let service = PlanningService::new(paths.clone());
    let settings = EncodeSettings {
        backend: EncoderBackend::Cpu,
        overwrite: true,
        ..EncodeSettings::default()
    };
    let plan = service
        .plan(PlanRequest {
            input_path: source.clone(),
            output_dir: Some(temp.path().join("out")),
            workdir: None,
            ffmpeg_path: Some(fake("fake-ffmpeg")),
            ffprobe_path: Some(fake("fake-ffprobe")),
            settings,
            force_capability_refresh: true,
        })
        .await
        .expect("plan");
    let item = &plan.items[0];
    std::fs::write(&item.output_path, b"original output").expect("existing output");
    let result = execute_item(
        item,
        &fake("fake-fail"),
        &paths,
        &ActivityHub::new(),
        CancellationToken::new(),
        None,
        1,
        1,
        None,
    )
    .await
    .expect("failed encode is an item result");
    assert!(!result.item_result.success);
    assert_eq!(std::fs::read(&item.output_path).expect("restored output"), b"original output");
    assert!(!paths.temp_dir.read_dir().expect("temp dir").any(|entry| {
        entry.expect("entry").file_name().to_string_lossy().contains("overwrite-backup")
    }));
}

#[tokio::test]
async fn preview_uses_the_same_progress_protocol_and_keeps_successful_samples() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let source = temp.path().join("preview movie.mp4");
    std::fs::write(&source, b"fixture").expect("source");
    let service = PlanningService::new(paths.clone());
    let plan = service
        .plan(PlanRequest {
            input_path: source.clone(),
            output_dir: Some(temp.path().join("out")),
            workdir: None,
            ffmpeg_path: Some(fake("fake-ffmpeg")),
            ffprobe_path: Some(fake("fake-ffprobe")),
            settings: EncodeSettings {
                backend: EncoderBackend::Cpu,
                overwrite: true,
                ..EncodeSettings::default()
            },
            force_capability_refresh: true,
        })
        .await
        .expect("plan");
    let job = PreviewJob {
        source_path: source,
        source_sample_path: paths.previews_dir.join("source-sample.mp4"),
        encoded_sample_path: paths.previews_dir.join("encoded-sample.mp4"),
        window: choose_sample_window(2.0, &PreviewOptions::default()).expect("window"),
        plan_item: plan.items[0].clone(),
    };
    let events = Arc::new(Mutex::new(Vec::<ProgressEvent>::new()));
    let sink_events = events.clone();
    let sink: ProgressSink = Arc::new(move |event| {
        sink_events.lock().expect("events").push(event);
    });
    let result = execute_preview(
        &job,
        &fake("fake-ffmpeg"),
        &paths,
        &ActivityHub::new(),
        CancellationToken::new(),
        Some(sink),
    )
    .await
    .expect("preview");
    assert!(result.success);
    assert!(job.source_sample_path.is_file());
    assert!(job.encoded_sample_path.is_file());
    assert!(events.lock().expect("events").iter().any(|event| event.stage == "preview"));
    assert_eq!(PreviewSampleMode::Middle.as_str(), "middle");
}

#[tokio::test]
async fn two_pass_commands_keep_the_typed_progress_and_passlog_contract() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let source = temp.path().join("two-pass.mp4");
    std::fs::write(&source, b"fixture").expect("source");
    let service = PlanningService::new(paths.clone());
    let plan = service
        .plan(PlanRequest {
            input_path: source,
            output_dir: Some(temp.path().join("out")),
            workdir: None,
            ffmpeg_path: Some(fake("fake-ffmpeg")),
            ffprobe_path: Some(fake("fake-ffprobe")),
            settings: EncodeSettings {
                backend: EncoderBackend::Cpu,
                overwrite: true,
                two_pass: true,
                ..EncodeSettings::default()
            },
            force_capability_refresh: true,
        })
        .await
        .expect("plan");
    let commands =
        render_encode_commands(&plan.ffmpeg_path, &plan.items[0], &paths, None, None, "encode")
            .expect("commands");
    assert_eq!(commands.len(), 2);
    let first = commands[0].args.iter().map(|value| value.to_string_lossy()).collect::<Vec<_>>();
    let second = commands[1].args.iter().map(|value| value.to_string_lossy()).collect::<Vec<_>>();
    assert!(first.windows(2).any(|pair| pair == ["-progress", "pipe:1"]));
    assert!(first.windows(2).any(|pair| pair == ["-pass", "1"]));
    assert!(second.windows(2).any(|pair| pair == ["-pass", "2"]));
    assert!(first.iter().any(|value| value.ends_with("encode.ffpass")));
}

#[tokio::test]
async fn external_subtitle_sidecars_are_copied_with_the_encoded_stem() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let source = temp.path().join("movie.mp4");
    std::fs::write(&source, b"fixture").expect("source");
    std::fs::write(temp.path().join("movie.en.srt"), b"1\n00:00:00,000 --> 00:00:01,000\nHi\n")
        .expect("subtitle");
    let plan = PlanningService::new(paths.clone())
        .plan(PlanRequest {
            input_path: source,
            output_dir: Some(temp.path().join("out")),
            workdir: None,
            ffmpeg_path: Some(fake("fake-ffmpeg")),
            ffprobe_path: Some(fake("fake-ffprobe")),
            settings: EncodeSettings {
                backend: EncoderBackend::Cpu,
                overwrite: true,
                ..EncodeSettings::default()
            },
            force_capability_refresh: true,
        })
        .await
        .expect("plan");
    let result = execute_item(
        &plan.items[0],
        &plan.ffmpeg_path,
        &paths,
        &ActivityHub::new(),
        CancellationToken::new(),
        None,
        1,
        1,
        None,
    )
    .await
    .expect("encode");
    assert!(result.item_result.success);
    assert_eq!(result.copied_external_subtitle_paths.len(), 1);
    assert!(result.copied_external_subtitle_paths[0].is_file());
    assert!(
        result.copied_external_subtitle_paths[0]
            .file_name()
            .expect("subtitle name")
            .to_string_lossy()
            .starts_with("movie_hevc")
    );
}

#[test]
fn corrupt_app_config_is_renamed_and_replaced_with_defaults() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let path = AppConfig::path(&paths);
    std::fs::write(&path, "not-json").expect("corrupt config");
    let config = AppConfig::load(&paths).expect("recovery");
    assert_eq!(config.language, "en");
    assert!(!path.exists());
    assert!(
        temp.path().join("config").read_dir().expect("config dir").any(|entry| entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .contains("broken-"))
    );
}

#[test]
fn legacy_workdir_config_is_migrated_without_deleting_the_source() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    std::fs::write(
        paths.workdir.join("app_config.json"),
        r#"{"language":"zh_cn","default_preset_name":"default_av1","recent_paths":["movie.mp4"]}"#,
    )
    .expect("legacy config");
    let value = AppConfig::load(&paths).expect("migration");
    assert_eq!(value.language, "zh_cn");
    assert_eq!(value.default_preset_name.as_deref(), Some("default_av1"));
    assert!(AppConfig::path(&paths).is_file());
    assert!(paths.workdir.join("app_config.json").is_file());
    assert!(paths.workdir.read_dir().expect("workdir").any(|entry| {
        entry.expect("entry").file_name().to_string_lossy().contains("migrated-")
    }));
}

#[test]
fn settings_preserve_direct_legacy_json_and_presets_get_a_schema_version() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let settings = EncodeSettings::default();
    std::fs::write(
        SettingsStore::new(paths.clone()).path(),
        serde_json::to_vec_pretty(&settings).expect("legacy settings"),
    )
    .expect("write settings");
    assert_eq!(SettingsStore::new(paths.clone()).load().expect("load settings"), Some(settings));

    let presets = PresetStore::new(paths.clone());
    presets.ensure_defaults().expect("default presets");
    let data: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(paths.presets_dir.join("default_hevc.json")).expect("preset"),
    )
    .expect("preset json");
    assert_eq!(data["schema_version"].as_u64(), Some(2));
}

#[test]
fn window_state_round_trips_geometry() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let store = WindowStateStore::new(paths);
    let mut state = store.load().expect("default state");
    state.windows.insert(
        "main".into(),
        WindowGeometry { width: 1100, height: 760, x: Some(10), y: Some(20), maximized: false },
    );
    store.save(state.clone()).expect("save state");
    assert_eq!(store.load().expect("load state"), state);
}

#[test]
fn activity_history_can_be_cleared_and_exported() {
    let temp = tempfile::tempdir().expect("temp");
    let activity = ActivityHub::new();
    activity.emit("process", "encoded fixture");
    let export = temp.path().join("activity.log");
    activity.export(&export).expect("export activity");
    let text = std::fs::read_to_string(&export).expect("read activity");
    assert!(text.contains("[process] encoded fixture"));
    activity.clear();
    assert!(activity.history().is_empty());
}
