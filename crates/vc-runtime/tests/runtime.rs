use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio_util::sync::CancellationToken;
use vc_core::queue::{QueueItemStatus, QueueRunState, validate_queue_state};
use vc_core::{
    EncodePlanItem, EncodeSettings, EncoderBackend, PreviewJob, PreviewOptions, PreviewSampleMode,
    choose_sample_window,
};
use vc_runtime::ActivityHub;
use vc_runtime::AppPaths;
use vc_runtime::Application;
use vc_runtime::execution::{
    ProgressEvent, ProgressSink, execute_item, execute_plan, execute_preview,
};
use vc_runtime::ffmpeg::ToolPaths;
use vc_runtime::ffmpeg::command::render_encode_commands;
use vc_runtime::ffmpeg::probe::ffprobe_json;
use vc_runtime::ffmpeg::process::{
    OutputStream, ToolRequest, run_capture, run_capture_exact, run_streaming,
};
use vc_runtime::ffmpeg::progress::{ProgressParser, progress_percent};
use vc_runtime::planning::{EncodePlan, PlanRequest, PlanningService};
use vc_runtime::queue::supervisor::{ExecutionContext, QueueSupervisor};
use vc_runtime::storage::app_config::AppConfig;
use vc_runtime::storage::presets::PresetStore;
use vc_runtime::storage::settings::SettingsStore;
use vc_runtime::storage::window_state::{WindowGeometry, WindowStateStore};

fn fake(name: &str) -> PathBuf {
    let filename = if cfg!(windows) { format!("{name}.ps1") } else { format!("{name}.sh") };
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/fake-tools").join(filename)
}

async fn parallel_items(
    paths: &AppPaths,
    planning_ffmpeg: PathBuf,
    names: &[&str],
    backends: Vec<EncoderBackend>,
) -> Vec<EncodePlanItem> {
    let service = PlanningService::new(paths.clone());
    let mut items = Vec::with_capacity(names.len());
    for name in names {
        let source = paths.root.join(name);
        std::fs::write(&source, b"fixture").expect("source");
        let mut plan = service
            .plan(PlanRequest {
                input_path: source,
                output_dir: Some(paths.root.join("out")),
                workdir: None,
                ffmpeg_path: Some(planning_ffmpeg.clone()),
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
        let mut item = plan.items.remove(0);
        assert!(item.is_ready(), "parallel fixture plan was skipped: {:?}", item.skip_reason);
        item.settings.parallel_enabled = true;
        item.settings.parallel_backends = backends.clone();
        item.settings.encoder_preset = None;
        items.push(item);
    }
    items
}

async fn wait_for_idle(
    supervisor: &QueueSupervisor,
) -> vc_runtime::queue::supervisor::QueueSnapshot {
    let mut receiver = supervisor.subscribe();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let snapshot = supervisor.snapshot().await;
        if snapshot.state.run_state == QueueRunState::Idle && snapshot.state.active_run_id.is_none()
        {
            return (*snapshot).clone();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "queue did not become idle: state={:?}, active_run_id={:?}, items={:?}",
            snapshot.state.run_state,
            snapshot.state.active_run_id,
            snapshot
                .state
                .items
                .iter()
                .map(|item| (&item.status, &item.run_id))
                .collect::<Vec<_>>()
        );
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            tokio::time::timeout(remaining, receiver.changed()).await.is_ok(),
            "queue did not become idle before the deadline"
        );
    }
}

async fn wait_for_running(supervisor: &QueueSupervisor) {
    let mut receiver = supervisor.subscribe();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let snapshot = supervisor.snapshot().await;
        if snapshot.metrics.running_items >= 1 {
            return;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            tokio::time::timeout(remaining, receiver.changed()).await.is_ok(),
            "parallel queue did not start an item before the deadline"
        );
    }
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
        sink.lock().expect("line lock").push((line.stream, line.text));
        async { Ok(()) }
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
async fn exact_capture_preserves_more_than_channel_capacity_lines() {
    let request = ToolRequest { program: fake("fake-large-output"), args: Vec::new(), cwd: None };
    let output = run_capture_exact(request, CancellationToken::new()).await.expect("exact capture");

    assert_eq!(output.code, 0);
    assert_eq!(output.stdout.lines().count(), 10_000);
    assert_eq!(output.stderr.lines().count(), 10_000);
    assert!(output.stdout.lines().next().is_some_and(|line| line == "stdout-1"));
    assert!(output.stdout.lines().last().is_some_and(|line| line == "stdout-10000"));
    assert!(output.stderr.lines().next().is_some_and(|line| line == "stderr-1"));
    assert!(output.stderr.lines().last().is_some_and(|line| line == "stderr-10000"));
}

