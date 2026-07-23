mod contracts;

use contracts::{
    ActivityEventDto, ApiErrorDto, AppSettingsDto, BootstrapDto, PlanItemDto, PlanRequestDto,
    PlanResponseDto, PreviewOptionsDto, PreviewResultDto, QueueItemDto, QueueItemResultDto,
    QueueMetricsDto, QueueProgressDto, QueueSnapshotDto, QueueStateDto, QueueStreamMessage,
    SettingsDto,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::ipc::Channel;
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindowBuilder,
};
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use vc_core::queue::{QueueItemStatus, QueueRunState};
use vc_core::{
    AudioMode, Codec, CompressionRatio, ContainerFormat, DecodeAcceleration, EncodeSettings,
    EncoderBackend, PreviewOptions, PreviewSampleMode,
};
use vc_runtime::queue::supervisor::{DEFAULT_CLOSE_IDLE_TIMEOUT, QueueSnapshot, WaitForIdleError};
use vc_runtime::storage::window_state::{
    GeometryEventKind, WindowGeometry, WindowGeometryRuntime, WindowStateStore,
    classify_geometry_event,
};
use vc_runtime::{Application, DEFAULT_ACTIVITY_HISTORY_LIMIT, PlanRequest, RuntimeError};

/// Registry of cancellable IPC stream subscriptions.
#[derive(Clone, Default)]
pub struct SubscriptionRegistry {
    inner: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl SubscriptionRegistry {
    pub fn insert(&self, token: CancellationToken) -> String {
        let id = Uuid::new_v4().to_string();
        if let Ok(mut map) = self.inner.lock() {
            map.insert(id.clone(), token);
        }
        id
    }

    pub fn cancel(&self, id: &str) -> bool {
        let token = self.inner.lock().ok().and_then(|mut map| map.remove(id));
        if let Some(token) = token {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub fn remove(&self, id: &str) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(id);
        }
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|map| map.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Clone)]
pub struct AppRuntime {
    pub application: Application,
    pub geometry: WindowGeometryRuntime,
    pub subscriptions: SubscriptionRegistry,
}

async fn blocking_api<T, F>(operation: F) -> Result<T, ApiErrorDto>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, RuntimeError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| ApiErrorDto { code: "join".into(), message: error.to_string() })?
        .map_err(api_error)
}

fn restore_window_geometry_from_cache(window: &tauri::WebviewWindow, geometry: &WindowGeometry) {
    if geometry.width > 0 && geometry.height > 0 {
        let _ = window.set_size(PhysicalSize::new(geometry.width, geometry.height));
    }
    if let (Some(x), Some(y)) = (geometry.x, geometry.y) {
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    if geometry.maximized {
        let _ = window.maximize();
    }
}

fn read_window_geometry(window: &tauri::Window) -> Option<WindowGeometry> {
    let size = window.inner_size().ok()?;
    let position = window.outer_position().ok();
    let maximized = window.is_maximized().unwrap_or(false);
    Some(WindowGeometry {
        width: size.width,
        height: size.height,
        x: position.as_ref().map(|value| value.x),
        y: position.as_ref().map(|value| value.y),
        maximized,
    })
}

fn note_and_schedule_geometry(runtime: &WindowGeometryRuntime, window: &tauri::Window) {
    let Some(geometry) = read_window_geometry(window) else {
        return;
    };
    runtime.note_geometry(window.label(), geometry);
    let generation = runtime.bump_generation();
    let runtime = runtime.clone();
    tauri::async_runtime::spawn(async move {
        runtime.schedule_save_after(generation, tokio::time::sleep).await;
    });
}

fn flush_geometry_async(runtime: WindowGeometryRuntime) {
    tauri::async_runtime::spawn(async move {
        let result =
            tauri::async_runtime::spawn_blocking(move || runtime.flush_pending_now()).await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("window geometry flush failed: {error}"),
            Err(error) => eprintln!("window geometry flush join failed: {error}"),
        }
    });
}

