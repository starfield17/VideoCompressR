use super::activity::ActivityHub;
use super::planning::EncodePlan;
use crate::error::RuntimeError;
use crate::ffmpeg::ToolPaths;
use crate::ffmpeg::capabilities::ensure_capabilities;
use crate::ffmpeg::command::{
    cleanup_passlog, passlog_path, render_encode_commands, render_preview_extract,
};
use crate::ffmpeg::process::{OutputStream, ProcessLine, run_streaming};
use crate::ffmpeg::progress::{ProgressParser, progress_percent};
use crate::platform::paths::AppPaths;
use crate::process_log::ProcessLogWriter;
use crate::subtitles::copy_external_subtitles;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use vc_core::planning::{resolve_encoder, unique_parallel_backends};
use vc_core::queue::ItemResult;
use vc_core::{EncodePlanItem, EncoderBackend};

/// Minimum interval between progress summary lines written to the process log.
const PROGRESS_LOG_SUMMARY_INTERVAL_SECS: f64 = 3.0;

#[derive(Clone, Debug)]
pub struct ProgressEvent {
    pub item_id: Option<String>,
    pub stage: String,
    pub state: String,
    pub percent: Option<f64>,
    pub speed: Option<String>,
    pub elapsed_sec: Option<f64>,
    pub current_pass: u32,
    pub total_passes: u32,
    pub message: Option<String>,
}

pub type ProgressSink = Arc<dyn Fn(ProgressEvent) + Send + Sync>;

#[derive(Clone, Debug)]
pub struct ExecutionResult {
    pub item_result: ItemResult,
    pub source_path: PathBuf,
    pub output_path: PathBuf,
    pub commands: Vec<Vec<String>>,
    pub copied_external_subtitle_paths: Vec<PathBuf>,
    pub external_subtitle_warnings: Vec<String>,
}

fn failed_execution_result(
    item: &EncodePlanItem,
    paths: &AppPaths,
    error: &RuntimeError,
) -> ExecutionResult {
    ExecutionResult {
        item_result: ItemResult {
            success: false,
            skipped: false,
            return_code: None,
            output_path: Some(item.output_path.clone()),
            log_path: Some(paths.logs_dir.join(format!("{}-encode.log", token(&item.source_path)))),
            error: Some(error.to_string()),
        },
        source_path: item.source_path.clone(),
        output_path: item.output_path.clone(),
        commands: Vec::new(),
        copied_external_subtitle_paths: Vec::new(),
        external_subtitle_warnings: Vec::new(),
    }
}

pub async fn execute_plan(
    plan: &EncodePlan,
    paths: &AppPaths,
    activity: &ActivityHub,
    cancel: CancellationToken,
    sink: Option<ProgressSink>,
) -> Result<Vec<ExecutionResult>, RuntimeError> {
    if plan.items.iter().any(|item| item.settings.parallel_enabled) {
        return execute_plan_parallel(plan, paths, activity, cancel, sink).await;
    }
    let mut results = Vec::with_capacity(plan.items.len());
    for (index, item) in plan.items.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        results.push(
            execute_item(
                item,
                &plan.ffmpeg_path,
                paths,
                activity,
                cancel.clone(),
                sink.clone(),
                index + 1,
                plan.items.len(),
                None,
            )
            .await?,
        );
    }
    Ok(results)
}