#[tokio::test]
async fn stderr_lines_are_not_dropped_under_backpressure() {
    let request = ToolRequest { program: fake("fake-large-output"), args: Vec::new(), cwd: None };
    let lines = Arc::new(Mutex::new(Vec::new()));
    let sink = lines.clone();
    run_streaming(request, CancellationToken::new(), move |line| {
        let sink = sink.clone();
        async move {
            tokio::task::yield_now().await;
            sink.lock().expect("line lock").push((line.stream, line.text));
            Ok(())
        }
    })
    .await
    .expect("stream");
    let captured = lines.lock().expect("line lock");
    assert_eq!(
        captured.iter().filter(|(stream, _)| *stream == OutputStream::Stderr).count(),
        10_000
    );
    assert!(
        captured
            .iter()
            .any(|(stream, text)| { *stream == OutputStream::Stderr && text == "stderr-10000" })
    );
}

#[tokio::test]
async fn invalid_utf8_does_not_terminate_process_reader() {
    let request = ToolRequest { program: fake("fake-invalid-utf8"), args: Vec::new(), cwd: None };
    let output =
        run_capture_exact(request, CancellationToken::new()).await.expect("capture invalid utf8");

    assert_eq!(output.stdout.lines().count(), 2);
    assert_eq!(output.stderr.lines().count(), 2);
    assert!(output.stdout.contains("stdout-after"));
    assert!(output.stderr.contains("stderr-after"));
}

#[tokio::test]
async fn large_ffprobe_json_is_not_truncated() {
    let value = ffprobe_json(&fake("fake-large-ffprobe"), Path::new("fixture.mp4"))
        .await
        .expect("large ffprobe json");
    assert_eq!(value["format"]["metadata"]["key-1"], "value-1");
    assert_eq!(value["format"]["metadata"]["key-10000"], "value-10000");
}

#[tokio::test]
async fn fake_hang_is_stopped_by_process_tree_cancellation() {
    let token = CancellationToken::new();
    let worker_token = token.clone();
    let request = ToolRequest { program: fake("fake-hang"), args: Vec::new(), cwd: None };
    let worker =
        tokio::spawn(
            async move { run_streaming(request, worker_token, |_| async { Ok(()) }).await },
        );
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
        sink.lock().expect("line lock").push((line.stream, line.text));
        async { Ok(()) }
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
    assert!(plan.items[0].is_ready(), "fixture plan was skipped: {:?}", plan.items[0].skip_reason);
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
async fn directory_plan_skips_one_probe_failure_and_continues() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let input = temp.path().join("directory");
    std::fs::create_dir(&input).expect("input directory");
    for name in ["valid-a.mp4", "broken.mp4", "valid-b.mp4"] {
        std::fs::write(input.join(name), b"fixture").expect("source");
    }

    let plan = PlanningService::new(paths.clone())
        .plan(PlanRequest {
            input_path: input,
            output_dir: Some(temp.path().join("out")),
            workdir: None,
            ffmpeg_path: Some(fake("fake-ffmpeg")),
            ffprobe_path: Some(fake("fake-ffprobe-directory")),
            settings: EncodeSettings {
                backend: EncoderBackend::Cpu,
                overwrite: true,
                ..EncodeSettings::default()
            },
            force_capability_refresh: true,
        })
        .await
        .expect("directory plan");

    assert_eq!(plan.items.len(), 3);
    assert_eq!(plan.items.iter().filter(|item| item.is_ready()).count(), 2);
    let skipped =
        plan.items.iter().find(|item| item.skip_reason.is_some()).expect("broken item skipped");
    assert!(skipped.source_path.ends_with("broken.mp4"));
    assert!(
        skipped.skip_reason.as_ref().expect("skip reason").0.contains("ffprobe fixture failure")
    );
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
    assert!(result.item_result.success, "fixture encode failed: {:?}", result.item_result.error);
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

#[test]
fn activity_hub_emit_reaches_subscribers() {
    let activity = ActivityHub::new();
    let mut receiver = activity.subscribe();
    activity.emit("process", "worker started");
    let event = receiver.try_recv().expect("activity event");
    assert_eq!(event.category, "process");
    assert_eq!(event.message, "worker started");
}

#[test]
fn application_bootstrap_does_not_require_tokio_runtime() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    let application = Application::bootstrap(paths).expect("bootstrap");

    assert_eq!(application.queue.progress_worker_spawns(), 0);
    assert_eq!(application.queue.snapshot_worker_spawns(), 0);
}