fn app_settings_to_dto(value: &vc_runtime::storage::app_config::AppConfig) -> AppSettingsDto {
    AppSettingsDto {
        language: value.language.clone(),
        default_preset_name: value.default_preset_name.clone(),
        keep_preview_temp: value.keep_preview_temp,
        recent_paths: value.recent_paths.clone(),
        last_source_path: value.last_source_path.clone(),
        last_output_dir: value.last_output_dir.clone(),
        workdir_path: value.workdir_path.clone(),
        ffmpeg_path: value.ffmpeg_path.clone(),
        ffprobe_path: value.ffprobe_path.clone(),
        log_level: value.log_level.clone(),
        queue_table_header_state: value.queue_table_header_state.clone(),
    }
}

fn app_settings_from_dto(
    value: AppSettingsDto,
    mut current: vc_runtime::storage::app_config::AppConfig,
) -> vc_runtime::storage::app_config::AppConfig {
    current.language = value.language;
    current.default_preset_name = value.default_preset_name;
    current.keep_preview_temp = value.keep_preview_temp;
    current.recent_paths = value.recent_paths;
    current.last_source_path = value.last_source_path;
    current.last_output_dir = value.last_output_dir;
    current.workdir_path = value.workdir_path;
    current.ffmpeg_path = value.ffmpeg_path;
    current.ffprobe_path = value.ffprobe_path;
    current.log_level = value.log_level;
    current.queue_table_header_state = value.queue_table_header_state;
    current
}

fn api_error(error: RuntimeError) -> ApiErrorDto {
    let code = match error {
        RuntimeError::ToolDiscovery(_) => "tool_discovery",
        RuntimeError::Capability(_) => "capability",
        RuntimeError::Planning(_) | RuntimeError::Probe(_) => "planning",
        RuntimeError::Encode(_) | RuntimeError::Cancelled | RuntimeError::Background(_) => {
            "execution"
        }
        RuntimeError::Config(_) => "configuration",
        RuntimeError::Queue(_) => "queue",
        RuntimeError::Io(_) | RuntimeError::Json(_) | RuntimeError::ToolFailed { .. } => "runtime",
    };
    ApiErrorDto { code: code.into(), message: error.to_string() }
}

fn parse_codec(value: &str) -> Result<Codec, ApiErrorDto> {
    match value {
        "hevc" => Ok(Codec::Hevc),
        "av1" => Ok(Codec::Av1),
        _ => Err(invalid("codec", value)),
    }
}

fn parse_backend(value: &str) -> Result<EncoderBackend, ApiErrorDto> {
    match value {
        "auto" => Ok(EncoderBackend::Auto),
        "cpu" => Ok(EncoderBackend::Cpu),
        "nvenc" => Ok(EncoderBackend::Nvenc),
        "qsv" => Ok(EncoderBackend::Qsv),
        "amf" => Ok(EncoderBackend::Amf),
        "videotoolbox" => Ok(EncoderBackend::VideoToolbox),
        _ => Err(invalid("backend", value)),
    }
}

fn parse_decode(value: &str) -> Result<DecodeAcceleration, ApiErrorDto> {
    match value {
        "software" => Ok(DecodeAcceleration::Software),
        "videotoolbox" => Ok(DecodeAcceleration::VideoToolbox),
        _ => Err(invalid("decodeAcceleration", value)),
    }
}

fn parse_container(value: &str) -> Result<ContainerFormat, ApiErrorDto> {
    match value {
        "mkv" => Ok(ContainerFormat::Mkv),
        "mp4" => Ok(ContainerFormat::Mp4),
        _ => Err(invalid("container", value)),
    }
}

fn parse_audio(value: &str) -> Result<AudioMode, ApiErrorDto> {
    match value {
        "copy" => Ok(AudioMode::Copy),
        "aac" => Ok(AudioMode::Aac),
        _ => Err(invalid("audioMode", value)),
    }
}

fn invalid(field: &str, value: &str) -> ApiErrorDto {
    ApiErrorDto { code: "validation".into(), message: format!("Invalid {field} value: {value}") }
}

