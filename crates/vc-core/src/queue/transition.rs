use super::{
    ItemProgress, ItemResult, JobError, QueueExecutionProfile, QueueItem, QueueItemStatus,
    QueueRunState, QueueState,
};
use crate::model::EncodePlanItem;
use crate::model::EncoderBackend;
use crate::planning::unique_parallel_backends;
use std::collections::HashSet;
use thiserror::Error;

#[derive(Clone, Debug)]
pub enum QueueCommand {
    Enqueue(Vec<EncodePlanItem>),
    StartRun { run_id: String },
    StartItem { item_id: String, run_id: String },
    ReportProgress { item_id: String, run_id: String, progress: ItemProgress },
    Finish { item_id: String, run_id: String, result: ItemResult },
    Fail { item_id: String, run_id: String, error: JobError },
    Cancel { item_id: String, run_id: String, reason: String },
    PauseAfterCurrent,
    PauseComplete { run_id: String },
    CancelRun { run_id: String },
    AbortRun { run_id: String, reason: String },
    RecoverRun { reason: String },
    RunIdle { run_id: String },
    Retry { item_ids: Vec<String> },
    Remove { item_ids: Vec<String> },
    Reorder { ordered_ids: Vec<String> },
    ClearCompleted,
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum QueueError {
    #[error("queue is busy")]
    Busy,
    #[error("queue item was not found: {0}")]
    ItemNotFound(String),
    #[error("illegal queue transition for {item_id}: {message}")]
    Illegal { item_id: String, message: String },
    #[error("stale queue run event was ignored")]
    StaleRun,
    #[error("reorder list does not match queued items")]
    InvalidOrder,
    #[error("incompatible queued execution profiles: {0}")]
    IncompatibleExecutionProfile(String),
    #[error("queue state invariant violated: {0}")]
    Invariant(String),
}

fn item_mut<'a>(state: &'a mut QueueState, id: &str) -> Result<&'a mut QueueItem, QueueError> {
    state
        .items
        .iter_mut()
        .find(|item| item.item_id == id)
        .ok_or_else(|| QueueError::ItemNotFound(id.into()))
}

fn check_run(item: &QueueItem, run_id: &str) -> Result<(), QueueError> {
    if item.run_id.as_deref() != Some(run_id) {
        return Err(QueueError::StaleRun);
    }
    Ok(())
}

fn check_active_run(state: &QueueState, run_id: &str) -> Result<(), QueueError> {
    if run_id.is_empty() || state.active_run_id.as_deref() != Some(run_id) {
        return Err(QueueError::StaleRun);
    }
    Ok(())
}

fn item_sequence(item_id: &str) -> Option<u64> {
    item_id.strip_prefix("item-")?.split('-').next()?.parse().ok()
}

fn max_item_sequence(state: &QueueState) -> u64 {
    state.items.iter().filter_map(|item| item_sequence(&item.item_id)).max().unwrap_or(0)
}

fn profile_for_item(item: &QueueItem) -> Result<QueueExecutionProfile, QueueError> {
    if !item.plan.settings.parallel_enabled {
        return Ok(QueueExecutionProfile::Serial);
    }
    let backends = unique_parallel_backends(&item.plan.settings.parallel_backends)
        .into_iter()
        .filter(|backend| *backend != EncoderBackend::Auto)
        .collect::<Vec<_>>();
    if backends.is_empty() {
        return Err(QueueError::IncompatibleExecutionProfile(
            "Parallel mode requires at least one explicit backend.".into(),
        ));
    }
    Ok(QueueExecutionProfile::Parallel { backends })
}

pub fn execution_profile(state: &QueueState) -> Result<QueueExecutionProfile, QueueError> {
    let mut queued = state.items.iter().filter(|item| item.status == QueueItemStatus::Queued);
    let Some(first) = queued.next() else {
        return Err(QueueError::Illegal {
            item_id: String::new(),
            message: "no queued items".into(),
        });
    };
    let profile = profile_for_item(first)?;
    for item in queued {
        if profile_for_item(item)? != profile {
            return Err(QueueError::IncompatibleExecutionProfile(
                "Queued items use incompatible execution modes. Remove or reconfigure items before starting.".into(),
            ));
        }
    }
    Ok(profile)
}

