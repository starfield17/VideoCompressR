use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

use crate::error::RuntimeError;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ActivityEvent {
    pub category: String,
    pub message: String,
    pub timestamp: String,
}

#[derive(Clone)]
pub struct ActivityHub {
    sender: broadcast::Sender<ActivityEvent>,
    history: Arc<Mutex<Vec<ActivityEvent>>>,
}

impl ActivityHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(512);
        Self { sender, history: Arc::new(Mutex::new(Vec::new())) }
    }
    pub fn emit(&self, category: impl Into<String>, message: impl Into<String>) {
        let event = ActivityEvent {
            category: category.into(),
            message: message.into(),
            timestamp: format!("{:?}", std::time::SystemTime::now()),
        };
        if let Ok(mut history) = self.history.lock() {
            history.push(event.clone());
            if history.len() > 50_000 {
                let drop_count = history.len() - 50_000;
                history.drain(..drop_count);
            }
        }
        let _ = self.sender.send(event);
    }
    pub fn subscribe(&self) -> broadcast::Receiver<ActivityEvent> {
        self.sender.subscribe()
    }
    pub fn history(&self) -> Vec<ActivityEvent> {
        self.history.lock().map(|value| value.clone()).unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut history) = self.history.lock() {
            history.clear();
        }
    }

    pub fn export(&self, path: &Path) -> Result<(), RuntimeError> {
        let lines = self
            .history()
            .into_iter()
            .map(|event| format!("[{}] [{}] {}", event.timestamp, event.category, event.message))
            .collect::<Vec<_>>();
        let text = if lines.is_empty() { String::new() } else { format!("{}\n", lines.join("\n")) };
        std::fs::write(path, text)?;
        Ok(())
    }
}

impl Default for ActivityHub {
    fn default() -> Self {
        Self::new()
    }
}