fn settings_from_dto(value: SettingsDto) -> Result<EncodeSettings, ApiErrorDto> {
    let ratio = value
        .ratio
        .map(|ratio| {
            CompressionRatio::new(ratio).map_err(|error| ApiErrorDto {
                code: "validation".into(),
                message: error.to_string(),
            })
        })
        .transpose()?;
    let parallel_backends = value
        .parallel_backends
        .iter()
        .map(|backend| parse_backend(backend))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EncodeSettings {
        codec: parse_codec(&value.codec)?,
        backend: parse_backend(&value.backend)?,
        decode_acceleration: parse_decode(&value.decode_acceleration)?,
        parallel_enabled: value.parallel_enabled,
        parallel_backends,
        ratio,
        min_video_kbps: value.min_video_kbps,
        max_video_kbps: value.max_video_kbps,
        container: parse_container(&value.container)?,
        audio_mode: parse_audio(&value.audio_mode)?,
        audio_bitrate: value.audio_bitrate,
        copy_subtitles: value.copy_subtitles,
        copy_external_subtitles: value.copy_external_subtitles,
        two_pass: value.two_pass,
        encoder_preset: value.encoder_preset,
        pixel_format: value.pixel_format,
        maxrate_factor: value.maxrate_factor,
        bufsize_factor: value.bufsize_factor,
        overwrite: value.overwrite,
        recursive: value.recursive,
        dry_run: false,
    })
}

fn settings_to_dto(value: &EncodeSettings) -> SettingsDto {
    SettingsDto {
        codec: value.codec.as_str().into(),
        backend: value.backend.as_str().into(),
        decode_acceleration: value.decode_acceleration.as_str().into(),
        parallel_enabled: value.parallel_enabled,
        parallel_backends: value
            .parallel_backends
            .iter()
            .map(|backend| backend.as_str().into())
            .collect(),
        ratio: value.ratio.map(|ratio| ratio.get()),
        min_video_kbps: value.min_video_kbps,
        max_video_kbps: value.max_video_kbps,
        container: value.container.as_str().into(),
        audio_mode: value.audio_mode.as_str().into(),
        audio_bitrate: value.audio_bitrate.clone(),
        copy_subtitles: value.copy_subtitles,
        copy_external_subtitles: value.copy_external_subtitles,
        two_pass: value.two_pass,
        encoder_preset: value.encoder_preset.clone(),
        pixel_format: value.pixel_format.clone(),
        maxrate_factor: value.maxrate_factor,
        bufsize_factor: value.bufsize_factor,
        overwrite: value.overwrite,
        recursive: value.recursive,
    }
}

fn request_from_dto(value: PlanRequestDto) -> Result<PlanRequest, ApiErrorDto> {
    Ok(PlanRequest {
        input_path: PathBuf::from(value.input_path),
        output_dir: value.output_dir.map(PathBuf::from),
        workdir: value.workdir.map(PathBuf::from),
        ffmpeg_path: value.ffmpeg_path.map(PathBuf::from),
        ffprobe_path: value.ffprobe_path.map(PathBuf::from),
        settings: settings_from_dto(value.settings)?,
        force_capability_refresh: false,
    })
}

fn plan_item_to_dto(item: &vc_core::EncodePlanItem) -> PlanItemDto {
    PlanItemDto {
        source_path: item.source_path.to_string_lossy().into_owned(),
        output_path: item.output_path.to_string_lossy().into_owned(),
        width: item.media_info.as_ref().and_then(|media| media.width),
        height: item.media_info.as_ref().and_then(|media| media.height),
        duration: item.media_info.as_ref().map(|media| media.duration),
        source_bitrate: item.media_info.as_ref().map(|media| media.video_bitrate_bps),
        target_bitrate: item.target_video_bitrate_bps,
        encoder: item.encoder.as_ref().map(|encoder| encoder.encoder_name.clone()),
        backend: item.encoder.as_ref().map(|encoder| encoder.backend.as_str().into()),
        warnings: item.warnings.iter().map(|warning| warning.0.clone()).collect(),
        skip_reason: item.skip_reason.as_ref().map(|reason| reason.0.clone()),
    }
}

fn plan_to_dto(plan: &vc_runtime::EncodePlan) -> PlanResponseDto {
    PlanResponseDto {
        items: plan.items.iter().map(plan_item_to_dto).collect(),
        ffmpeg_path: plan.ffmpeg_path.to_string_lossy().into_owned(),
        ffprobe_path: plan.ffprobe_path.to_string_lossy().into_owned(),
        input_root: plan.input_root.to_string_lossy().into_owned(),
        output_root: plan.output_root.to_string_lossy().into_owned(),
    }
}

