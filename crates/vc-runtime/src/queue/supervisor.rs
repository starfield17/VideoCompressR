use crate::activity::ActivityHub;
use crate::error::RuntimeError;
use crate::execution::{ProgressEvent, ProgressSink, execute_item};
use crate::ffmpeg::{ToolPaths, capabilities::ensure_capabilities};
use crate::platform::paths::AppPaths;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::{Mutex, Notify, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use vc_core::EncodePlanItem;
use vc_core::queue::{
    ItemProgress, QueueCommand, QueueError, QueueExecutionProfile, QueueMetrics, QueueRunState,
    QueueState, apply, compute_metrics, execution_profile,
};

/// Maximum UI snapshot publish rate for coalesced (progress) updates.
pub const SNAPSHOT_COALESCE_INTERVAL: Duration = Duration::from_millis(200);
/// Progress events are applied on a single worker; channel bounds memory.
pub const PROGRESS_CHANNEL_CAPACITY: usize = 256;
/// Default wait for queue idle on window close.
pub const DEFAULT_CLOSE_IDLE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct QueueSnapshot {
    pub state: QueueState,
    pub metrics: QueueMetrics,
}

#[derive(Clone)]
pub struct ExecutionContext {
    pub paths: AppPaths,
    pub tools: ToolPaths,
    pub activity: ActivityHub,
}

#[derive(Clone)]
struct ActiveRun {
    run_id: String,
    cancel: CancellationToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SnapshotPublishMode {
    Immediate,
    Coalesced,
}

fn publish_mode(command: &QueueCommand) -> SnapshotPublishMode {
    match command {
        QueueCommand::ReportProgress { .. } => SnapshotPublishMode::Coalesced,
        _ => SnapshotPublishMode::Immediate,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitForIdleError {
    TimedOut,
    Closed,
}

#[derive(Clone)]
pub struct QueueSupervisor {
    inner: Arc<QueueSupervisorInner>,
}

struct QueueSupervisorInner {
    state: Arc<Mutex<QueueState>>,
    snapshots: watch::Sender<Arc<QueueSnapshot>>,
    hub: ActivityHub,
    active_run: Arc<Mutex<Option<ActiveRun>>>,
    run_control: Arc<Mutex<()>>,
    workdir: Arc<Mutex<Option<PathBuf>>>,
    /// Progress updates are applied by a single long-lived worker (no per-event spawn).
    progress_tx: mpsc::Sender<ProgressUpdate>,
    progress_rx: Mutex<Option<mpsc::Receiver<ProgressUpdate>>>,
    snapshot_dirty: Arc<AtomicBool>,
    snapshot_notify: Arc<Notify>,
    background_cancel: CancellationToken,
    lifecycle: Mutex<BackgroundLifecycle>,
    /// Counts of spawned progress worker tasks (should stay 1 for the supervisor lifetime).
    progress_worker_spawns: Arc<AtomicU64>,
    /// Counts of spawned snapshot publisher tasks (should stay 1 for the supervisor lifetime).
    snapshot_worker_spawns: Arc<AtomicU64>,
    /// Counts of snapshot publishes (for tests).
    snapshot_publish_count: Arc<AtomicU64>,
    /// Counts of compute_metrics calls (for tests).
    metrics_compute_count: Arc<AtomicU64>,
}

struct BackgroundLifecycle {
    started: bool,
    shutdown: bool,
    progress_handle: Option<JoinHandle<()>>,
    snapshot_handle: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct BackgroundContext {
    state: Arc<Mutex<QueueState>>,
    snapshots: watch::Sender<Arc<QueueSnapshot>>,
    snapshot_dirty: Arc<AtomicBool>,
    snapshot_notify: Arc<Notify>,
    snapshot_publish_count: Arc<AtomicU64>,
    metrics_compute_count: Arc<AtomicU64>,
}

struct ProgressUpdate {
    item_id: String,
    run_id: String,
    progress: ItemProgress,
}

impl QueueSupervisor {
    pub fn new(hub: ActivityHub) -> Self {
        let state = Arc::new(Mutex::new(QueueState::default()));
        let snapshot = Arc::new(QueueSnapshot {
            state: QueueState::default(),
            metrics: QueueMetrics::default(),
        });
        let (snapshots, _) = watch::channel(snapshot);
        let (progress_tx, progress_rx) = mpsc::channel(PROGRESS_CHANNEL_CAPACITY);
        Self {
            inner: Arc::new(QueueSupervisorInner {
                state,
                snapshots,
                hub,
                active_run: Arc::new(Mutex::new(None)),
                run_control: Arc::new(Mutex::new(())),
                workdir: Arc::new(Mutex::new(None)),
                progress_tx,
                progress_rx: Mutex::new(Some(progress_rx)),
                snapshot_dirty: Arc::new(AtomicBool::new(false)),
                snapshot_notify: Arc::new(Notify::new()),
                background_cancel: CancellationToken::new(),
                lifecycle: Mutex::new(BackgroundLifecycle {
                    started: false,
                    shutdown: false,
                    progress_handle: None,
                    snapshot_handle: None,
                }),
                progress_worker_spawns: Arc::new(AtomicU64::new(0)),
                snapshot_worker_spawns: Arc::new(AtomicU64::new(0)),
                snapshot_publish_count: Arc::new(AtomicU64::new(0)),
                metrics_compute_count: Arc::new(AtomicU64::new(0)),
            }),
        }
    }

    fn background_context(&self) -> BackgroundContext {
        BackgroundContext {
            state: self.inner.state.clone(),
            snapshots: self.inner.snapshots.clone(),
            snapshot_dirty: self.inner.snapshot_dirty.clone(),
            snapshot_notify: self.inner.snapshot_notify.clone(),
            snapshot_publish_count: self.inner.snapshot_publish_count.clone(),
            metrics_compute_count: self.inner.metrics_compute_count.clone(),
        }
    }

    /// Start the long-lived workers inside the caller's existing Tokio runtime.
    pub async fn ensure_background_tasks_started(&self) -> Result<(), RuntimeError> {
        let mut lifecycle = self.inner.lifecycle.lock().await;
        if lifecycle.shutdown {
            return Err(RuntimeError::Background("queue supervisor is shut down".into()));
        }
        if lifecycle.started {
            return Ok(());
        }
        let progress_rx = self.inner.progress_rx.lock().await.take().ok_or_else(|| {
            RuntimeError::Background("progress receiver was already taken".into())
        })?;
        let weak = Arc::downgrade(&self.inner);
        let context = self.background_context();
        let cancel = self.inner.background_cancel.clone();
        self.inner.progress_worker_spawns.fetch_add(1, Ordering::SeqCst);
        let progress_handle = tokio::spawn(run_progress_worker(
            weak.clone(),
            context.clone(),
            progress_rx,
            cancel.clone(),
        ));
        self.inner.snapshot_worker_spawns.fetch_add(1, Ordering::SeqCst);
        let snapshot_handle = tokio::spawn(run_snapshot_publisher(weak, context, cancel));
        lifecycle.started = true;
        lifecycle.progress_handle = Some(progress_handle);
        lifecycle.snapshot_handle = Some(snapshot_handle);
        Ok(())
    }

    pub async fn initialize(&self) -> Result<(), RuntimeError> {
        self.ensure_background_tasks_started().await
    }

    pub async fn shutdown(&self) {
        let (progress_handle, snapshot_handle) = {
            let mut lifecycle = self.inner.lifecycle.lock().await;
            if lifecycle.shutdown
                && lifecycle.progress_handle.is_none()
                && lifecycle.snapshot_handle.is_none()
            {
                return;
            }
            lifecycle.shutdown = true;
            self.inner.background_cancel.cancel();
            (lifecycle.progress_handle.take(), lifecycle.snapshot_handle.take())
        };
        if let Some(handle) = progress_handle {
            let _ = handle.await;
        }
        if let Some(handle) = snapshot_handle {
            let _ = handle.await;
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<QueueSnapshot>> {
        self.inner.snapshots.subscribe()
    }

    /// Synchronous watch-borrow; safe for UI event threads (no block_on).
    pub fn snapshot_now(&self) -> Arc<QueueSnapshot> {
        self.inner.snapshots.borrow().clone()
    }

    pub async fn snapshot(&self) -> Arc<QueueSnapshot> {
        self.snapshot_now()
    }

    pub fn snapshot_publish_count(&self) -> u64 {
        self.inner.snapshot_publish_count.load(Ordering::SeqCst)
    }

    pub fn metrics_compute_count(&self) -> u64 {
        self.inner.metrics_compute_count.load(Ordering::SeqCst)
    }

    pub fn progress_worker_spawns(&self) -> u64 {
        self.inner.progress_worker_spawns.load(Ordering::SeqCst)
    }

    pub fn snapshot_worker_spawns(&self) -> u64 {
        self.inner.snapshot_worker_spawns.load(Ordering::SeqCst)
    }

    /// Apply a progress event without spawning a new Tokio task.
    pub fn report_progress(&self, event: ProgressEvent, fallback_item_id: &str, run_id: &str) {
        let update = ProgressUpdate {
            item_id: event.item_id.unwrap_or_else(|| fallback_item_id.to_owned()),
            run_id: run_id.to_owned(),
            progress: ItemProgress {
                percent: event.percent.unwrap_or(0.0),
                speed: event.speed,
                elapsed_sec: event.elapsed_sec,
                current_pass: event.current_pass,
                total_passes: event.total_passes,
            },
        };
        // Drop when full: state still holds last progress; next accepted update refreshes.
        let _ = self.inner.progress_tx.try_send(update);
    }

    pub fn progress_sink(&self, item_id: String, run_id: String) -> ProgressSink {
        let supervisor = self.clone();
        Arc::new(move |event: ProgressEvent| {
            supervisor.report_progress(event, &item_id, &run_id);
        })
    }

    pub async fn enqueue(&self, plans: Vec<EncodePlanItem>) -> Result<(), RuntimeError> {
        self.apply(QueueCommand::Enqueue(plans)).await
    }
    pub async fn set_workdir(&self, workdir: PathBuf) {
        *self.inner.workdir.lock().await = Some(workdir);
    }
    pub async fn set_default_workdir(&self, workdir: PathBuf) {
        let mut value = self.inner.workdir.lock().await;
        if value.is_none() {
            *value = Some(workdir);
        }
    }
    pub async fn retry(&self, ids: Vec<String>) -> Result<(), RuntimeError> {
        self.apply(QueueCommand::Retry { item_ids: ids }).await
    }
    pub async fn remove(&self, ids: Vec<String>) -> Result<(), RuntimeError> {
        self.apply(QueueCommand::Remove { item_ids: ids }).await
    }
    pub async fn reorder(&self, ids: Vec<String>) -> Result<(), RuntimeError> {
        self.apply(QueueCommand::Reorder { ordered_ids: ids }).await
    }
    pub async fn clear_completed(&self) -> Result<(), RuntimeError> {
        self.apply(QueueCommand::ClearCompleted).await
    }
    pub async fn pause_after_current(&self) -> Result<(), RuntimeError> {
        self.apply(QueueCommand::PauseAfterCurrent).await
    }

    pub async fn stop(&self) -> Result<(), RuntimeError> {
        let _control = self.inner.run_control.lock().await;
        let Some(active) = self.inner.active_run.lock().await.clone() else {
            return Ok(());
        };
        active.cancel.cancel();
        match self.apply(QueueCommand::CancelRun { run_id: active.run_id }).await {
            Ok(()) | Err(RuntimeError::Queue(QueueError::StaleRun)) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Force-abort the active run through the core reducer, even if its task handle is gone.
    pub async fn force_abort_active_run(
        &self,
        reason: impl Into<String>,
    ) -> Result<(), RuntimeError> {
        let reason = reason.into();
        let _control = self.inner.run_control.lock().await;
        if let Some(active) = self.inner.active_run.lock().await.clone() {
            active.cancel.cancel();
        }
        self.apply(QueueCommand::RecoverRun { reason }).await?;
        *self.inner.active_run.lock().await = None;
        Ok(())
    }

    /// Wait until the queue is Idle or `timeout` elapses (watch-based, no polling).
    pub async fn wait_until_idle(
        &self,
        timeout_duration: Duration,
    ) -> Result<(), WaitForIdleError> {
        let mut receiver = self.subscribe();
        if receiver.borrow().state.run_state == QueueRunState::Idle {
            return Ok(());
        }
        let wait = async {
            loop {
                receiver.changed().await.map_err(|_| WaitForIdleError::Closed)?;
                if receiver.borrow().state.run_state == QueueRunState::Idle {
                    return Ok(());
                }
            }
        };
        match tokio::time::timeout(timeout_duration, wait).await {
            Ok(result) => result,
            Err(_) => Err(WaitForIdleError::TimedOut),
        }
    }

    pub async fn start(&self, mut context: ExecutionContext) -> Result<(), RuntimeError> {
        let _control = self.inner.run_control.lock().await;
        if let Some(workdir) = self.inner.workdir.lock().await.clone() {
            context.paths = context.paths.for_workdir(&workdir);
            context.paths.ensure()?;
        }
        let profile = {
            let state = self.inner.state.lock().await;
            execution_profile(&state)?
        };
        self.ensure_background_tasks_started().await?;
        let run_id = uuid::Uuid::new_v4().to_string();
        self.apply(QueueCommand::StartRun { run_id: run_id.clone() }).await?;
        let cancel = CancellationToken::new();
        *self.inner.active_run.lock().await =
            Some(ActiveRun { run_id: run_id.clone(), cancel: cancel.clone() });
        let supervisor = self.clone();
        let cleanup_run_id = run_id.clone();
        tokio::spawn(async move {
            let result = supervisor.run_loop(context, run_id, cancel.clone(), profile).await;
            if let Err(error) = result {
                supervisor.inner.hub.emit("error", error.to_string());
                if let Err(abort_error) = supervisor
                    .apply(QueueCommand::AbortRun {
                        run_id: cleanup_run_id.clone(),
                        reason: error.to_string(),
                    })
                    .await
                {
                    supervisor.inner.hub.emit("error", abort_error.to_string());
                }
            }
            supervisor.clear_active_run(&cleanup_run_id).await;
        });
        Ok(())
    }

    async fn clear_active_run(&self, run_id: &str) {
        let _control = self.inner.run_control.lock().await;
        let mut active = self.inner.active_run.lock().await;
        if active.as_ref().is_some_and(|value| value.run_id == run_id) {
            *active = None;
        }
    }

    async fn run_loop(
        &self,
        context: ExecutionContext,
        run_id: String,
        cancel: CancellationToken,
        profile: QueueExecutionProfile,
    ) -> Result<(), RuntimeError> {
        if let QueueExecutionProfile::Parallel { backends } = profile {
            self.run_parallel_loop(context, run_id.clone(), cancel.clone(), backends).await?;
            self.finish_run(&run_id).await?;
            return Ok(());
        }
        let ids = {
            self.inner
                .state
                .lock()
                .await
                .items
                .iter()
                .filter(|item| item.status == vc_core::queue::QueueItemStatus::Queued)
                .map(|item| item.item_id.clone())
                .collect::<Vec<_>>()
        };
        for item_id in ids {
            if cancel.is_cancelled() {
                break;
            }
            if self.inner.state.lock().await.run_state == QueueRunState::PauseRequested {
                break;
            }
            let item = {
                self.inner
                    .state
                    .lock()
                    .await
                    .items
                    .iter()
                    .find(|item| item.item_id == item_id)
                    .map(|item| item.plan.clone())
            };
            let Some(item) = item else {
                continue;
            };
            self.apply(QueueCommand::StartItem {
                item_id: item_id.clone(),
                run_id: run_id.clone(),
            })
            .await?;
            let sink = self.progress_sink(item_id.clone(), run_id.clone());
            let result = execute_item(
                &item,
                &context.tools.ffmpeg,
                &context.paths,
                &context.activity,
                cancel.clone(),
                Some(sink),
                1,
                1,
                Some(item_id.clone()),
            )
            .await;
            match result {
                Ok(value) => {
                    let command_result = QueueCommand::Finish {
                        item_id: item_id.clone(),
                        run_id: run_id.clone(),
                        result: value.item_result.clone(),
                    };
                    self.apply(command_result).await?;
                }
                Err(RuntimeError::Cancelled) => {
                    let _ = self
                        .apply(QueueCommand::Cancel {
                            item_id: item_id.clone(),
                            run_id: run_id.clone(),
                            reason: "Operation cancelled.".into(),
                        })
                        .await;
                    break;
                }
                Err(error) => {
                    let _ = self
                        .apply(QueueCommand::Fail {
                            item_id: item_id.clone(),
                            run_id: run_id.clone(),
                            error: vc_core::queue::JobError { message: error.to_string() },
                        })
                        .await;
                }
            }
        }
        self.finish_run(&run_id).await?;
        Ok(())
    }

    async fn finish_run(&self, run_id: &str) -> Result<(), RuntimeError> {
        if self.inner.state.lock().await.run_state == QueueRunState::PauseRequested {
            self.apply(QueueCommand::PauseComplete { run_id: run_id.into() }).await
        } else {
            self.apply(QueueCommand::RunIdle { run_id: run_id.into() }).await
        }
    }

    async fn run_parallel_loop(
        &self,
        context: ExecutionContext,
        run_id: String,
        cancel: CancellationToken,
        backends: Vec<vc_core::EncoderBackend>,
    ) -> Result<(), RuntimeError> {
        let ids = self
            .inner
            .state
            .lock()
            .await
            .items
            .iter()
            .filter(|item| item.status == vc_core::queue::QueueItemStatus::Queued)
            .map(|item| item.item_id.clone())
            .collect::<Vec<_>>();
        let capabilities = ensure_capabilities(&context.paths, &context.tools, false).await?;
        let pending = Arc::new(Mutex::new(VecDeque::from(ids)));
        let mut workers = Vec::with_capacity(backends.len());
        for backend in backends {
            let pending = pending.clone();
            let supervisor = self.clone();
            let context = context.clone();
            let capabilities = capabilities.clone();
            let worker_cancel = cancel.child_token();
            let queue_cancel = cancel.clone();
            let run_id = run_id.clone();
            workers.push(tokio::spawn(async move {
                loop {
                    if queue_cancel.is_cancelled() || worker_cancel.is_cancelled() {
                        return Ok::<(), RuntimeError>(());
                    }
                    if supervisor.inner.state.lock().await.run_state
                        == QueueRunState::PauseRequested
                    {
                        return Ok(());
                    }
                    let item_id = { pending.lock().await.pop_front() };
                    let Some(item_id) = item_id else {
                        return Ok(());
                    };
                    if supervisor.inner.state.lock().await.run_state
                        == QueueRunState::PauseRequested
                    {
                        pending.lock().await.push_front(item_id);
                        return Ok(());
                    }
                    let item = {
                        supervisor
                            .inner
                            .state
                            .lock()
                            .await
                            .items
                            .iter()
                            .find(|item| item.item_id == item_id)
                            .map(|item| item.plan.clone())
                    };
                    let Some(mut item) = item else {
                        continue;
                    };
                    supervisor
                        .apply(QueueCommand::StartItem {
                            item_id: item_id.clone(),
                            run_id: run_id.clone(),
                        })
                        .await?;
                    if item.skip_reason.is_none() {
                        let selection = match vc_core::planning::resolve_encoder(
                            item.settings.codec,
                            backend,
                            &capabilities,
                        ) {
                            Ok(value) => value,
                            Err(error) => {
                                let _ = supervisor
                                    .apply(QueueCommand::Fail {
                                        item_id: item_id.clone(),
                                        run_id: run_id.clone(),
                                        error: vc_core::queue::JobError { message: error },
                                    })
                                    .await;
                                return Ok(());
                            }
                        };
                        item.encoder = Some(selection.clone());
                        item.settings.backend = backend;
                        item.settings.parallel_enabled = false;
                        item.settings.encoder_preset = selection.default_preset.clone();
                    }
                    let sink = supervisor.progress_sink(item_id.clone(), run_id.clone());
                    match execute_item(
                        &item,
                        &context.tools.ffmpeg,
                        &context.paths,
                        &context.activity,
                        worker_cancel.clone(),
                        Some(sink),
                        1,
                        1,
                        Some(item_id.clone()),
                    )
                    .await
                    {
                        Ok(result) => {
                            supervisor
                                .apply(QueueCommand::Finish {
                                    item_id,
                                    run_id: run_id.clone(),
                                    result: result.item_result,
                                })
                                .await?
                        }
                        Err(RuntimeError::Cancelled) => {
                            let _ = supervisor
                                .apply(QueueCommand::Cancel {
                                    item_id,
                                    run_id: run_id.clone(),
                                    reason: "Operation cancelled.".into(),
                                })
                                .await;
                            return Ok(());
                        }
                        Err(error) => {
                            let _ = supervisor
                                .apply(QueueCommand::Fail {
                                    item_id,
                                    run_id: run_id.clone(),
                                    error: vc_core::queue::JobError { message: error.to_string() },
                                })
                                .await;
                            continue;
                        }
                    }
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
                        first_error =
                            Some(RuntimeError::Queue(vc_core::queue::QueueError::Illegal {
                                item_id: String::new(),
                                message: error.to_string(),
                            }));
                    }
                    cancel.cancel();
                }
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }

    async fn apply(&self, command: QueueCommand) -> Result<(), RuntimeError> {
        let mode = publish_mode(&command);
        self.apply_with_mode(command, mode).await
    }

    /// Apply a queue command (used by stress tests and specialized adapters).
    pub async fn apply_command(&self, command: QueueCommand) -> Result<(), RuntimeError> {
        self.apply(command).await
    }

    async fn apply_with_mode(
        &self,
        command: QueueCommand,
        mode: SnapshotPublishMode,
    ) -> Result<(), RuntimeError> {
        {
            let mut state = self.inner.state.lock().await;
            apply(&mut state, command)?;
        }
        match mode {
            SnapshotPublishMode::Immediate => {
                // Clear dirty so a pending coalesced publish does not re-send stale mid-window.
                self.inner.snapshot_dirty.store(false, Ordering::SeqCst);
                self.publish_snapshot_from_state().await;
            }
            SnapshotPublishMode::Coalesced => {
                self.inner.snapshot_dirty.store(true, Ordering::SeqCst);
                self.inner.snapshot_notify.notify_one();
            }
        }
        Ok(())
    }

    async fn publish_snapshot_from_state(&self) {
        let snapshot = {
            let state = self.inner.state.lock().await;
            self.inner.metrics_compute_count.fetch_add(1, Ordering::SeqCst);
            Arc::new(QueueSnapshot { metrics: compute_metrics(&state), state: state.clone() })
        };
        self.inner.snapshot_publish_count.fetch_add(1, Ordering::SeqCst);
        self.inner.snapshots.send_replace(snapshot);
    }
}

impl Drop for QueueSupervisorInner {
    fn drop(&mut self) {
        self.background_cancel.cancel();
    }
}

async fn run_progress_worker(
    weak: Weak<QueueSupervisorInner>,
    context: BackgroundContext,
    mut progress_rx: mpsc::Receiver<ProgressUpdate>,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            update = progress_rx.recv() => {
                let Some(update) = update else { break; };
                if weak.upgrade().is_none() {
                    break;
                }
                let command = QueueCommand::ReportProgress {
                    item_id: update.item_id,
                    run_id: update.run_id,
                    progress: update.progress,
                };
                let _ = apply_background_command(&context, command).await;
            }
        }
    }
}

async fn run_snapshot_publisher(
    weak: Weak<QueueSupervisorInner>,
    context: BackgroundContext,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = context.snapshot_notify.notified() => {
                if weak.upgrade().is_none() {
                    break;
                }
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(SNAPSHOT_COALESCE_INTERVAL) => {}
                }
                if !context.snapshot_dirty.swap(false, Ordering::SeqCst) {
                    continue;
                }
                publish_snapshot(&context).await;
            }
        }
    }
}

async fn apply_background_command(
    context: &BackgroundContext,
    command: QueueCommand,
) -> Result<(), RuntimeError> {
    {
        let mut state = context.state.lock().await;
        apply(&mut state, command)?;
    }
    context.snapshot_dirty.store(true, Ordering::SeqCst);
    context.snapshot_notify.notify_one();
    Ok(())
}

async fn publish_snapshot(context: &BackgroundContext) {
    let snapshot = {
        let state = context.state.lock().await;
        context.metrics_compute_count.fetch_add(1, Ordering::SeqCst);
        Arc::new(QueueSnapshot { metrics: compute_metrics(&state), state: state.clone() })
    };
    context.snapshot_publish_count.fetch_add(1, Ordering::SeqCst);
    context.snapshots.send_replace(snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;
    use vc_core::queue::QueueItemStatus;
    use vc_core::{EncodePlanItem, EncodeSettings, EncoderBackend};

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
    fn queue_supervisor_construction_does_not_require_tokio_runtime() {
        let supervisor = QueueSupervisor::new(ActivityHub::new());

        assert_eq!(supervisor.progress_worker_spawns(), 0);
        assert_eq!(supervisor.snapshot_worker_spawns(), 0);
    }

    #[tokio::test]
    async fn background_workers_start_once_and_shutdown_idempotently() {
        let supervisor = QueueSupervisor::new(ActivityHub::new());
        supervisor.initialize().await.expect("initialize");
        supervisor.initialize().await.expect("second initialize");
        assert_eq!(supervisor.progress_worker_spawns(), 1);
        assert_eq!(supervisor.snapshot_worker_spawns(), 1);
        supervisor.shutdown().await;
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn old_run_cleanup_does_not_clear_new_run_cancel_token() {
        let supervisor = QueueSupervisor::new(ActivityHub::new());
        let run_two_cancel = CancellationToken::new();
        *supervisor.inner.active_run.lock().await =
            Some(ActiveRun { run_id: "run-1".into(), cancel: CancellationToken::new() });

        let control = supervisor.inner.run_control.lock().await;
        let cleanup = {
            let supervisor = supervisor.clone();
            tokio::spawn(async move { supervisor.clear_active_run("run-1").await })
        };
        *supervisor.inner.active_run.lock().await =
            Some(ActiveRun { run_id: "run-2".into(), cancel: run_two_cancel.clone() });
        drop(control);
        assert!(cleanup.await.is_ok(), "cleanup task panicked");

        let active = match supervisor.inner.active_run.lock().await.clone() {
            Some(active) => active,
            None => panic!("new active run was cleared"),
        };
        assert_eq!(active.run_id, "run-2");
        assert!(!active.cancel.is_cancelled());
    }

    #[tokio::test(start_paused = true)]
    async fn progress_burst_is_coalesced() {
        let supervisor = QueueSupervisor::new(ActivityHub::new());
        supervisor.initialize().await.expect("initialize");
        supervisor.enqueue(vec![plan_item("a")]).await.expect("enqueue");
        let item_id = supervisor.snapshot_now().state.items[0].item_id.clone();
        let run_id = "run-test".to_owned();
        supervisor
            .apply(QueueCommand::StartRun { run_id: run_id.clone() })
            .await
            .expect("start run");
        supervisor
            .apply(QueueCommand::StartItem { item_id: item_id.clone(), run_id: run_id.clone() })
            .await
            .expect("start item");
        let before = supervisor.snapshot_publish_count();
        for percent in 0..100 {
            supervisor.report_progress(
                ProgressEvent {
                    item_id: Some(item_id.clone()),
                    stage: "encode".into(),
                    state: "running".into(),
                    percent: Some(percent as f64),
                    speed: Some("2.0x".into()),
                    elapsed_sec: Some(percent as f64),
                    current_pass: 1,
                    total_passes: 1,
                    message: None,
                },
                &item_id,
                &run_id,
            );
        }
        // Drain progress worker.
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        tokio::time::advance(SNAPSHOT_COALESCE_INTERVAL + Duration::from_millis(10)).await;
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        let published = supervisor.snapshot_publish_count() - before;
        // One coalesce window after structural starts already published immediately.
        assert!(published <= 3, "expected coalesced publishes, got {published}");
        assert!(published >= 1);
    }

    #[tokio::test(start_paused = true)]
    async fn progress_snapshot_rate_is_bounded() {
        let supervisor = QueueSupervisor::new(ActivityHub::new());
        supervisor.initialize().await.expect("initialize");
        supervisor.enqueue(vec![plan_item("a")]).await.expect("enqueue");
        let item_id = supervisor.snapshot_now().state.items[0].item_id.clone();
        let run_id = "run-rate".to_owned();
        supervisor.apply(QueueCommand::StartRun { run_id: run_id.clone() }).await.unwrap();
        supervisor
            .apply(QueueCommand::StartItem { item_id: item_id.clone(), run_id: run_id.clone() })
            .await
            .unwrap();
        let before = supervisor.snapshot_publish_count();
        for tick in 0..20 {
            for _ in 0..50 {
                supervisor.report_progress(
                    ProgressEvent {
                        item_id: Some(item_id.clone()),
                        stage: "encode".into(),
                        state: "running".into(),
                        percent: Some((tick as f64) * 5.0),
                        speed: None,
                        elapsed_sec: None,
                        current_pass: 1,
                        total_passes: 1,
                        message: None,
                    },
                    &item_id,
                    &run_id,
                );
            }
            tokio::time::advance(SNAPSHOT_COALESCE_INTERVAL).await;
            for _ in 0..10 {
                tokio::task::yield_now().await;
            }
        }
        let published = supervisor.snapshot_publish_count() - before;
        // ~20 windows over 4 seconds of virtual time; allow some headroom.
        assert!(published <= 30, "publish rate unbounded: {published}");
    }

    #[tokio::test]
    async fn structural_commands_publish_immediately() {
        let supervisor = QueueSupervisor::new(ActivityHub::new());
        let before = supervisor.snapshot_publish_count();
        supervisor.enqueue(vec![plan_item("a")]).await.unwrap();
        assert_eq!(supervisor.snapshot_publish_count(), before + 1);
        assert_eq!(supervisor.snapshot_now().state.items.len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn final_progress_is_not_lost() {
        let supervisor = QueueSupervisor::new(ActivityHub::new());
        supervisor.initialize().await.expect("initialize");
        supervisor.enqueue(vec![plan_item("a")]).await.unwrap();
        let item_id = supervisor.snapshot_now().state.items[0].item_id.clone();
        let run_id = "run-final".to_owned();
        supervisor.apply(QueueCommand::StartRun { run_id: run_id.clone() }).await.unwrap();
        supervisor
            .apply(QueueCommand::StartItem { item_id: item_id.clone(), run_id: run_id.clone() })
            .await
            .unwrap();
        for percent in [10.0, 50.0, 99.0, 100.0] {
            supervisor.report_progress(
                ProgressEvent {
                    item_id: Some(item_id.clone()),
                    stage: "encode".into(),
                    state: "running".into(),
                    percent: Some(percent),
                    speed: None,
                    elapsed_sec: None,
                    current_pass: 1,
                    total_passes: 1,
                    message: None,
                },
                &item_id,
                &run_id,
            );
        }
        for _ in 0..30 {
            tokio::task::yield_now().await;
        }
        // Immediate Finish must publish final state including last progress.
        supervisor
            .apply(QueueCommand::Finish {
                item_id: item_id.clone(),
                run_id: run_id.clone(),
                result: vc_core::queue::ItemResult {
                    success: true,
                    skipped: false,
                    return_code: Some(0),
                    output_path: None,
                    log_path: None,
                    error: None,
                },
            })
            .await
            .unwrap();
        let snap = supervisor.snapshot_now();
        assert_eq!(snap.state.items[0].status, QueueItemStatus::Done);
    }

    #[tokio::test]
    async fn thousand_progress_events_do_not_spawn_thousand_tasks() {
        let supervisor = QueueSupervisor::new(ActivityHub::new());
        supervisor.initialize().await.expect("initialize");
        // Only the single long-lived progress worker is spawned.
        assert_eq!(supervisor.progress_worker_spawns(), 1);
        supervisor.enqueue(vec![plan_item("a")]).await.unwrap();
        let item_id = supervisor.snapshot_now().state.items[0].item_id.clone();
        for percent in 0..1_000 {
            supervisor.report_progress(
                ProgressEvent {
                    item_id: Some(item_id.clone()),
                    stage: "encode".into(),
                    state: "running".into(),
                    percent: Some(percent as f64 / 10.0),
                    speed: None,
                    elapsed_sec: None,
                    current_pass: 1,
                    total_passes: 1,
                    message: None,
                },
                &item_id,
                "run",
            );
        }
        assert_eq!(supervisor.progress_worker_spawns(), 1);
    }

    #[tokio::test]
    async fn multiple_parallel_workers_share_one_publisher() {
        let supervisor = QueueSupervisor::new(ActivityHub::new());
        supervisor.initialize().await.expect("initialize");
        assert_eq!(supervisor.progress_worker_spawns(), 1);
        let sink_a = supervisor.progress_sink("a".into(), "run".into());
        let sink_b = supervisor.progress_sink("b".into(), "run".into());
        sink_a(ProgressEvent {
            item_id: Some("a".into()),
            stage: "encode".into(),
            state: "running".into(),
            percent: Some(1.0),
            speed: None,
            elapsed_sec: None,
            current_pass: 1,
            total_passes: 1,
            message: None,
        });
        sink_b(ProgressEvent {
            item_id: Some("b".into()),
            stage: "encode".into(),
            state: "running".into(),
            percent: Some(2.0),
            speed: None,
            elapsed_sec: None,
            current_pass: 1,
            total_passes: 1,
            message: None,
        });
        assert_eq!(supervisor.progress_worker_spawns(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn close_timeout_never_waits_forever() {
        let supervisor = QueueSupervisor::new(ActivityHub::new());
        supervisor.enqueue(vec![plan_item("a")]).await.unwrap();
        supervisor.apply(QueueCommand::StartRun { run_id: "stuck".into() }).await.unwrap();
        // Leave run in Running without a finish path.
        let wait = {
            let supervisor = supervisor.clone();
            tokio::spawn(async move { supervisor.wait_until_idle(Duration::from_secs(2)).await })
        };
        tokio::time::advance(Duration::from_secs(3)).await;
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        let result = wait.await.expect("join");
        assert_eq!(result, Err(WaitForIdleError::TimedOut));
    }

    #[tokio::test]
    async fn close_timeout_forces_abort() {
        let supervisor = QueueSupervisor::new(ActivityHub::new());
        supervisor.enqueue(vec![plan_item("a")]).await.unwrap();
        let item_id = supervisor.snapshot_now().state.items[0].item_id.clone();
        supervisor.apply(QueueCommand::StartRun { run_id: "r".into() }).await.unwrap();
        supervisor
            .apply(QueueCommand::StartItem { item_id: item_id.clone(), run_id: "r".into() })
            .await
            .unwrap();
        *supervisor.inner.active_run.lock().await =
            Some(ActiveRun { run_id: "r".into(), cancel: CancellationToken::new() });
        supervisor.force_abort_active_run("close timeout").await.unwrap();
        let snapshot = supervisor.snapshot_now();
        assert_eq!(snapshot.state.run_state, QueueRunState::Idle);
        assert_eq!(snapshot.state.items[0].status, QueueItemStatus::Cancelled);
        assert_eq!(snapshot.state.items[0].run_id, None);
        vc_core::queue::validate_queue_state(&snapshot.state).expect("valid recovered state");
    }

    #[tokio::test]
    async fn force_abort_without_active_handle_returns_valid_idle_state() {
        let supervisor = QueueSupervisor::new(ActivityHub::new());
        supervisor.enqueue(vec![plan_item("a"), plan_item("b")]).await.unwrap();
        let item_id = supervisor.snapshot_now().state.items[0].item_id.clone();
        supervisor.apply(QueueCommand::StartRun { run_id: "lost".into() }).await.unwrap();
        supervisor.apply(QueueCommand::StartItem { item_id, run_id: "lost".into() }).await.unwrap();
        supervisor.force_abort_active_run("missing run task").await.unwrap();
        let snapshot = supervisor.snapshot_now();
        vc_core::queue::validate_queue_state(&snapshot.state).expect("valid recovered state");
        assert_eq!(snapshot.state.run_state, QueueRunState::Idle);
        assert_eq!(snapshot.state.items[0].status, QueueItemStatus::Cancelled);
        assert_eq!(snapshot.state.items[1].status, QueueItemStatus::Queued);
        assert!(snapshot.state.items.iter().all(|item| item.run_id.is_none()));
    }

    #[tokio::test]
    async fn snapshot_now_is_sync() {
        let supervisor = QueueSupervisor::new(ActivityHub::new());
        let snap = supervisor.snapshot_now();
        assert_eq!(snap.state.run_state, QueueRunState::Idle);
    }
}