async fn execute_plan_parallel(
    plan: &EncodePlan,
    paths: &AppPaths,
    activity: &ActivityHub,
    cancel: CancellationToken,
    sink: Option<ProgressSink>,
) -> Result<Vec<ExecutionResult>, RuntimeError> {
    let configured_backends = plan
        .items
        .iter()
        .flat_map(|item| item.settings.parallel_backends.iter().copied())
        .collect::<Vec<_>>();
    let backends = unique_parallel_backends(&configured_backends)
        .into_iter()
        .filter(|backend| *backend != EncoderBackend::Auto)
        .collect::<Vec<_>>();
    if backends.is_empty() {
        return Err(RuntimeError::Planning(
            "Parallel mode requires at least one explicit backend.".into(),
        ));
    }
    if plan
        .items
        .iter()
        .any(|item| item.settings.two_pass || item.settings.encoder_preset.is_some())
    {
        return Err(RuntimeError::Planning(
            "Parallel mode does not support two-pass encoding or a manual encoder preset.".into(),
        ));
    }
    let capabilities = ensure_capabilities(
        paths,
        &ToolPaths { ffmpeg: plan.ffmpeg_path.clone(), ffprobe: plan.ffprobe_path.clone() },
        false,
    )
    .await?;
    let pending = Arc::new(Mutex::new((0..plan.items.len()).collect::<VecDeque<_>>()));
    let results = Arc::new(Mutex::new(vec![None; plan.items.len()]));
    let mut workers = Vec::with_capacity(backends.len());
    for backend in backends {
        let pending = pending.clone();
        let results = results.clone();
        let capabilities = capabilities.clone();
        let queue_cancel = cancel.clone();
        let plan = plan.clone();
        let paths = paths.clone();
        let activity = activity.clone();
        let sink = sink.clone();
        workers.push(tokio::spawn(async move {
            loop {
                if queue_cancel.is_cancelled() {
                    return Ok::<(), RuntimeError>(());
                }
                let index = { pending.lock().await.pop_front() };
                let Some(index) = index else {
                    return Ok(());
                };
                let base = plan.items[index].clone();
                let mut item = base;
                if item.skip_reason.is_none() {
                    let selection =
                        match resolve_encoder(item.settings.codec, backend, &capabilities) {
                            Ok(value) => value,
                            Err(error) => {
                                results.lock().await[index] = Some(failed_execution_result(
                                    &item,
                                    &paths,
                                    &RuntimeError::Planning(error),
                                ));
                                return Ok(());
                            }
                        };
                    item.encoder = Some(selection.clone());
                    item.settings.backend = backend;
                    item.settings.parallel_enabled = false;
                    item.settings.encoder_preset = selection.default_preset.clone();
                }
                let item_cancel = queue_cancel.child_token();
                let result = match execute_item(
                    &item,
                    &plan.ffmpeg_path,
                    &paths,
                    &activity,
                    item_cancel,
                    sink.clone(),
                    index + 1,
                    plan.items.len(),
                    None,
                )
                .await
                {
                    Ok(value) => value,
                    Err(RuntimeError::Cancelled) if queue_cancel.is_cancelled() => {
                        return Err(RuntimeError::Cancelled);
                    }
                    Err(error) => failed_execution_result(&item, &paths, &error),
                };
                results.lock().await[index] = Some(result);
            }
        }));
    }
    let mut first_error = None;
    for worker in workers {
        match worker.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
                cancel.cancel();
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(RuntimeError::Encode(error.to_string()));
                }
                cancel.cancel();
            }
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    if cancel.is_cancelled() {
        return Err(RuntimeError::Cancelled);
    }
    let values = results.lock().await.iter_mut().map(Option::take).collect::<Vec<_>>();
    Ok(values.into_iter().flatten().collect())
}