fn queue_item_status(status: &QueueItemStatus) -> &'static str {
    match status {
        QueueItemStatus::Draft => "draft",
        QueueItemStatus::Queued => "queued",
        QueueItemStatus::Running => "running",
        QueueItemStatus::Done => "done",
        QueueItemStatus::Failed => "failed",
        QueueItemStatus::Skipped => "skipped",
        QueueItemStatus::Cancelled => "cancelled",
    }
}

fn queue_run_state(state: &QueueRunState) -> &'static str {
    match state {
        QueueRunState::Idle => "idle",
        QueueRunState::Running => "running",
        QueueRunState::PauseRequested => "pause_requested",
        QueueRunState::Paused => "paused",
        QueueRunState::Cancelling => "cancelling",
    }
}

fn queue_item_result_to_dto(result: &vc_core::queue::ItemResult) -> QueueItemResultDto {
    QueueItemResultDto {
        success: result.success,
        skipped: result.skipped,
        return_code: result.return_code,
        output_path: result.output_path.as_ref().map(|path| path.to_string_lossy().into_owned()),
        log_path: result.log_path.as_ref().map(|path| path.to_string_lossy().into_owned()),
        error: result.error.clone(),
    }
}

fn queue_snapshot_to_dto(snapshot: &QueueSnapshot) -> QueueSnapshotDto {
    let items = snapshot
        .state
        .items
        .iter()
        .map(|item| QueueItemDto {
            item_id: item.item_id.clone(),
            plan: plan_item_to_dto(&item.plan),
            status: queue_item_status(&item.status).into(),
            progress: QueueProgressDto {
                percent: item.progress.percent,
                speed: item.progress.speed.clone(),
                elapsed_sec: item.progress.elapsed_sec,
                current_pass: item.progress.current_pass,
                total_passes: item.progress.total_passes,
            },
            error: item.error.as_ref().map(|error| error.message.clone()),
            result: item.result.as_ref().map(queue_item_result_to_dto),
            run_id: item.run_id.clone(),
        })
        .collect::<Vec<_>>();
    let metrics = &snapshot.metrics;
    QueueSnapshotDto {
        state: QueueStateDto {
            run_state: queue_run_state(&snapshot.state.run_state).into(),
            active_run_id: snapshot.state.active_run_id.clone(),
            items,
        },
        metrics: QueueMetricsDto {
            total_items: metrics.total_items,
            queued_items: metrics.queued_items,
            running_items: metrics.running_items,
            failed_items: metrics.failed_items,
            done_items: metrics.done_items,
            skipped_items: metrics.skipped_items,
            cancelled_items: metrics.cancelled_items,
            ready_items: metrics.ready_items,
            completed_items: metrics.completed_items,
            total_duration_sec: metrics.total_duration_sec,
            estimated_saved_bytes: metrics.estimated_saved_bytes,
            queue_percent: metrics.queue_percent,
            eta_sec: metrics.eta_sec,
            current_item_id: metrics.current_item_id.clone(),
            current_file_name: metrics.current_file_name.clone(),
            current_file_percent: metrics.current_file_percent,
            current_speed: metrics.current_speed.clone(),
        },
    }
}

#[tauri::command]
async fn open_aux_window(
    app: AppHandle,
    state: State<'_, AppRuntime>,
    kind: String,
) -> Result<(), ApiErrorDto> {
    let (label, title, width, height) = match kind.as_str() {
        "queue" => ("queue", "Queue", 920.0, 520.0),
        "activity" => ("activity", "Activity Log", 760.0, 520.0),
        "presets" => ("presets", "Preset Manager", 620.0, 460.0),
        "settings" => ("settings", "Settings", 760.0, 560.0),
        "preview" => ("preview", "Preview Result", 760.0, 520.0),
        value => return Err(invalid("window", value)),
    };
    if let Some(window) = app.get_webview_window(label) {
        window
            .set_focus()
            .map_err(|error| ApiErrorDto { code: "window".into(), message: error.to_string() })?;
        return Ok(());
    }
    // Geometry comes from the in-memory cache only — no disk read on the command path.
    let cached = state.geometry.get(label);
    let window = WebviewWindowBuilder::new(
        &app,
        label,
        WebviewUrl::App(format!("index.html?window={kind}").into()),
    )
    .title(title)
    .inner_size(width, height)
    .min_inner_size(480.0, 360.0)
    .resizable(true)
    .center()
    .build()
    .map_err(|error| ApiErrorDto { code: "window".into(), message: error.to_string() })?;
    if let Some(geometry) = cached {
        restore_window_geometry_from_cache(&window, &geometry);
    }
    Ok(())
}

