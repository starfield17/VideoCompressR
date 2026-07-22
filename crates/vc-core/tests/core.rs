use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use vc_core::model::{
    CapabilitySnapshot, Codec, CompressionRatio, EncodeSettings, EncoderBackend, EncoderCapability,
    MediaInfo,
};
use vc_core::planning::{PlanningInput, choose_ratio, compute_target_video_bitrate, plan_item};
use vc_core::queue::{
    ItemProgress, QueueCommand, QueueItemStatus, QueueRunState, QueueState, apply, compute_metrics,
};

fn media(path: &Path) -> MediaInfo {
    MediaInfo {
        path: path.to_path_buf(),
        source_size_bytes: Some(7_500_000),
        duration: 12.0,
        format_bitrate_bps: 5_000_000,
        video_bitrate_bps: 4_000_000,
        audio_bitrate_bps: 128_000,
        width: Some(1920),
        height: Some(1080),
        fps: Some(30.0),
        video_codec: "h264".into(),
        audio_codec: Some("aac".into()),
    }
}

fn planned(source: &str, output: &str) -> vc_core::EncodePlanItem {
    plan_item(PlanningInput {
        source: media(Path::new(source)),
        output_path: PathBuf::from(output),
        settings: EncodeSettings { overwrite: true, ..EncodeSettings::default() },
        capabilities: capabilities(&[(EncoderBackend::Cpu, "libx265")]),
        output_exists: false,
    })
    .expect("plan")
}

fn capabilities(entries: &[(EncoderBackend, &str)]) -> CapabilitySnapshot {
    let mut codecs = BTreeMap::new();
    codecs.insert(
        "hevc".into(),
        entries
            .iter()
            .map(|(backend, encoder)| EncoderCapability {
                backend: *backend,
                encoder: (*encoder).into(),
                supports_two_pass: *encoder == "libx265",
                default_preset: Some(if *encoder == "libx265" { "slow" } else { "p6" }.into()),
                presets: vec!["slow".into(), "p6".into()],
            })
            .collect(),
    );
    codecs.insert("av1".into(), Vec::new());
    CapabilitySnapshot { codecs, ..CapabilitySnapshot::default() }
}

#[test]
fn bitrate_policy_matches_legacy_table() {
    let cases = [
        (4_000_000, 0.76, 250, 0, 3_040_000),
        (100_000, 0.64, 250, 0, 250_000),
        (4_000_000, 0.76, 250, 2_000, 2_000_000),
        (1, 0.01, 0, 0, 50_000),
    ];
    for (source, ratio, min, max, expected) in cases {
        assert_eq!(compute_target_video_bitrate(source, ratio, min, max).get(), expected);
    }
    assert_eq!(choose_ratio(Codec::Hevc, None).expect("default"), 0.76);
    assert_eq!(choose_ratio(Codec::Av1, None).expect("default"), 0.64);
    assert!(CompressionRatio::new(0.0).is_err());
}

#[test]
fn bitrate_golden_fixture_is_root_owned() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/golden/bitrate.json");
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(path).expect("golden fixture"))
            .expect("golden JSON");
    for row in rows {
        let target = compute_target_video_bitrate(
            row["source"].as_u64().expect("source"),
            row["ratio"].as_f64().expect("ratio"),
            row["min_kbps"].as_u64().expect("min"),
            row["max_kbps"].as_u64().expect("max"),
        );
        assert_eq!(target.get(), row["target"].as_u64().expect("target"));
    }
}

#[test]
fn plan_golden_fixture_matches_cpu_hevc_selection() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden/plan/cpu-hevc.json");
    let row: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("golden plan")).expect("json");
    let item = planned("sample.mp4", "sample_hevc.mp4");
    assert_eq!(
        item.encoder.as_ref().expect("encoder").encoder_name,
        row["encoder"].as_str().expect("encoder")
    );
    assert_eq!(
        item.target_video_bitrate_bps,
        row["target_video_bitrate_bps"].as_u64().expect("target")
    );
    assert_eq!(item.output_path.extension().and_then(|value| value.to_str()), Some("mp4"));
}