pub fn validate_queue_state(state: &QueueState) -> Result<(), QueueError> {
    let mut ids = HashSet::with_capacity(state.items.len());
    for item in &state.items {
        if !ids.insert(&item.item_id) {
            return Err(QueueError::Invariant(format!("duplicate item id: {}", item.item_id)));
        }
    }

    let active_required = matches!(
        state.run_state,
        QueueRunState::Running | QueueRunState::PauseRequested | QueueRunState::Cancelling
    );
    if active_required != state.active_run_id.is_some() {
        return Err(QueueError::Invariant(
            "active_run_id must exist exactly while the queue is active".into(),
        ));
    }
    if matches!(state.run_state, QueueRunState::Idle | QueueRunState::Paused)
        && state.items.iter().any(|item| item.status == QueueItemStatus::Running)
    {
        return Err(QueueError::Invariant("idle or paused queue contains a running item".into()));
    }

    for item in &state.items {
        if item.status == QueueItemStatus::Running {
            if item.run_id.as_deref() != state.active_run_id.as_deref() {
                return Err(QueueError::Invariant(format!(
                    "running item {} does not belong to the active run",
                    item.item_id
                )));
            }
        } else if item.run_id.is_some() {
            return Err(QueueError::Invariant(format!(
                "non-running item {} retains a run id",
                item.item_id
            )));
        }
        if matches!(item.status, QueueItemStatus::Done | QueueItemStatus::Skipped)
            && item.progress.percent != 100.0
        {
            return Err(QueueError::Invariant(format!(
                "completed item {} is not at 100 percent",
                item.item_id
            )));
        }
        if matches!(item.status, QueueItemStatus::Failed | QueueItemStatus::Cancelled)
            && item.error.is_none()
        {
            return Err(QueueError::Invariant(format!(
                "failed or cancelled item {} has no error",
                item.item_id
            )));
        }
    }
    Ok(())
}

pub fn apply(state: &mut QueueState, command: QueueCommand) -> Result<(), QueueError> {
    let is_progress = matches!(&command, QueueCommand::ReportProgress { .. });
    if is_progress {
        apply_unchecked(state, command)?;
        return validate_queue_state(state);
    }
    let mut next = state.clone();
    apply_unchecked(&mut next, command)?;
    validate_queue_state(&next)?;
    *state = next;
    Ok(())
}