#[allow(clippy::too_many_arguments)]
pub async fn execute_item(
    item: &EncodePlanItem,
    ffmpeg: &Path,
    paths: &AppPaths,
    activity: &ActivityHub,
    cancel: CancellationToken,
    sink: Option<ProgressSink>,
    index: usize,
    total: usize,
    item_id: Option<String>,
) -> Result<ExecutionResult, RuntimeError> {
    let log_path = paths.logs_dir.join(format!("{}-encode.log", token(&item.source_path)));
    let log_writer = ProcessLogWriter::open(log_path.clone()).await?;
    log_writer
        .write_line(format!("[{} / {}] encoding {}", index, total, item.source_path.display()))
        .await?;
    if let Some(reason) = &item.skip_reason {
        let _ = log_writer.finish().await;
        return Ok(ExecutionResult {
            item_result: ItemResult {
                success: false,
                skipped: true,
                return_code: None,
                output_path: Some(item.output_path.clone()),
                log_path: Some(log_path),
                error: Some(reason.0.clone()),
            },
            source_path: item.source_path.clone(),
            output_path: item.output_path.clone(),
            commands: Vec::new(),
            copied_external_subtitle_paths: Vec::new(),
            external_subtitle_warnings: Vec::new(),
        });
    }
    let requests = render_encode_commands(ffmpeg, item, paths, None, None, "encode")?;
    let passlog = passlog_path(paths, item, "encode");
    let output_existed_before = item.output_path.exists();
    let commands = requests
        .iter()
        .map(|request| {
            let mut values = Vec::with_capacity(request.args.len() + 1);
            values.push(request.program.to_string_lossy().into_owned());
            values.extend(request.args.iter().map(|value| value.to_string_lossy().into_owned()));
            values
        })
        .collect::<Vec<_>>();
    if let Some(parent) = item.output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let output_backup = prepare_output_backup(paths, item)?;
    let total_passes = requests.len() as u32;
    for (pass, request) in requests.iter().enumerate() {
        let mut parser = ProgressParser::default();
        let duration = item.media_info.as_ref().map(|value| value.duration);
        let activity_for_line = activity.clone();
        let sink_for_line = sink.clone();
        let item_id_for_line = item_id.clone();
        let log_tx = log_writer.sender();
        let mut last_summary = Instant::now()
            .checked_sub(std::time::Duration::from_secs_f64(PROGRESS_LOG_SUMMARY_INTERVAL_SECS))
            .unwrap_or_else(Instant::now);
        let output =
            match run_streaming(request.clone(), cancel.clone(), move |line: ProcessLine| {
                match line.stream {
                    OutputStream::Stderr => {
                        // Diagnostic: file log + activity (not machine progress).
                        let _ = log_tx.try_send(&line.text);
                        activity_for_line.emit("process", line.text);
                    }
                    OutputStream::Stdout => {
                        // Machine progress: parse only; do not flood activity or raw log.
                        for update in parser.push(&(line.text + "\n")) {
                            let percent = progress_percent(&update, duration);
                            let speed = update.values.get("speed").cloned();
                            let elapsed = update
                                .values
                                .get("out_time_us")
                                .and_then(|value| value.parse::<f64>().ok())
                                .map(|value| value / 1_000_000.0);
                            let overall = percent.map(|value| {
                                ((pass as f64 + value / 100.0) / total_passes as f64) * 100.0
                            });
                            if last_summary.elapsed().as_secs_f64()
                                >= PROGRESS_LOG_SUMMARY_INTERVAL_SECS
                                || update.is_end
                            {
                                last_summary = Instant::now();
                                let summary = format!(
                                    "progress pass={}/{} percent={:.1} speed={}",
                                    pass as u32 + 1,
                                    total_passes,
                                    overall.unwrap_or(0.0),
                                    speed.as_deref().unwrap_or("-")
                                );
                                let _ = log_tx.try_send(summary);
                            }
                            if let Some(callback) = &sink_for_line {
                                callback(ProgressEvent {
                                    item_id: item_id_for_line.clone(),
                                    stage: "encode".into(),
                                    state: if update.is_end {
                                        "finished_file".into()
                                    } else {
                                        "running".into()
                                    },
                                    percent: overall,
                                    speed,
                                    elapsed_sec: elapsed,
                                    current_pass: pass as u32 + 1,
                                    total_passes,
                                    message: None,
                                });
                            }
                        }
                    }
                }
            })
            .await
            {
                Ok(value) => value,
                Err(error) => {
                    let _ = log_writer.finish().await;
                    cleanup_passlog(&passlog);
                    restore_output(
                        &item.output_path,
                        output_backup.as_deref(),
                        output_existed_before,
                    );
                    return Err(error);
                }
            };
        if output.cancelled {
            let _ = log_writer.finish().await;
            cleanup_passlog(&passlog);
            restore_output(&item.output_path, output_backup.as_deref(), output_existed_before);
            return Err(RuntimeError::Cancelled);
        }
        if output.code != 0 {
            let message = format!("FFmpeg exited with code {}", output.code);
            let _ = log_writer.write_line(&message).await;
            let _ = log_writer.finish().await;
            cleanup_passlog(&passlog);
            restore_output(&item.output_path, output_backup.as_deref(), output_existed_before);
            activity.emit("error", &message);
            return Ok(ExecutionResult {
                item_result: ItemResult {
                    success: false,
                    skipped: false,
                    return_code: Some(output.code),
                    output_path: Some(item.output_path.clone()),
                    log_path: Some(log_path.clone()),
                    error: Some(message),
                },
                source_path: item.source_path.clone(),
                output_path: item.output_path.clone(),
                commands,
                copied_external_subtitle_paths: Vec::new(),
                external_subtitle_warnings: Vec::new(),
            });
        }
    }
    let _ = log_writer.write_line("encode completed").await;
    let _ = log_writer.finish().await;
    cleanup_passlog(&passlog);
    if !item.output_path.is_file() {
        restore_output(&item.output_path, output_backup.as_deref(), output_existed_before);
        return Err(RuntimeError::Encode(format!(
            "FFmpeg completed without creating output: {}",
            item.output_path.display()
        )));
    }
    let (copied, warnings) = if item.settings.copy_external_subtitles {
        match copy_external_subtitles(&item.source_path, &item.output_path, item.settings.overwrite)
        {
            Ok(value) => value,
            Err(error) => {
                restore_output(&item.output_path, output_backup.as_deref(), output_existed_before);
                return Err(error);
            }
        }
    } else {
        (Vec::new(), Vec::new())
    };
    discard_output_backup(output_backup.as_deref());
    for warning in &warnings {
        activity.emit("error", warning);
    }
    Ok(ExecutionResult {
        item_result: ItemResult {
            success: true,
            skipped: false,
            return_code: Some(0),
            output_path: Some(item.output_path.clone()),
            log_path: Some(log_path),
            error: None,
        },
        source_path: item.source_path.clone(),
        output_path: item.output_path.clone(),
        commands,
        copied_external_subtitle_paths: copied,
        external_subtitle_warnings: warnings,
    })
}

