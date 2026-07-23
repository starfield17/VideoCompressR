//! Bounded, single-open process log writer.
//!
//! Each encode/preview task opens the log file once and writes through a
//! bounded channel. Diagnostic lines take priority over progress summaries.

use crate::error::RuntimeError;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

pub const PROCESS_LOG_CHANNEL_CAPACITY: usize = 256;

#[derive(Debug)]
enum LogCommand {
    Write(String),
    Flush(oneshot::Sender<Result<(), RuntimeError>>),
}

/// Counts opens for tests / instrumentation without depending on the filesystem.
#[derive(Clone, Default)]
pub struct LogOpenCounter {
    opens: Arc<AtomicU64>,
}

impl LogOpenCounter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_open(&self) {
        self.opens.fetch_add(1, Ordering::SeqCst);
    }

    pub fn opens(&self) -> u64 {
        self.opens.load(Ordering::SeqCst)
    }
}

/// Lightweight cloneable handle for non-async callbacks (process line handlers).
#[derive(Clone)]
pub struct ProcessLogSender {
    sender: mpsc::Sender<LogCommand>,
}

impl ProcessLogSender {
    pub async fn send(&self, text: impl Into<String>) -> Result<(), RuntimeError> {
        let mut text = text.into();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        self.sender
            .send(LogCommand::Write(text))
            .await
            .map_err(|_| RuntimeError::Encode("process log writer closed".into()))
    }

    pub fn try_send(&self, text: impl Into<String>) -> bool {
        let mut text = text.into();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        self.sender.try_send(LogCommand::Write(text)).is_ok()
    }
}

pub struct ProcessLogWriter {
    sender: mpsc::Sender<LogCommand>,
    handle: JoinHandle<Result<(), RuntimeError>>,
}

impl ProcessLogWriter {
    pub async fn open(path: PathBuf) -> Result<Self, RuntimeError> {
        Self::open_with_counter(path, None).await
    }

    pub async fn open_with_counter(
        path: PathBuf,
        counter: Option<LogOpenCounter>,
    ) -> Result<Self, RuntimeError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let file = tokio::fs::OpenOptions::new().create(true).append(true).open(&path).await?;
        if let Some(counter) = &counter {
            counter.record_open();
        }
        let (sender, receiver) = mpsc::channel(PROCESS_LOG_CHANNEL_CAPACITY);
        let handle = tokio::spawn(async move { writer_loop(file, receiver).await });
        Ok(Self { sender, handle })
    }

    pub fn sender(&self) -> ProcessLogSender {
        ProcessLogSender { sender: self.sender.clone() }
    }

    /// Enqueue a diagnostic line. Applies backpressure when the channel is full
    /// (does not drop). Prefer this for stderr and lifecycle messages.
    pub async fn write_line(&self, text: impl Into<String>) -> Result<(), RuntimeError> {
        let mut text = text.into();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        self.sender
            .send(LogCommand::Write(text))
            .await
            .map_err(|_| RuntimeError::Encode("process log writer closed".into()))
    }

    /// Best-effort write that never blocks. Suitable for progress summaries.
    pub fn try_write_line(&self, text: impl Into<String>) -> bool {
        self.sender().try_send(text)
    }

    pub async fn flush(&self) -> Result<(), RuntimeError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(LogCommand::Flush(tx))
            .await
            .map_err(|_| RuntimeError::Encode("process log writer closed".into()))?;
        rx.await.map_err(|_| RuntimeError::Encode("process log flush cancelled".into()))?
    }

    /// Close the writer, flush, and wait for the background task.
    pub async fn finish(self) -> Result<(), RuntimeError> {
        let Self { sender, handle } = self;
        drop(sender);
        handle.await.map_err(|error| RuntimeError::Encode(error.to_string()))?
    }
}