#[test]
fn auto_backend_prefers_hardware_and_planner_injects_default_preset() {
    let source = PathBuf::from("input.mp4");
    let settings = EncodeSettings { overwrite: true, ..EncodeSettings::default() };
    let item = plan_item(PlanningInput {
        source: media(&source),
        output_path: PathBuf::from("output.mp4"),
        settings,
        capabilities: capabilities(&[
            (EncoderBackend::Qsv, "hevc_qsv"),
            (EncoderBackend::Cpu, "libx265"),
        ]),
        output_exists: false,
    })
    .expect("plan");
    let encoder = item.encoder.expect("encoder");
    assert_eq!(encoder.backend, EncoderBackend::Qsv);
    assert_eq!(item.settings.encoder_preset, Some("p6".into()));
    assert_eq!(item.target_video_bitrate_bps, 3_040_000);
}

#[test]
fn planner_rejects_two_pass_for_hardware_and_existing_output_without_overwrite() {
    let source = PathBuf::from("input.mp4");
    let mut settings = EncodeSettings { two_pass: true, ..EncodeSettings::default() };
    assert!(
        plan_item(PlanningInput {
            source: media(&source),
            output_path: PathBuf::from("output.mp4"),
            settings: settings.clone(),
            capabilities: capabilities(&[(EncoderBackend::Qsv, "hevc_qsv")]),
            output_exists: false,
        })
        .is_err()
    );

    settings.two_pass = false;
    assert!(
        plan_item(PlanningInput {
            source: media(&source),
            output_path: PathBuf::from("output.mp4"),
            settings,
            capabilities: capabilities(&[(EncoderBackend::Cpu, "libx265")]),
            output_exists: true,
        })
        .is_err()
    );
}

#[test]
fn queue_reducer_rejects_stale_progress_and_computes_metrics() {
    let source = PathBuf::from("input.mp4");
    let settings = EncodeSettings { overwrite: true, ..EncodeSettings::default() };
    let plan = plan_item(PlanningInput {
        source: media(&source),
        output_path: PathBuf::from("output.mp4"),
        settings,
        capabilities: capabilities(&[(EncoderBackend::Cpu, "libx265")]),
        output_exists: false,
    })
    .expect("plan");
    let mut state = QueueState::default();
    apply(&mut state, QueueCommand::Enqueue(vec![plan])).expect("enqueue");
    assert_eq!(state.items[0].status, QueueItemStatus::Queued);
    apply(&mut state, QueueCommand::StartRun { run_id: "run-1".into() }).expect("start");
    let item_id = state.items[0].item_id.clone();
    apply(&mut state, QueueCommand::StartItem { item_id: item_id.clone(), run_id: "run-1".into() })
        .expect("start item");
    assert!(
        apply(
            &mut state,
            QueueCommand::ReportProgress {
                item_id: item_id.clone(),
                run_id: "old-run".into(),
                progress: ItemProgress { percent: 100.0, ..ItemProgress::default() }
            }
        )
        .is_err()
    );
    apply(
        &mut state,
        QueueCommand::ReportProgress {
            item_id,
            run_id: "run-1".into(),
            progress: ItemProgress {
                percent: 50.0,
                speed: Some("2x".into()),
                ..ItemProgress::default()
            },
        },
    )
    .expect("progress");
    let metrics = compute_metrics(&state);
    assert_eq!(state.run_state, QueueRunState::Running);
    assert_eq!(metrics.running_items, 1);
    assert_eq!(metrics.current_file_percent, Some(50.0));
    assert!(metrics.queue_percent > 0.0 && metrics.queue_percent < 100.0);
}