#[tauri::command]
async fn bootstrap(state: State<'_, AppRuntime>) -> Result<BootstrapDto, ApiErrorDto> {
    let snapshot = state.application.bootstrap_snapshot().await.map_err(api_error)?;
    Ok(BootstrapDto {
        language: snapshot.config.language.clone(),
        default_preset_name: snapshot.config.default_preset_name.clone(),
        ffmpeg_path: snapshot.ffmpeg_path.map(|path| path.to_string_lossy().into_owned()),
        ffprobe_path: snapshot.ffprobe_path.map(|path| path.to_string_lossy().into_owned()),
        settings: settings_to_dto(&state.application.default_settings().map_err(api_error)?),
        app_settings: app_settings_to_dto(&snapshot.config),
        queue: queue_snapshot_to_dto(&snapshot.queue),
    })
}

#[tauri::command]
async fn plan_encode(
    state: State<'_, AppRuntime>,
    request: PlanRequestDto,
) -> Result<PlanResponseDto, ApiErrorDto> {
    let plan = state.application.plan(request_from_dto(request)?).await.map_err(api_error)?;
    Ok(plan_to_dto(&plan))
}

#[tauri::command]
async fn queue_add(
    state: State<'_, AppRuntime>,
    request: PlanRequestDto,
) -> Result<PlanResponseDto, ApiErrorDto> {
    let plan = state.application.plan(request_from_dto(request)?).await.map_err(api_error)?;
    state.application.queue.set_workdir(plan.workdir.clone()).await;
    state.application.queue.enqueue(plan.items.clone()).await.map_err(api_error)?;
    Ok(plan_to_dto(&plan))
}

#[tauri::command]
async fn queue_start(state: State<'_, AppRuntime>) -> Result<(), ApiErrorDto> {
    state.application.start_queue().await.map_err(api_error)
}

#[tauri::command]
async fn queue_pause_after_current(state: State<'_, AppRuntime>) -> Result<(), ApiErrorDto> {
    state.application.queue.pause_after_current().await.map_err(api_error)
}

#[tauri::command]
async fn queue_stop(state: State<'_, AppRuntime>) -> Result<(), ApiErrorDto> {
    state.application.queue.stop().await.map_err(api_error)
}

#[tauri::command]
async fn queue_reorder(
    state: State<'_, AppRuntime>,
    ordered_ids: Vec<String>,
) -> Result<(), ApiErrorDto> {
    state.application.queue.reorder(ordered_ids).await.map_err(api_error)
}

#[tauri::command]
async fn queue_retry(
    state: State<'_, AppRuntime>,
    item_ids: Vec<String>,
) -> Result<(), ApiErrorDto> {
    state.application.queue.retry(item_ids).await.map_err(api_error)
}

#[tauri::command]
async fn queue_remove(
    state: State<'_, AppRuntime>,
    item_ids: Vec<String>,
) -> Result<(), ApiErrorDto> {
    state.application.queue.remove(item_ids).await.map_err(api_error)
}

#[tauri::command]
async fn queue_clear_completed(state: State<'_, AppRuntime>) -> Result<(), ApiErrorDto> {
    state.application.queue.clear_completed().await.map_err(api_error)
}

#[tauri::command]
fn queue_subscribe(
    state: State<'_, AppRuntime>,
    channel: Channel<QueueStreamMessage>,
) -> Result<String, ApiErrorDto> {
    let receiver = state.application.queue.subscribe();
    let initial = receiver.borrow().as_ref().clone();
    channel
        .send(QueueStreamMessage::Snapshot(queue_snapshot_to_dto(&initial)))
        .map_err(|error| ApiErrorDto { code: "ipc".into(), message: error.to_string() })?;
    let cancel = CancellationToken::new();
    let subscription_id = state.subscriptions.insert(cancel.clone());
    let registry = state.subscriptions.clone();
    let id_for_task = subscription_id.clone();
    tauri::async_runtime::spawn(async move {
        let mut receiver = receiver;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                changed = receiver.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let snapshot = receiver.borrow().as_ref().clone();
                    if channel
                        .send(QueueStreamMessage::Snapshot(queue_snapshot_to_dto(&snapshot)))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
        registry.remove(&id_for_task);
    });
    Ok(subscription_id)
}