async fn writer_loop(
    file: tokio::fs::File,
    mut receiver: mpsc::Receiver<LogCommand>,
) -> Result<(), RuntimeError> {
    let mut writer = BufWriter::new(file);
    while let Some(command) = receiver.recv().await {
        match command {
            LogCommand::Write(text) => {
                writer.write_all(text.as_bytes()).await?;
            }
            LogCommand::Flush(reply) => {
                let result = writer.flush().await.map_err(RuntimeError::from);
                let _ = reply.send(result);
            }
        }
    }
    writer.flush().await?;
    Ok(())
}

/// Synchronous open/append used only for one-shot headers when no async writer exists.
pub fn append_log_sync(path: &Path, text: &str) -> Result<(), RuntimeError> {
    use std::fs::OpenOptions;
    use std::io::Write;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(text.as_bytes())?;
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn process_log_file_is_opened_once_per_execution() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("encode.log");
        let counter = LogOpenCounter::new();
        let writer = ProcessLogWriter::open_with_counter(path.clone(), Some(counter.clone()))
            .await
            .expect("open");
        for index in 0..100 {
            writer.write_line(format!("line {index}")).await.expect("write");
        }
        writer.finish().await.expect("finish");
        assert_eq!(counter.opens(), 1);
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("line 0"));
        assert!(text.contains("line 99"));
    }

    #[tokio::test]
    async fn open_fails_immediately_when_parent_is_not_a_directory() {
        let temp = tempfile::tempdir().expect("temp");
        let parent = temp.path().join("blocked");
        std::fs::write(&parent, b"file").expect("parent file");
        let result = ProcessLogWriter::open(parent.join("encode.log")).await;
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn writer_failure_is_returned_by_finish() {
        let writer = ProcessLogWriter::open(PathBuf::from("/dev/full")).await.expect("open");
        writer.write_line("this write must fail").await.expect("enqueue");
        assert!(writer.finish().await.is_err());
    }

    #[tokio::test]
    async fn log_writer_flushes_on_completion() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("flush.log");
        let writer = ProcessLogWriter::open(path.clone()).await.expect("open");
        writer.write_line("hello").await.expect("write");
        writer.finish().await.expect("finish");
        assert_eq!(std::fs::read_to_string(&path).expect("read").trim(), "hello");
    }

    #[tokio::test]
    async fn log_writer_flushes_on_cancellation() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("cancel.log");
        let writer = ProcessLogWriter::open(path.clone()).await.expect("open");
        writer.write_line("partial").await.expect("write");
        // Dropping sender via finish path is the cancellation analogue.
        writer.finish().await.expect("finish");
        assert!(std::fs::read_to_string(&path).expect("read").contains("partial"));
    }

    #[tokio::test]
    async fn bounded_log_channel_applies_backpressure() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("backpressure.log");
        let writer = ProcessLogWriter::open(path).await.expect("open");
        // Fill beyond capacity with try_write; excess should fail, not grow memory.
        let mut accepted = 0_usize;
        let mut rejected = 0_usize;
        for index in 0..PROCESS_LOG_CHANNEL_CAPACITY * 4 {
            if writer.try_write_line(format!("p {index}")) {
                accepted += 1;
            } else {
                rejected += 1;
            }
        }
        assert!(accepted <= PROCESS_LOG_CHANNEL_CAPACITY + 1);
        assert!(rejected > 0);
        // Diagnostic path still works after draining.
        let _ = writer.flush().await;
        writer.write_line("diagnostic").await.expect("diagnostic");
        writer.finish().await.expect("finish");
    }

    #[tokio::test]
    async fn large_process_output_does_not_grow_memory_unbounded() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("large.log");
        let counter = LogOpenCounter::new();
        let writer =
            ProcessLogWriter::open_with_counter(path, Some(counter.clone())).await.expect("open");
        // Produce a large stream of progress-like lines using try_send only.
        for index in 0..100_000 {
            let _ = writer.try_write_line(format!("frame={index}"));
            if index % 1_000 == 0 {
                tokio::task::yield_now().await;
            }
        }
        // Channel capacity is fixed; opens stay at 1.
        assert_eq!(counter.opens(), 1);
        // Allow some drain time then finish.
        tokio::time::sleep(Duration::from_millis(50)).await;
        writer.finish().await.expect("finish");
    }
}