#[test]
fn queue_pause_after_current_preserves_unstarted_items() {
    let mut state = QueueState::default();
    apply(
        &mut state,
        QueueCommand::Enqueue(vec![
            planned("one.mp4", "one_hevc.mp4"),
            planned("two.mp4", "two_hevc.mp4"),
        ]),
    )
    .expect("enqueue");
    apply(&mut state, QueueCommand::StartRun { run_id: "run-1".into() }).expect("start");
    let first = state.items[0].item_id.clone();
    apply(&mut state, QueueCommand::StartItem { item_id: first.clone(), run_id: "run-1".into() })
        .expect("start item");
    apply(&mut state, QueueCommand::PauseAfterCurrent).expect("pause request");
    apply(
        &mut state,
        QueueCommand::Finish {
            item_id: first,
            run_id: "run-1".into(),
            result: vc_core::queue::ItemResult {
                success: true,
                skipped: false,
                return_code: Some(0),
                output_path: None,
                log_path: None,
                error: None,
            },
        },
    )
    .expect("finish");
    apply(&mut state, QueueCommand::PauseComplete { run_id: "run-1".into() })
        .expect("pause complete");
    assert_eq!(state.run_state, QueueRunState::Paused);
    assert_eq!(state.items[0].status, QueueItemStatus::Done);
    assert_eq!(state.items[1].status, QueueItemStatus::Queued);
}

#[test]
fn queue_reorder_moves_only_draft_and_queued_items() {
    let mut state = QueueState::default();
    apply(
        &mut state,
        QueueCommand::Enqueue(vec![
            planned("one.mp4", "one_hevc.mp4"),
            planned("two.mp4", "two_hevc.mp4"),
            planned("three.mp4", "three_hevc.mp4"),
        ]),
    )
    .expect("enqueue");
    let ids = state.items.iter().map(|item| item.item_id.clone()).collect::<Vec<_>>();
    let mut reordered = ids.clone();
    reordered.swap(0, 2);
    apply(&mut state, QueueCommand::Reorder { ordered_ids: reordered.clone() }).expect("reorder");
    assert_eq!(state.items.iter().map(|item| item.item_id.clone()).collect::<Vec<_>>(), reordered);

    apply(&mut state, QueueCommand::StartRun { run_id: "run-2".into() }).expect("start");
    let done = state.items[0].item_id.clone();
    apply(&mut state, QueueCommand::StartItem { item_id: done.clone(), run_id: "run-2".into() })
        .expect("start item");
    apply(
        &mut state,
        QueueCommand::Finish {
            item_id: done.clone(),
            run_id: "run-2".into(),
            result: vc_core::queue::ItemResult {
                success: true,
                skipped: false,
                return_code: Some(0),
                output_path: None,
                log_path: None,
                error: None,
            },
        },
    )
    .expect("finish");
    apply(&mut state, QueueCommand::RunIdle { run_id: "run-2".into() }).expect("idle");
    let second_done = state.items[1].item_id.clone();
    apply(&mut state, QueueCommand::StartRun { run_id: "run-3".into() }).expect("start second run");
    apply(
        &mut state,
        QueueCommand::StartItem { item_id: second_done.clone(), run_id: "run-3".into() },
    )
    .expect("start second item");
    apply(
        &mut state,
        QueueCommand::Finish {
            item_id: second_done,
            run_id: "run-3".into(),
            result: vc_core::queue::ItemResult {
                success: true,
                skipped: false,
                return_code: Some(0),
                output_path: None,
                log_path: None,
                error: None,
            },
        },
    )
    .expect("finish second item");
    apply(&mut state, QueueCommand::RunIdle { run_id: "run-3".into() }).expect("idle second run");
    let mut invalid = state.items.iter().map(|item| item.item_id.clone()).collect::<Vec<_>>();
    invalid.swap(0, 1);
    assert!(apply(&mut state, QueueCommand::Reorder { ordered_ids: invalid }).is_err());
}