#[tauri::command]
fn queue_unsubscribe(
    state: State<'_, AppRuntime>,
    subscription_id: String,
) -> Result<(), ApiErrorDto> {
    state.subscriptions.cancel(&subscription_id);
    Ok(())
}

#[tauri::command]
fn activity_subscribe(
    state: State<'_, AppRuntime>,
    channel: Channel<QueueStreamMessage>,
) -> Result<String, ApiErrorDto> {
    let mut receiver = state.application.activity.subscribe();
    let cancel = CancellationToken::new();
    let subscription_id = state.subscriptions.insert(cancel.clone());
    let registry = state.subscriptions.clone();
    let id_for_task = subscription_id.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                event = receiver.recv() => {
                    let Ok(event) = event else { break; };
                    let message = QueueStreamMessage::Activity(ActivityEventDto {
                        category: event.category,
                        message: event.message,
                        timestamp: event.timestamp,
                    });
                    if channel.send(message).is_err() {
                        break;
                    }
                }
            }
        }
        registry.remove(&id_for_task);
    });
    Ok(subscription_id)
}

#[tauri::command]
fn activity_unsubscribe(
    state: State<'_, AppRuntime>,
    subscription_id: String,
) -> Result<(), ApiErrorDto> {
    state.subscriptions.cancel(&subscription_id);
    Ok(())
}

#[tauri::command]
fn subscription_count(state: State<'_, AppRuntime>) -> usize {
    state.subscriptions.len()
}

#[tauri::command]
async fn activity_history(
    state: State<'_, AppRuntime>,
    limit: Option<usize>,
) -> Result<Vec<ActivityEventDto>, ApiErrorDto> {
    let activity = state.application.activity.clone();
    let limit = limit.unwrap_or(DEFAULT_ACTIVITY_HISTORY_LIMIT);
    blocking_api(move || {
        Ok(activity
            .history_tail(limit)
            .into_iter()
            .map(|event| ActivityEventDto {
                category: event.category,
                message: event.message,
                timestamp: event.timestamp,
            })
            .collect())
    })
    .await
}

#[tauri::command]
fn activity_clear(state: State<'_, AppRuntime>) {
    state.application.activity.clear();
}

#[tauri::command]
async fn activity_export(state: State<'_, AppRuntime>, path: String) -> Result<(), ApiErrorDto> {
    let activity = state.application.activity.clone();
    blocking_api(move || activity.export(&PathBuf::from(path))).await
}

#[tauri::command]
async fn redetect_encoders(state: State<'_, AppRuntime>) -> Result<(), ApiErrorDto> {
    state.application.refresh_capabilities().await.map_err(api_error)
}

#[tauri::command]
async fn save_settings(
    state: State<'_, AppRuntime>,
    settings: SettingsDto,
) -> Result<(), ApiErrorDto> {
    let application = state.application.clone();
    let settings = settings_from_dto(settings)?;
    blocking_api(move || application.save_settings(&settings)).await
}

#[tauri::command]
async fn save_app_settings(
    state: State<'_, AppRuntime>,
    settings: AppSettingsDto,
) -> Result<(), ApiErrorDto> {
    let application = state.application.clone();
    blocking_api(move || {
        let current = application.config()?;
        application.save_config(&app_settings_from_dto(settings, current))
    })
    .await
}

#[tauri::command]
async fn preset_list(state: State<'_, AppRuntime>) -> Result<Vec<String>, ApiErrorDto> {
    let application = state.application.clone();
    blocking_api(move || application.presets.list()).await
}

#[tauri::command]
async fn preset_load(
    state: State<'_, AppRuntime>,
    name: String,
) -> Result<SettingsDto, ApiErrorDto> {
    let application = state.application.clone();
    blocking_api(move || application.presets.load(&name).map(|settings| settings_to_dto(&settings)))
        .await
}

