use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("tool discovery failed: {0}")]
    ToolDiscovery(String),
    #[error("tool failed with exit code {code}: {message}")]
    ToolFailed { code: i32, message: String },
    #[error("FFprobe failed: {0}")]
    Probe(String),
    #[error("capability detection failed: {0}")]
    Capability(String),
    #[error("planning failed: {0}")]
    Planning(String),
    #[error("encoding failed: {0}")]
    Encode(String),
    #[error("operation cancelled")]
    Cancelled,
    #[error("configuration error: {0}")]
    Config(String),
    #[error("background lifecycle error: {0}")]
    Background(String),
    #[error("queue error: {0}")]
    Queue(#[from] vc_core::queue::QueueError),
}

impl From<vc_core::CoreError> for RuntimeError {
    fn from(value: vc_core::CoreError) -> Self {
        Self::Planning(value.to_string())
    }
}