#[tokio::test]
async fn parallel_item_failure_does_not_cancel_other_workers() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let items = parallel_items(
        &paths,
        fake("fake-ffmpeg-item-fail"),
        &["missing-output-item.mp4", "success-item.mp4"],
        vec![EncoderBackend::Cpu, EncoderBackend::Qsv],
    )
    .await;
    let supervisor = QueueSupervisor::new(ActivityHub::new());
    supervisor.enqueue(items).await.expect("enqueue");
    supervisor
        .start(ExecutionContext {
            paths: paths.clone(),
            tools: ToolPaths {
                ffmpeg: fake("fake-ffmpeg-item-fail"),
                ffprobe: fake("fake-ffprobe"),
            },
            activity: ActivityHub::new(),
        })
        .await
        .expect("start");
    let snapshot = wait_for_idle(&supervisor).await;
    assert_eq!(snapshot.metrics.failed_items, 1);
    assert_eq!(snapshot.metrics.done_items, 1);
    assert!(snapshot.state.items.iter().all(|item| item.status != QueueItemStatus::Running));
}

#[tokio::test]
async fn worker_failure_cannot_leave_idle_with_running_items() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let mut items = parallel_items(
        &paths,
        fake("fake-ffmpeg"),
        &["worker-error.mp4"],
        vec![EncoderBackend::Cpu],
    )
    .await;
    let blocked_parent = paths.root.join("blocked-parent");
    std::fs::write(&blocked_parent, b"not a directory").expect("blocked parent");
    items[0].output_path = blocked_parent.join("output.mp4");

    let supervisor = QueueSupervisor::new(ActivityHub::new());
    supervisor.enqueue(items).await.expect("enqueue");
    supervisor
        .start(ExecutionContext {
            paths: paths.clone(),
            tools: ToolPaths { ffmpeg: fake("fake-ffmpeg"), ffprobe: fake("fake-ffprobe") },
            activity: ActivityHub::new(),
        })
        .await
        .expect("start");
    let snapshot = wait_for_idle(&supervisor).await;
    assert_eq!(snapshot.metrics.failed_items, 1);
    assert!(snapshot.state.items.iter().all(|item| item.status != QueueItemStatus::Running));
    validate_queue_state(&snapshot.state).expect("valid state after worker failure");
}

#[tokio::test]
async fn direct_parallel_execution_keeps_successful_items_after_failure() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let items = parallel_items(
        &paths,
        fake("fake-ffmpeg-item-fail"),
        &["missing-output-item.mp4", "success-item.mp4"],
        vec![EncoderBackend::Cpu, EncoderBackend::Qsv],
    )
    .await;
    let results = execute_plan(
        &EncodePlan {
            items,
            ffmpeg_path: fake("fake-ffmpeg-item-fail"),
            ffprobe_path: fake("fake-ffprobe"),
            input_root: paths.root.clone(),
            output_root: paths.root.join("out"),
            workdir: paths.workdir.clone(),
        },
        &paths,
        &ActivityHub::new(),
        CancellationToken::new(),
        None,
    )
    .await
    .expect("parallel execution");
    assert_eq!(results.len(), 2);
    assert_eq!(results.iter().filter(|result| result.item_result.success).count(), 1);
    assert_eq!(results.iter().filter(|result| !result.item_result.success).count(), 1);
}

