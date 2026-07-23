//! Performance / responsiveness stress tests (no wall-clock FPS claims).

use std::path::PathBuf;
use std::time::Duration;

use vc_core::queue::QueueCommand;
use vc_core::{EncodePlanItem, EncodeSettings, EncoderBackend};
use vc_runtime::ActivityHub;
use vc_runtime::AppPaths;
use vc_runtime::activity::{MAX_ACTIVITY_HISTORY, MAX_ACTIVITY_HISTORY_REQUEST};
use vc_runtime::execution::ProgressEvent;
use vc_runtime::process_log::{LogOpenCounter, ProcessLogWriter};
use vc_runtime::queue::supervisor::{
    QueueSupervisor, SNAPSHOT_COALESCE_INTERVAL, WaitForIdleError,
};
use vc_runtime::storage::window_state::{
    GEOMETRY_SAVE_DEBOUNCE, WindowGeometry, WindowGeometryRuntime, WindowStateStore,
};

fn plan_item(name: &str) -> EncodePlanItem {
    EncodePlanItem {
        source_path: PathBuf::from(format!("/videos/{name}.mp4")),
        output_path: PathBuf::from(format!("/out/{name}.mp4")),
        media_info: None,
        encoder: None,
        settings: EncodeSettings { backend: EncoderBackend::Cpu, ..EncodeSettings::default() },
        target_video_bitrate_bps: 1_000_000,
        warnings: vec![],
        skip_reason: None,
    }
}

#[test]
fn activity_emit_100k_stays_bounded() {
    let activity = ActivityHub::new();
    for index in 0..100_000 {
        activity.emit("process", format!("event-{index}"));
    }
    assert!(activity.retained_len() <= MAX_ACTIVITY_HISTORY);
    assert_eq!(activity.history_tail(500).len(), 500);
    let clamped = activity.history_tail(MAX_ACTIVITY_HISTORY_REQUEST + 10_000);
    assert!(clamped.len() <= MAX_ACTIVITY_HISTORY_REQUEST);
}

#[tokio::test(start_paused = true)]
async fn large_queue_progress_snapshot_rate_is_bounded() {
    let supervisor = QueueSupervisor::new(ActivityHub::new());
    supervisor.initialize().await.expect("initialize");
    let items: Vec<_> = (0..1_000).map(|index| plan_item(&format!("item-{index}"))).collect();
    supervisor.enqueue(items).await.expect("enqueue");
    let run_id = "stress-run".to_owned();
    supervisor
        .apply_command(QueueCommand::StartRun { run_id: run_id.clone() })
        .await
        .expect("start");
    // Start first 4 items as "running" workers.
    let ids: Vec<String> = supervisor
        .snapshot_now()
        .state
        .items
        .iter()
        .take(4)
        .map(|item| item.item_id.clone())
        .collect();
    for item_id in &ids {
        supervisor
            .apply_command(QueueCommand::StartItem {
                item_id: item_id.clone(),
                run_id: run_id.clone(),
            })
            .await
            .expect("start item");
    }
    let before = supervisor.snapshot_publish_count();
    let metrics_before = supervisor.metrics_compute_count();
    for tick in 0..2_500 {
        let item_id = &ids[tick % ids.len()];
        supervisor.report_progress(
            ProgressEvent {
                item_id: Some(item_id.clone()),
                stage: "encode".into(),
                state: "running".into(),
                percent: Some((tick % 100) as f64),
                speed: Some("2.0x".into()),
                elapsed_sec: Some(tick as f64 * 0.1),
                current_pass: 1,
                total_passes: 1,
                message: None,
            },
            item_id,
            &run_id,
        );
        if tick % 50 == 0 {
            tokio::time::advance(SNAPSHOT_COALESCE_INTERVAL).await;
            for _ in 0..5 {
                tokio::task::yield_now().await;
            }
        }
    }
    tokio::time::advance(SNAPSHOT_COALESCE_INTERVAL * 2).await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    let published = supervisor.snapshot_publish_count() - before;
    let metrics = supervisor.metrics_compute_count() - metrics_before;
    // 2500 progress events must not produce thousands of publishes/metrics passes.
    assert!(published < 200, "snapshot publishes too high: {published}");
    assert!(metrics < 200, "metrics computes too high: {metrics}");
    assert_eq!(supervisor.progress_worker_spawns(), 1);
}

#[tokio::test(start_paused = true)]
async fn geometry_storm_writes_few_times() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = AppPaths::from_root(temp.path());
    paths.ensure().expect("layout");
    let runtime = WindowGeometryRuntime::load(WindowStateStore::new(paths));
    runtime.start().expect("start worker");
    for index in 0..10_000 {
        runtime.note_geometry(
            "main",
            WindowGeometry {
                width: 800 + (index % 40) as u32,
                height: 600,
                x: Some(index % 100),
                y: Some(20),
                maximized: false,
            },
        );
    }
    tokio::time::advance(GEOMETRY_SAVE_DEBOUNCE + Duration::from_millis(100)).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    runtime.shutdown().await;
    assert!(runtime.save_count() <= 2, "writes={}", runtime.save_count());
    assert!(runtime.save_count() >= 1);
    assert_eq!(runtime.worker_spawn_count(), 1);
}

#[tokio::test]
async fn log_flood_opens_once() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("flood.log");
    let counter = LogOpenCounter::new();
    let writer =
        ProcessLogWriter::open_with_counter(path, Some(counter.clone())).await.expect("open");
    for index in 0..50_000 {
        let _ = writer.try_write_line(format!("frame={index}"));
        if index % 5_000 == 0 {
            tokio::task::yield_now().await;
        }
    }
    writer.write_line("diagnostic end").await.expect("diag");
    writer.finish().await.expect("finish");
    assert_eq!(counter.opens(), 1);
}

#[tokio::test(start_paused = true)]
async fn close_wait_times_out() {
    let supervisor = QueueSupervisor::new(ActivityHub::new());
    supervisor.enqueue(vec![plan_item("stuck")]).await.unwrap();
    supervisor.apply_command(QueueCommand::StartRun { run_id: "stuck".into() }).await.unwrap();
    let wait = {
        let supervisor = supervisor.clone();
        tokio::spawn(async move { supervisor.wait_until_idle(Duration::from_secs(1)).await })
    };
    tokio::time::advance(Duration::from_secs(2)).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(wait.await.unwrap(), Err(WaitForIdleError::TimedOut));
    supervisor.force_abort_active_run("test").await.unwrap();
    assert_eq!(supervisor.snapshot_now().state.run_state, vc_core::queue::QueueRunState::Idle);
}
