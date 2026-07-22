use super::{
    ItemProgress, ItemResult, JobError, QueueItem, QueueItemStatus, QueueRunState, QueueState,
};
use crate::model::EncodePlanItem;
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

pub fn apply(state: &mut QueueState, command: QueueCommand) -> Result<(), QueueError> {
    match command {
        QueueCommand::Enqueue(plans) => {
            if !matches!(state.run_state, QueueRunState::Idle | QueueRunState::Paused) {
                return Err(QueueError::Busy);
            }
            let start = state.items.len();
            state.items.extend(plans.into_iter().enumerate().map(|(offset, plan)| QueueItem {
                item_id: format!("item-{}-{}", start + offset + 1, plan.source_path.display()),
                status: if plan.skip_reason.is_some() {
                    QueueItemStatus::Skipped
                } else {
                    QueueItemStatus::Queued
                },
                progress: ItemProgress {
                    percent: if plan.skip_reason.is_some() { 100.0 } else { 0.0 },
                    ..ItemProgress::default()
                },
                error: plan.skip_reason.as_ref().map(|value| JobError { message: value.0.clone() }),
                result: None,
                run_id: None,
                plan,
            }));
        }
        QueueCommand::StartRun { .. } => {
            if !matches!(state.run_state, QueueRunState::Idle | QueueRunState::Paused) {
                return Err(QueueError::Busy);
            }
            if !state.items.iter().any(|item| item.status == QueueItemStatus::Queued) {
                return Err(QueueError::Illegal {
                    item_id: String::new(),
                    message: "no queued items".into(),
                });
            }
            state.run_state = QueueRunState::Running;
        }
        QueueCommand::StartItem { item_id, run_id } => {
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
                QueueItemStatus::Failed
            };
            item.progress.percent = 100.0;
            item.result = Some(result);
            item.run_id = None;
        }
        QueueCommand::Fail { item_id, run_id, error } => {
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
        QueueCommand::PauseComplete { .. } => {
            if state.run_state != QueueRunState::PauseRequested {
                return Err(QueueError::Busy);
            }
            if state.items.iter().any(|item| item.status == QueueItemStatus::Running) {
                return Err(QueueError::Busy);
            }
            state.run_state = QueueRunState::Paused;
        }
        QueueCommand::CancelRun { .. } => state.run_state = QueueRunState::Cancelling,
        QueueCommand::RunIdle { .. } => state.run_state = QueueRunState::Idle,
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