#[tokio::test]
async fn backend_worker_failure_does_not_cancel_other_backends() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let items = parallel_items(
        &paths,
        fake("fake-ffmpeg"),
        &["backend-one.mp4", "backend-two.mp4"],
        vec![EncoderBackend::Nvenc, EncoderBackend::Cpu],
    )
    .await;
    let supervisor = QueueSupervisor::new(ActivityHub::new());
    supervisor.enqueue(items).await.expect("enqueue");
    supervisor
        .start(ExecutionContext {
            paths: paths.clone(),
            tools: ToolPaths { ffmpeg: fake("fake-ffmpeg"), ffprobe: fake("fake-ffprobe") },
            activity: ActivityHub::new(),
        })
        .await
        .expect("start");
    let snapshot = wait_for_idle(&supervisor).await;
    assert_eq!(snapshot.metrics.failed_items, 1);
    assert_eq!(snapshot.metrics.done_items, 1);
}

#[tokio::test]
async fn queue_stop_cancels_all_active_parallel_workers() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let items = parallel_items(
        &paths,
        fake("fake-ffmpeg-item-fail"),
        &["hang-one.mp4", "hang-two.mp4"],
        vec![EncoderBackend::Cpu, EncoderBackend::Qsv],
    )
    .await;
    let supervisor = QueueSupervisor::new(ActivityHub::new());
    supervisor.enqueue(items).await.expect("enqueue");
    supervisor
        .start(ExecutionContext {
            paths: paths.clone(),
            tools: ToolPaths { ffmpeg: fake("fake-queue-hang"), ffprobe: fake("fake-ffprobe") },
            activity: ActivityHub::new(),
        })
        .await
        .expect("start");
    wait_for_running(&supervisor).await;
    supervisor.stop().await.expect("stop");
    let snapshot = wait_for_idle(&supervisor).await;
    assert!(snapshot.state.items.iter().all(|item| item.status != QueueItemStatus::Running));
    assert!(snapshot.metrics.cancelled_items >= 1);
}

#[tokio::test]
async fn mixed_serial_and_parallel_queue_is_rejected_before_start() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let mut items = parallel_items(
        &paths,
        fake("fake-ffmpeg"),
        &["serial.mp4", "parallel.mp4"],
        vec![EncoderBackend::Cpu],
    )
    .await;
    items[0].settings.parallel_enabled = false;
    items[0].settings.parallel_backends.clear();
    let supervisor = QueueSupervisor::new(ActivityHub::new());
    supervisor.enqueue(items).await.expect("enqueue");
    let error = supervisor
        .start(ExecutionContext {
            paths: paths.clone(),
            tools: ToolPaths { ffmpeg: fake("fake-ffmpeg"), ffprobe: fake("fake-ffprobe") },
            activity: ActivityHub::new(),
        })
        .await
        .expect_err("mixed queue must be rejected");
    assert!(error.to_string().contains("incompatible execution modes"));
    let snapshot = supervisor.snapshot().await;
    assert_eq!(snapshot.state.run_state, QueueRunState::Idle);
    assert_eq!(snapshot.state.active_run_id, None);
    assert!(snapshot.state.items.iter().all(|item| item.status == QueueItemStatus::Queued));
    validate_queue_state(&snapshot.state).expect("valid rejected queue state");
}