pub async fn execute_preview(
    job: &vc_core::PreviewJob,
    ffmpeg: &Path,
    paths: &AppPaths,
    activity: &ActivityHub,
    cancel: CancellationToken,
    sink: Option<ProgressSink>,
) -> Result<vc_core::PreviewResult, RuntimeError> {
    let log_path = paths.logs_dir.join(format!("{}-preview.log", token(&job.source_path)));
    let log_writer = ProcessLogWriter::open(log_path.clone()).await?;
    log_writer.write_line("preview start").await?;
    let extract = render_preview_extract(ffmpeg, job);
    let extract_activity = activity.clone();
    let extract_log = log_writer.sender();
    let extract_output = match run_streaming(extract, cancel.clone(), move |line| {
        if line.stream == OutputStream::Stderr {
            let _ = extract_log.try_send(&line.text);
            extract_activity.emit("process", line.text);
        }
    })
    .await
    {
        Ok(value) => value,
        Err(error) => {
            let _ = log_writer.finish().await;
            cleanup_preview_files(job, paths);
            return Err(error);
        }
    };
    if extract_output.cancelled {
        let _ = log_writer.finish().await;
        cleanup_preview_files(job, paths);
        return Err(RuntimeError::Cancelled);
    }
    if extract_output.code != 0 {
        let _ = log_writer.finish().await;
        cleanup_preview_files(job, paths);
        return Err(RuntimeError::Encode(format!(
            "Preview sample extraction failed with code {}",
            extract_output.code
        )));
    }
    let requests = match render_encode_commands(
        ffmpeg,
        &job.plan_item,
        paths,
        Some(&job.source_sample_path),
        Some(&job.encoded_sample_path),
        "preview",
    ) {
        Ok(value) => value,
        Err(error) => {
            let _ = log_writer.finish().await;
            cleanup_preview_files(job, paths);
            return Err(error);
        }
    };
    let total_passes = requests.len().max(1) as u32;
    for (pass, request) in requests.into_iter().enumerate() {
        let mut preview_parser = ProgressParser::default();
        let activity_for_line = activity.clone();
        let sink_for_line = sink.clone();
        let duration = job.window.duration_sec;
        let log_tx = log_writer.sender();
        let output = match run_streaming(request, cancel.clone(), move |line| match line.stream {
            OutputStream::Stderr => {
                let _ = log_tx.try_send(&line.text);
                activity_for_line.emit("process", line.text);
            }
            OutputStream::Stdout => {
                for update in preview_parser.push(&(line.text + "\n")) {
                    if let Some(callback) = &sink_for_line {
                        callback(ProgressEvent {
                            item_id: None,
                            stage: "preview".into(),
                            state: if update.is_end {
                                "finished_file".into()
                            } else {
                                "running".into()
                            },
                            percent: progress_percent(&update, Some(duration)),
                            speed: update.values.get("speed").cloned(),
                            elapsed_sec: update
                                .values
                                .get("out_time_us")
                                .and_then(|value| value.parse::<f64>().ok())
                                .map(|value| value / 1_000_000.0),
                            current_pass: pass as u32 + 1,
                            total_passes,
                            message: None,
                        });
                    }
                }
            }
        })
        .await
        {
            Ok(value) => value,
            Err(error) => {
                let _ = log_writer.finish().await;
                cleanup_preview_files(job, paths);
                return Err(error);
            }
        };
        if output.cancelled {
            let _ = log_writer.finish().await;
            cleanup_preview_files(job, paths);
            return Err(RuntimeError::Cancelled);
        }
        if output.code != 0 {
            let _ = log_writer.finish().await;
            cleanup_preview_files(job, paths);
            return Err(RuntimeError::Encode(format!(
                "Preview encoding failed with code {}",
                output.code
            )));
        }
    }
    let _ = log_writer.finish().await;
    cleanup_passlog(&passlog_path(paths, &job.plan_item, "preview"));
    if cancel.is_cancelled() {
        cleanup_preview_files(job, paths);
        return Err(RuntimeError::Cancelled);
    }
    let source_size = match std::fs::metadata(&job.source_sample_path) {
        Ok(value) => value.len(),
        Err(error) => {
            cleanup_preview_files(job, paths);
            return Err(error.into());
        }
    };
    let encoded_size = match std::fs::metadata(&job.encoded_sample_path) {
        Ok(value) => value.len(),
        Err(error) => {
            cleanup_preview_files(job, paths);
            return Err(error.into());
        }
    };
    let ratio = if source_size > 0 { encoded_size as f64 / source_size as f64 } else { 0.0 };
    let full = (std::fs::metadata(&job.source_path)?.len() as f64 * ratio).round() as u64;
    let mut notes = job.window.notes.clone();
    notes.push("Preview output size is estimated from the sample and is not an exact full-output guarantee.".into());
    Ok(vc_core::PreviewResult {
        job: job.clone(),
        success: true,
        source_sample_size: source_size,
        encoded_sample_size: encoded_size,
        sample_compression_ratio: ratio,
        estimated_full_output_size: full,
        notes,
        log_path: Some(log_path),
        error_message: None,
    })
}