#[tauri::command]
async fn preset_save(
    state: State<'_, AppRuntime>,
    name: String,
    settings: SettingsDto,
) -> Result<String, ApiErrorDto> {
    let application = state.application.clone();
    let settings = settings_from_dto(settings)?;
    blocking_api(move || {
        application.presets.save(&name, &settings).map(|path| path.to_string_lossy().into_owned())
    })
    .await
}

#[tauri::command]
async fn preset_delete(state: State<'_, AppRuntime>, name: String) -> Result<(), ApiErrorDto> {
    let application = state.application.clone();
    blocking_api(move || application.presets.delete(&name)).await
}

#[tauri::command]
async fn preview(
    state: State<'_, AppRuntime>,
    request: PlanRequestDto,
    options: PreviewOptionsDto,
) -> Result<PreviewResultDto, ApiErrorDto> {
    let sample_mode = match options.sample_mode.as_str() {
        "middle" => PreviewSampleMode::Middle,
        "custom" => PreviewSampleMode::Custom,
        value => return Err(invalid("sampleMode", value)),
    };
    let result = state
        .application
        .preview(
            request_from_dto(request)?,
            PreviewOptions {
                sample_mode,
                sample_duration_sec: options.sample_duration_sec,
                custom_start_sec: options.custom_start_sec,
            },
            CancellationToken::new(),
        )
        .await
        .map_err(api_error)?;
    Ok(PreviewResultDto {
        success: result.success,
        source_path: result.job.source_path.to_string_lossy().into_owned(),
        source_sample_path: result.job.source_sample_path.to_string_lossy().into_owned(),
        encoded_sample_path: result.job.encoded_sample_path.to_string_lossy().into_owned(),
        sample_source_size: result.source_sample_size,
        sample_encoded_size: result.encoded_sample_size,
        sample_compression_ratio: result.sample_compression_ratio,
        estimated_full_output_size: result.estimated_full_output_size,
        notes: result.notes,
        log_path: result.log_path.map(|path| path.to_string_lossy().into_owned()),
        error_message: result.error_message,
    })
}