fn apply_unchecked(state: &mut QueueState, command: QueueCommand) -> Result<(), QueueError> {
    match command {
        QueueCommand::Enqueue(plans) => {
            if !matches!(state.run_state, QueueRunState::Idle | QueueRunState::Paused) {
                return Err(QueueError::Busy);
            }
            let mut sequence = state.next_item_sequence.max(max_item_sequence(state));
            let mut ids =
                state.items.iter().map(|item| item.item_id.clone()).collect::<HashSet<_>>();
            let mut new_items = Vec::with_capacity(plans.len());
            for plan in plans {
                sequence = sequence.checked_add(1).ok_or_else(|| QueueError::Illegal {
                    item_id: String::new(),
                    message: "queue item sequence overflow".into(),
                })?;
                let item_id = loop {
                    let candidate = format!("item-{sequence}");
                    if ids.insert(candidate.clone()) {
                        break candidate;
                    }
                    sequence = sequence.checked_add(1).ok_or_else(|| QueueError::Illegal {
                        item_id: String::new(),
                        message: "queue item sequence overflow".into(),
                    })?;
                };
                new_items.push(QueueItem {
                    item_id,
                    status: if plan.skip_reason.is_some() {
                        QueueItemStatus::Skipped
                    } else {
                        QueueItemStatus::Queued
                    },
                    progress: ItemProgress {
                        percent: if plan.skip_reason.is_some() { 100.0 } else { 0.0 },
                        ..ItemProgress::default()
                    },
                    error: plan
                        .skip_reason
                        .as_ref()
                        .map(|value| JobError { message: value.0.clone() }),
                    result: None,
                    run_id: None,
                    plan,
                });
            }
            state.next_item_sequence = sequence;
            state.items.extend(new_items);
        }
        QueueCommand::StartRun { run_id } => {
            if run_id.is_empty() {
                return Err(QueueError::StaleRun);
            }
            if !matches!(state.run_state, QueueRunState::Idle | QueueRunState::Paused) {
                return Err(QueueError::Busy);
            }
            if !state.items.iter().any(|item| item.status == QueueItemStatus::Queued) {
                return Err(QueueError::Illegal {
                    item_id: String::new(),
                    message: "no queued items".into(),
                });
            }
            execution_profile(state)?;
            state.run_state = QueueRunState::Running;
            state.active_run_id = Some(run_id);
        }
        QueueCommand::StartItem { item_id, run_id } => {
            check_active_run(state, &run_id)?;
            let item = item_mut(state, &item_id)?;
            if item.status != QueueItemStatus::Queued {
                return Err(QueueError::Illegal {
                    item_id,
                    message: "only queued items can start".into(),
                });
            }
            item.status = QueueItemStatus::Running;
            item.run_id = Some(run_id);
            item.error = None;
            item.result = None;
            item.progress = ItemProgress::default();
        }
        QueueCommand::ReportProgress { item_id, run_id, progress } => {
            check_active_run(state, &run_id)?;
            let item = item_mut(state, &item_id)?;
            check_run(item, &run_id)?;
            if item.status != QueueItemStatus::Running {
                return Err(QueueError::Illegal {
                    item_id,
                    message: "progress requires a running item".into(),
                });
            }
            item.progress =
                ItemProgress { percent: progress.percent.clamp(0.0, 100.0), ..progress };
        }
        QueueCommand::Finish { item_id, run_id, result } => {
            check_active_run(state, &run_id)?;
            let item = item_mut(state, &item_id)?;
            check_run(item, &run_id)?;
            if item.status != QueueItemStatus::Running {
                return Err(QueueError::Illegal {
                    item_id,
                    message: "only running items can finish".into(),
                });
            }
            item.status = if result.skipped {
                QueueItemStatus::Skipped
            } else if result.success {
                QueueItemStatus::Done
            } else {
                item.error = Some(JobError {
                    message: result
                        .error
                        .clone()
                        .unwrap_or_else(|| "Queue item execution failed.".into()),
                });
                QueueItemStatus::Failed
            };
            item.progress.percent = 100.0;
            item.result = Some(result);
            item.run_id = None;
        }
        QueueCommand::Fail { item_id, run_id, error } => {
            check_active_run(state, &run_id)?;
            let item = item_mut(state, &item_id)?;
            check_run(item, &run_id)?;
            if item.status != QueueItemStatus::Running {
                return Err(QueueError::Illegal {
                    item_id,
                    message: "only running items can fail".into(),
                });
            }
            item.status = QueueItemStatus::Failed;
            item.error = Some(error);
            item.run_id = None;
        }
        QueueCommand::Cancel { item_id, run_id, reason } => {
            check_active_run(state, &run_id)?;
            let item = item_mut(state, &item_id)?;
            check_run(item, &run_id)?;
            if item.status != QueueItemStatus::Running {
                return Err(QueueError::Illegal {
                    item_id,
                    message: "only running items can cancel".into(),
                });
            }
            item.status = QueueItemStatus::Cancelled;
            item.error = Some(JobError { message: reason });
            item.run_id = None;
        }
        QueueCommand::PauseAfterCurrent => {
            if state.run_state != QueueRunState::Running {
                return Err(QueueError::Busy);
            }
            state.run_state = QueueRunState::PauseRequested;
        }
        QueueCommand::PauseComplete { run_id } => {
            check_active_run(state, &run_id)?;
            if state.run_state != QueueRunState::PauseRequested {
                return Err(QueueError::Busy);
            }
            if state.items.iter().any(|item| item.status == QueueItemStatus::Running) {
                return Err(QueueError::Busy);
            }
            state.run_state = QueueRunState::Paused;
            state.active_run_id = None;
        }
        QueueCommand::CancelRun { run_id } => {
            check_active_run(state, &run_id)?;
            if !matches!(
                state.run_state,
                QueueRunState::Running | QueueRunState::PauseRequested | QueueRunState::Cancelling
            ) {
                return Err(QueueError::Busy);
            }
            state.run_state = QueueRunState::Cancelling;
        }
        QueueCommand::AbortRun { run_id, reason } => {
            check_active_run(state, &run_id)?;
            for item in &mut state.items {
                if item.status == QueueItemStatus::Running
                    && item.run_id.as_deref() == Some(run_id.as_str())
                {
                    item.status = QueueItemStatus::Failed;
                    item.error = Some(JobError { message: reason.clone() });
                    item.run_id = None;
                }
            }
            state.run_state = QueueRunState::Idle;
            state.active_run_id = None;
        }
        QueueCommand::RecoverRun { reason } => {
            for item in &mut state.items {
                if item.status == QueueItemStatus::Running {
                    item.status = QueueItemStatus::Cancelled;
                    item.error = Some(JobError { message: reason.clone() });
                }
                item.run_id = None;
            }
            state.run_state = QueueRunState::Idle;
            state.active_run_id = None;
        }
        QueueCommand::RunIdle { run_id } => {
            check_active_run(state, &run_id)?;
            if state.items.iter().any(|item| {
                item.status == QueueItemStatus::Running
                    && item.run_id.as_deref() == Some(run_id.as_str())
            }) {
                return Err(QueueError::Busy);
            }
            state.run_state = QueueRunState::Idle;
            state.active_run_id = None;
        }
        QueueCommand::Retry { item_ids } => {
            if !matches!(state.run_state, QueueRunState::Idle | QueueRunState::Paused) {
                return Err(QueueError::Busy);
            }
            for id in item_ids {
                let item = item_mut(state, &id)?;
                if !matches!(item.status, QueueItemStatus::Failed | QueueItemStatus::Cancelled) {
                    return Err(QueueError::Illegal {
                        item_id: id,
                        message: "only failed or cancelled items can retry".into(),
                    });
                }
                item.status = QueueItemStatus::Queued;
                item.error = None;
                item.result = None;
                item.progress = ItemProgress::default();
            }
        }
        QueueCommand::Remove { item_ids } => {
            if !matches!(state.run_state, QueueRunState::Idle | QueueRunState::Paused) {
                return Err(QueueError::Busy);
            }
            if state.items.iter().any(|item| {
                item_ids.iter().any(|id| id == &item.item_id)
                    && item.status == QueueItemStatus::Running
            }) {
                return Err(QueueError::Busy);
            }
            state.items.retain(|item| !item_ids.iter().any(|id| id == &item.item_id));
        }
        QueueCommand::Reorder { ordered_ids } => {
            if !matches!(state.run_state, QueueRunState::Idle | QueueRunState::Paused) {
                return Err(QueueError::Busy);
            }
            if ordered_ids.len() != state.items.len()
                || ordered_ids.iter().collect::<std::collections::HashSet<_>>().len()
                    != state.items.len()
                || state.items.iter().any(|item| !ordered_ids.contains(&item.item_id))
            {
                return Err(QueueError::InvalidOrder);
            }
            let movable_before = state
                .items
                .iter()
                .filter(|item| {
                    matches!(item.status, QueueItemStatus::Draft | QueueItemStatus::Queued)
                })
                .map(|item| item.item_id.clone())
                .collect::<Vec<_>>();
            let fixed_before = state
                .items
                .iter()
                .filter(|item| {
                    !matches!(item.status, QueueItemStatus::Draft | QueueItemStatus::Queued)
                })
                .map(|item| item.item_id.clone())
                .collect::<Vec<_>>();
            let movable_after = ordered_ids
                .iter()
                .filter(|id| movable_before.iter().any(|item| item == *id))
                .cloned()
                .collect::<Vec<_>>();
            let fixed_after = ordered_ids
                .iter()
                .filter(|id| fixed_before.iter().any(|item| item == *id))
                .cloned()
                .collect::<Vec<_>>();
            if movable_after.len() != movable_before.len() || fixed_after != fixed_before {
                return Err(QueueError::InvalidOrder);
            }
            let old = std::mem::take(&mut state.items);
            state.items = ordered_ids
                .into_iter()
                .filter_map(|id| old.iter().find(|item| item.item_id == id).cloned())
                .collect();
        }
        QueueCommand::ClearCompleted => {
            if !matches!(state.run_state, QueueRunState::Idle | QueueRunState::Paused) {
                return Err(QueueError::Busy);
            }
            state.items.retain(|item| {
                !matches!(
                    item.status,
                    QueueItemStatus::Done | QueueItemStatus::Skipped | QueueItemStatus::Cancelled
                )
            });
        }
    }
    Ok(())
}