fn prepare_output_backup(
    paths: &AppPaths,
    item: &EncodePlanItem,
) -> Result<Option<PathBuf>, RuntimeError> {
    if !item.settings.overwrite || !item.output_path.exists() {
        return Ok(None);
    }
    let backup = paths.temp_dir.join(format!("{}.overwrite-backup", token(&item.source_path)));
    if backup.exists() {
        return Err(RuntimeError::Encode(format!(
            "An output backup already exists; recover it before retrying: {}",
            backup.display()
        )));
    }
    std::fs::rename(&item.output_path, &backup)?;
    Ok(Some(backup))
}

fn restore_output(output: &Path, backup: Option<&Path>, existed_before: bool) {
    if let Some(backup) = backup {
        let _ = std::fs::remove_file(output);
        let _ = std::fs::rename(backup, output);
    } else if !existed_before {
        let _ = std::fs::remove_file(output);
    }
}

fn discard_output_backup(backup: Option<&Path>) {
    if let Some(backup) = backup {
        let _ = std::fs::remove_file(backup);
    }
}

fn cleanup_preview_files(job: &vc_core::PreviewJob, paths: &AppPaths) {
    cleanup_passlog(&passlog_path(paths, &job.plan_item, "preview"));
    let _ = std::fs::remove_file(&job.source_sample_path);
    let _ = std::fs::remove_file(&job.encoded_sample_path);
}

fn token(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("item")
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() || matches!(value, '.' | '_' | '-') {
                value
            } else {
                '_'
            }
        })
        .collect()
}
