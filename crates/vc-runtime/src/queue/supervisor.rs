use crate::activity::ActivityHub;
use crate::error::RuntimeError;
use crate::execution::{ProgressEvent, ProgressSink, execute_item};
use crate::ffmpeg::{ToolPaths, capabilities::ensure_capabilities};
use crate::platform::paths::AppPaths;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;
use vc_core::EncodePlanItem;
use vc_core::queue::{
    ItemProgress, QueueCommand, QueueError, QueueExecutionProfile, QueueMetrics, QueueState, apply,
    compute_metrics, execution_profile,
};

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

#[derive(Clone)]
pub struct QueueSupervisor {
    state: Arc<Mutex<QueueState>>,
    snapshots: watch::Sender<Arc<QueueSnapshot>>,
    hub: ActivityHub,
    active_run: Arc<Mutex<Option<ActiveRun>>>,
    run_control: Arc<Mutex<()>>,
    workdir: Arc<Mutex<Option<PathBuf>>>,
}

impl QueueSupervisor {
    pub fn new(hub: ActivityHub) -> Self {
        let state = QueueState::default();
        let snapshot = Arc::new(QueueSnapshot { state, metrics: QueueMetrics::default() });
        let (snapshots, _) = watch::channel(snapshot);
        Self {
            state: Arc::new(Mutex::new(QueueState::default())),
            snapshots,
            hub,
            active_run: Arc::new(Mutex::new(None)),
            run_control: Arc::new(Mutex::new(())),
            workdir: Arc::new(Mutex::new(None)),
        }
    }
    pub fn subscribe(&self) -> watch::Receiver<Arc<QueueSnapshot>> {
        self.snapshots.subscribe()
    }
    pub async fn snapshot(&self) -> Arc<QueueSnapshot> {
        self.snapshots.borrow().clone()
    }
    pub async fn enqueue(&self, plans: Vec<EncodePlanItem>) -> Result<(), RuntimeError> {
        self.apply(QueueCommand::Enqueue(plans)).await
    }
    pub async fn set_workdir(&self, workdir: PathBuf) {
        *self.workdir.lock().await = Some(workdir);
    }
    pub async fn set_default_workdir(&self, workdir: PathBuf) {
        let mut value = self.workdir.lock().await;
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
        let _control = self.run_control.lock().await;
        let Some(active) = self.active_run.lock().await.clone() else {
            return Ok(());
        };
        active.cancel.cancel();
        match self.apply(QueueCommand::CancelRun { run_id: active.run_id }).await {
            Ok(()) | Err(RuntimeError::Queue(QueueError::StaleRun)) => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub async fn start(&self, mut context: ExecutionContext) -> Result<(), RuntimeError> {
        let _control = self.run_control.lock().await;
        if let Some(workdir) = self.workdir.lock().await.clone() {
            context.paths = context.paths.for_workdir(&workdir);
            context.paths.ensure()?;
        }
        let profile = {
            let state = self.state.lock().await;
            execution_profile(&state)?
        };
        let run_id = uuid::Uuid::new_v4().to_string();
        self.apply(QueueCommand::StartRun { run_id: run_id.clone() }).await?;
        let cancel = CancellationToken::new();
        *self.active_run.lock().await =
            Some(ActiveRun { run_id: run_id.clone(), cancel: cancel.clone() });
        let supervisor = self.clone();
        let cleanup_run_id = run_id.clone();
        tokio::spawn(async move {
            let result = supervisor.run_loop(context, run_id, cancel.clone(), profile).await;
            if let Err(error) = result {
                supervisor.hub.emit("error", error.to_string());
                if let Err(abort_error) = supervisor
                    .apply(QueueCommand::AbortRun {
                        run_id: cleanup_run_id.clone(),
                        reason: error.to_string(),
                    })
                    .await
                {
                    supervisor.hub.emit("error", abort_error.to_string());
                }
            }
            supervisor.clear_active_run(&cleanup_run_id).await;
        });
        Ok(())
    }

    async fn clear_active_run(&self, run_id: &str) {
        let _control = self.run_control.lock().await;
        let mut active = self.active_run.lock().await;
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
            self.state
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
            if self.state.lock().await.run_state == vc_core::queue::QueueRunState::PauseRequested {
                break;
            }
            let item = {
                self.state
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
            let item_id_for_sink = item_id.clone();
            let run_for_sink = run_id.clone();
            let queue_for_sink = self.clone();
            let sink: ProgressSink = Arc::new(move |event: ProgressEvent| {
                let queue = queue_for_sink.clone();
                let item_id = event.item_id.clone().unwrap_or_else(|| item_id_for_sink.clone());
                let run_id = run_for_sink.clone();
                tokio::spawn(async move {
                    let _ = queue
                        .apply(QueueCommand::ReportProgress {
                            item_id,
                            run_id,
                            progress: ItemProgress {
                                percent: event.percent.unwrap_or(0.0),
                                speed: event.speed,
                                elapsed_sec: event.elapsed_sec,
                                current_pass: event.current_pass,
                                total_passes: event.total_passes,
                            },
                        })
                        .await;
                });
            });
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
        if self.state.lock().await.run_state == vc_core::queue::QueueRunState::PauseRequested {
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
                    if supervisor.state.lock().await.run_state
                        == vc_core::queue::QueueRunState::PauseRequested
                    {
                        return Ok(());
                    }
                    let item_id = { pending.lock().await.pop_front() };
                    let Some(item_id) = item_id else {
                        return Ok(());
                    };
                    if supervisor.state.lock().await.run_state
                        == vc_core::queue::QueueRunState::PauseRequested
                    {
                        pending.lock().await.push_front(item_id);
                        return Ok(());
                    }
                    let item = {
                        supervisor
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
                    let item_id_for_sink = item_id.clone();
                    let run_for_sink = run_id.clone();
                    let queue_for_sink = supervisor.clone();
                    let sink: ProgressSink = Arc::new(move |event: ProgressEvent| {
                        let queue = queue_for_sink.clone();
                        let item_id =
                            event.item_id.clone().unwrap_or_else(|| item_id_for_sink.clone());
                        let run_id = run_for_sink.clone();
                        tokio::spawn(async move {
                            let _ = queue
                                .apply(QueueCommand::ReportProgress {
                                    item_id,
                                    run_id,
                                    progress: ItemProgress {
                                        percent: event.percent.unwrap_or(0.0),
                                        speed: event.speed,
                                        elapsed_sec: event.elapsed_sec,
                                        current_pass: event.current_pass,
                                        total_passes: event.total_passes,
                                    },
                                })
                                .await;
                        });
                    });
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
        let snapshot = {
            let mut state = self.state.lock().await;
            apply(&mut state, command)?;
            Arc::new(QueueSnapshot { metrics: compute_metrics(&state), state: state.clone() })
        };
        self.snapshots.send_replace(snapshot);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn old_run_cleanup_does_not_clear_new_run_cancel_token() {
        let supervisor = QueueSupervisor::new(ActivityHub::new());
        let run_two_cancel = CancellationToken::new();
        *supervisor.active_run.lock().await =
            Some(ActiveRun { run_id: "run-1".into(), cancel: CancellationToken::new() });

        // Hold the run-control lock so stale cleanup cannot pass the same
        // serialization point as a new run. Install the replacement handle
        // before releasing it, then let the old cleanup continue.
        let control = supervisor.run_control.lock().await;
        let cleanup = {
            let supervisor = supervisor.clone();
            tokio::spawn(async move { supervisor.clear_active_run("run-1").await })
        };
        *supervisor.active_run.lock().await =
            Some(ActiveRun { run_id: "run-2".into(), cancel: run_two_cancel.clone() });
        drop(control);
        cleanup.await.expect("cleanup task");

        let active = supervisor.active_run.lock().await.clone().expect("new active run");
        assert_eq!(active.run_id, "run-2");
        assert!(!active.cancel.is_cancelled());
    }
}
