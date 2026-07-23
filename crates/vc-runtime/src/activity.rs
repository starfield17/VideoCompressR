use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

use crate::error::RuntimeError;

/// Hard cap on retained activity events in memory.
pub const MAX_ACTIVITY_HISTORY: usize = 5_000;
/// Default number of events returned by history IPC.
pub const DEFAULT_ACTIVITY_HISTORY_LIMIT: usize = 500;
/// Maximum number of events a single history request may return.
pub const MAX_ACTIVITY_HISTORY_REQUEST: usize = 2_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ActivityEvent {
    pub category: String,
    pub message: String,
    pub timestamp: String,
}

#[derive(Clone)]
pub struct ActivityHub {
    sender: broadcast::Sender<ActivityEvent>,
    history: Arc<Mutex<VecDeque<ActivityEvent>>>,
}

impl ActivityHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(512);
        Self { sender, history: Arc::new(Mutex::new(VecDeque::new())) }
    }

    pub fn emit(&self, category: impl Into<String>, message: impl Into<String>) {
        let event = ActivityEvent {
            category: category.into(),
            message: message.into(),
            timestamp: format!("{:?}", std::time::SystemTime::now()),
        };
        if let Ok(mut history) = self.history.lock() {
            history.push_back(event.clone());
            while history.len() > MAX_ACTIVITY_HISTORY {
                history.pop_front();
            }
        }
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ActivityEvent> {
        self.sender.subscribe()
    }

    /// Full retained history (bounded by [`MAX_ACTIVITY_HISTORY`]).
    pub fn history(&self) -> Vec<ActivityEvent> {
        self.history.lock().map(|value| value.iter().cloned().collect()).unwrap_or_default()
    }

    /// Latest `limit` events. `limit` is clamped to [`MAX_ACTIVITY_HISTORY_REQUEST`].
    pub fn history_tail(&self, limit: usize) -> Vec<ActivityEvent> {
        let limit = limit.min(MAX_ACTIVITY_HISTORY_REQUEST);
        let Ok(history) = self.history.lock() else {
            return Vec::new();
        };
        let len = history.len();
        if limit == 0 || len == 0 {
            return Vec::new();
        }
        let start = len.saturating_sub(limit);
        history.iter().skip(start).cloned().collect()
    }

    pub fn retained_len(&self) -> usize {
        self.history.lock().map(|value| value.len()).unwrap_or(0)
    }

    pub fn clear(&self) {
        if let Ok(mut history) = self.history.lock() {
            history.clear();
        }
    }

    /// Stream retained events to a file without building a giant intermediate string.
    pub fn export(&self, path: &Path) -> Result<(), RuntimeError> {
        let events = self.history();
        let file = std::fs::File::create(path)?;
        let mut writer = BufWriter::new(file);
        for event in events {
            writeln!(writer, "[{}] [{}] {}", event.timestamp, event.category, event.message)?;
        }
        writer.flush()?;
        Ok(())
    }
}

impl Default for ActivityHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_history_is_bounded() {
        let activity = ActivityHub::new();
        for index in 0..(MAX_ACTIVITY_HISTORY + 2_500) {
            activity.emit("process", format!("event-{index}"));
        }
        assert_eq!(activity.retained_len(), MAX_ACTIVITY_HISTORY);
        assert_eq!(activity.history().len(), MAX_ACTIVITY_HISTORY);
    }

    #[test]
    fn activity_history_tail_returns_latest_events() {
        let activity = ActivityHub::new();
        for index in 0..100 {
            activity.emit("process", format!("event-{index}"));
        }
        let tail = activity.history_tail(10);
        assert_eq!(tail.len(), 10);
        assert_eq!(tail[0].message, "event-90");
        assert_eq!(tail[9].message, "event-99");
    }

    #[test]
    fn activity_history_limit_is_clamped() {
        let activity = ActivityHub::new();
        for index in 0..100 {
            activity.emit("process", format!("event-{index}"));
        }
        let tail = activity.history_tail(MAX_ACTIVITY_HISTORY_REQUEST + 10_000);
        assert!(tail.len() <= MAX_ACTIVITY_HISTORY_REQUEST);
        assert_eq!(tail.len(), 100);
    }

    #[test]
    fn activity_export_streams_all_retained_events() {
        let temp = tempfile::tempdir().expect("temp");
        let activity = ActivityHub::new();
        for index in 0..50 {
            activity.emit("process", format!("line-{index}"));
        }
        let path = temp.path().join("activity.log");
        activity.export(&path).expect("export");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("[process] line-0"));
        assert!(text.contains("[process] line-49"));
        assert_eq!(text.lines().count(), 50);
    }
}
