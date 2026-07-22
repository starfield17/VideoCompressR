//! Runtime boundary for tools, storage, planning, and queue execution.

pub mod activity;
pub mod application;
pub mod error;
pub mod execution;
pub mod ffmpeg;
pub mod planning;
pub mod platform;
pub mod preview;
pub mod queue;
pub mod scanner;
pub mod storage;
pub mod subtitles;

pub use activity::{ActivityEvent, ActivityHub};
pub use application::{Application, BootstrapSnapshot};
pub use error::RuntimeError;
pub use execution::{ExecutionResult, ProgressEvent, ProgressSink};
pub use planning::{EncodePlan, PlanRequest, PlanningService};
pub use platform::paths::AppPaths;
pub use storage::i18n::Translator;