#[tokio::test]
async fn different_parallel_backend_sets_are_rejected_before_start() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let mut items = parallel_items(
        &paths,
        fake("fake-ffmpeg"),
        &["one.mp4", "two.mp4"],
        vec![EncoderBackend::Cpu, EncoderBackend::Qsv],
    )
    .await;
    items[1].settings.parallel_backends = vec![EncoderBackend::Cpu];
    let supervisor = QueueSupervisor::new(ActivityHub::new());
    supervisor.enqueue(items).await.expect("enqueue");
    let error = supervisor
        .start(ExecutionContext {
            paths: paths.clone(),
            tools: ToolPaths { ffmpeg: fake("fake-ffmpeg"), ffprobe: fake("fake-ffprobe") },
            activity: ActivityHub::new(),
        })
        .await
        .expect_err("different backend sets must be rejected");
    assert!(error.to_string().contains("incompatible execution modes"));
    assert_eq!(supervisor.snapshot().await.state.run_state, QueueRunState::Idle);
}

#[tokio::test]
async fn same_parallel_backends_in_different_duplicate_forms_are_normalized() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let mut items = parallel_items(
        &paths,
        fake("fake-ffmpeg"),
        &["one.mp4", "two.mp4"],
        vec![EncoderBackend::Cpu, EncoderBackend::Cpu],
    )
    .await;
    items[1].settings.parallel_backends = vec![EncoderBackend::Cpu];
    let supervisor = QueueSupervisor::new(ActivityHub::new());
    supervisor.enqueue(items).await.expect("enqueue");
    supervisor
        .start(ExecutionContext {
            paths: paths.clone(),
            tools: ToolPaths { ffmpeg: fake("fake-ffmpeg"), ffprobe: fake("fake-ffprobe") },
            activity: ActivityHub::new(),
        })
        .await
        .expect("normalized parallel queue starts");
    let snapshot = wait_for_idle(&supervisor).await;
    assert_eq!(snapshot.metrics.done_items, 2);
    validate_queue_state(&snapshot.state).expect("valid completed queue");
}

#[tokio::test]
async fn all_serial_items_start() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let mut items = parallel_items(
        &paths,
        fake("fake-ffmpeg"),
        &["one.mp4", "two.mp4"],
        vec![EncoderBackend::Cpu],
    )
    .await;
    for item in &mut items {
        item.settings.parallel_enabled = false;
        item.settings.parallel_backends.clear();
    }
    let supervisor = QueueSupervisor::new(ActivityHub::new());
    supervisor.enqueue(items).await.expect("enqueue");
    supervisor
        .start(ExecutionContext {
            paths: paths.clone(),
            tools: ToolPaths { ffmpeg: fake("fake-ffmpeg"), ffprobe: fake("fake-ffprobe") },
            activity: ActivityHub::new(),
        })
        .await
        .expect("serial queue starts");
    let snapshot = wait_for_idle(&supervisor).await;
    assert_eq!(snapshot.metrics.done_items, 2);
}

#[tokio::test]
async fn old_run_cleanup_does_not_clear_new_run_cancel_token() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let mut items =
        parallel_items(&paths, fake("fake-ffmpeg"), &["run-again.mp4"], vec![EncoderBackend::Cpu])
            .await;
    items[0].settings.parallel_enabled = false;
    items[0].settings.parallel_backends.clear();
    let supervisor = QueueSupervisor::new(ActivityHub::new());
    supervisor.enqueue(items).await.expect("enqueue");
    let context = || ExecutionContext {
        paths: paths.clone(),
        tools: ToolPaths { ffmpeg: fake("fake-queue-hang"), ffprobe: fake("fake-ffprobe") },
        activity: ActivityHub::new(),
    };

    supervisor.start(context()).await.expect("first start");
    wait_for_running(&supervisor).await;
    supervisor.stop().await.expect("first stop");
    let first = wait_for_idle(&supervisor).await;
    let item_id = first.state.items[0].item_id.clone();
    assert_eq!(first.state.items[0].status, QueueItemStatus::Cancelled);

    supervisor.retry(vec![item_id]).await.expect("retry");
    supervisor.start(context()).await.expect("second start");
    wait_for_running(&supervisor).await;
    supervisor.stop().await.expect("second stop");
    let second = wait_for_idle(&supervisor).await;
    assert_eq!(second.metrics.cancelled_items, 1);
    assert!(second.state.items.iter().all(|item| item.status != QueueItemStatus::Running));
    validate_queue_state(&second.state).expect("valid state after consecutive runs");
}