pub fn run() {
    let application = match Application::current() {
        Ok(application) => application,
        Err(error) => {
            eprintln!("VideoCompressR initialization failed: {error}");
            return;
        }
    };
    let close_application = application.clone();
    let geometry = WindowGeometryRuntime::load(WindowStateStore::new(application.paths.clone()));
    let geometry_for_setup = geometry.clone();
    let geometry_for_event = geometry.clone();
    let close_pending = Arc::new(AtomicBool::new(false));
    let close_pending_for_event = close_pending.clone();
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppRuntime {
            application,
            geometry,
            subscriptions: SubscriptionRegistry::default(),
        })
        .setup(move |app| {
            if let Some(window) = app.get_webview_window("main") {
                if let Some(cached) = geometry_for_setup.get(window.label()) {
                    restore_window_geometry_from_cache(&window, &cached);
                }
            }
            Ok(())
        })
        .on_window_event(move |window, event| {
            // Never perform disk I/O or block_on on the UI event thread.
            let kind = match event {
                tauri::WindowEvent::Moved(_) => "Moved",
                tauri::WindowEvent::Resized(_) => "Resized",
                tauri::WindowEvent::CloseRequested { .. } => "CloseRequested",
                tauri::WindowEvent::Destroyed => "Destroyed",
                _ => "Other",
            };
            match classify_geometry_event(kind) {
                GeometryEventKind::MovedOrResized => {
                    note_and_schedule_geometry(&geometry_for_event, window);
                }
                GeometryEventKind::CloseOrDestroyed => {
                    if let Some(geometry) = read_window_geometry(window) {
                        geometry_for_event.note_geometry(window.label(), geometry);
                    }
                    flush_geometry_async(geometry_for_event.clone());
                }
                GeometryEventKind::Irrelevant => {}
            }

            if window.label() != "main" {
                return;
            }
            let tauri::WindowEvent::CloseRequested { api, .. } = event else {
                return;
            };
            // Synchronous watch borrow — no block_on.
            let snapshot = close_application.queue.snapshot_now();
            if matches!(
                snapshot.state.run_state,
                QueueRunState::Running | QueueRunState::PauseRequested | QueueRunState::Cancelling
            ) {
                api.prevent_close();
                if close_pending_for_event.swap(true, Ordering::SeqCst) {
                    return;
                }
                let application = close_application.clone();
                let window = window.clone();
                let close_pending = close_pending_for_event.clone();
                tauri::async_runtime::spawn(async move {
                    application.activity.emit(
                        "queue",
                        "Close requested while the queue is busy; stopping the active process.",
                    );
                    let _ = application.queue.stop().await;
                    match application.queue.wait_until_idle(DEFAULT_CLOSE_IDLE_TIMEOUT).await {
                        Ok(()) => {}
                        Err(WaitForIdleError::TimedOut) => {
                            eprintln!(
                                "queue did not become idle within {:?}; force-aborting",
                                DEFAULT_CLOSE_IDLE_TIMEOUT
                            );
                            application.activity.emit(
                                "error",
                                "Close timed out waiting for the queue; force-aborting active run.",
                            );
                            let _ = application
                                .queue
                                .force_abort_active_run("window close timeout")
                                .await;
                            // Best-effort short wait after force abort.
                            let _ = application.queue.wait_until_idle(Duration::from_secs(2)).await;
                        }
                        Err(WaitForIdleError::Closed) => {}
                    }
                    let _ = window.close();
                    close_pending.store(false, Ordering::SeqCst);
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            open_aux_window,
            plan_encode,
            queue_add,
            queue_start,
            queue_pause_after_current,
            queue_stop,
            queue_reorder,
            queue_retry,
            queue_remove,
            queue_clear_completed,
            queue_subscribe,
            queue_unsubscribe,
            activity_subscribe,
            activity_unsubscribe,
            subscription_count,
            activity_history,
            activity_clear,
            activity_export,
            redetect_encoders,
            save_settings,
            save_app_settings,
            preset_list,
            preset_load,
            preset_save,
            preset_delete,
            preview
        ]);
    if let Err(error) = builder.run(tauri::generate_context!()) {
        eprintln!("VideoCompressR desktop exited with an error: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_event_handler_source_has_no_block_on() {
        let source = include_str!("lib.rs");
        // Find the on_window_event closure body heuristically.
        let start = source.find(".on_window_event").expect("on_window_event present");
        let slice = &source[start..];
        let end = slice.find(".invoke_handler").unwrap_or(slice.len());
        let handler = &slice[..end];
        assert!(!handler.contains("block_on("), "window event handler must not call block_on");
        assert!(
            !handler.contains("block_in_place"),
            "window event handler must not call block_in_place"
        );
        assert!(
            !handler.contains("store.save") && !handler.contains(".save("),
            "window event handler must not save window state synchronously"
        );
        assert!(!handler.contains("sync_all"), "window event handler must not fsync");
        assert!(handler.contains("snapshot_now"), "close path should use snapshot_now");
    }

    #[test]
    fn file_io_commands_are_async() {
        let source = include_str!("lib.rs");
        for name in [
            "save_settings",
            "save_app_settings",
            "preset_list",
            "preset_load",
            "preset_save",
            "preset_delete",
            "activity_history",
            "activity_export",
            "open_aux_window",
        ] {
            let needle = format!("async fn {name}");
            assert!(source.contains(&needle), "expected {name} to be an async command");
        }
        assert!(
            source.contains("async fn blocking_api")
                || source.contains("fn blocking_api")
                || source.contains("async fn blocking_api")
                || source.contains("blocking_api")
        );
    }

    #[tokio::test]
    async fn blocking_api_propagates_runtime_errors() {
        let result = blocking_api(|| Err::<(), _>(RuntimeError::Config("boom".into()))).await;
        let err = result.expect_err("should fail");
        assert_eq!(err.code, "configuration");
        assert!(err.message.contains("boom"));
    }

    #[test]
    fn unsubscribe_is_idempotent() {
        let registry = SubscriptionRegistry::default();
        let token = CancellationToken::new();
        let id = registry.insert(token);
        assert!(registry.cancel(&id));
        assert!(!registry.cancel(&id));
        assert_eq!(registry.len(), 0);
    }
}
