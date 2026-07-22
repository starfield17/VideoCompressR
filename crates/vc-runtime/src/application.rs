use crate::activity::ActivityHub;
use crate::error::RuntimeError;
use crate::ffmpeg::{capabilities::ensure_capabilities, discover_tools};
use crate::planning::{EncodePlan, PlanRequest, PlanningService};
use crate::platform::paths::AppPaths;
use crate::queue::supervisor::{ExecutionContext, QueueSnapshot, QueueSupervisor};
use crate::storage::app_config::AppConfig;
use crate::storage::i18n::Translator;
use crate::storage::presets::PresetStore;
use crate::storage::settings::SettingsStore;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use vc_core::{EncodeSettings, PreviewOptions};

#[derive(Clone)]
pub struct Application {
    pub paths: AppPaths,
    pub planning: PlanningService,
    pub presets: PresetStore,
    pub queue: Arc<QueueSupervisor>,
    pub activity: ActivityHub,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct BootstrapSnapshot {
    pub config: AppConfig,
    pub ffmpeg_path: Option<PathBuf>,
    pub ffprobe_path: Option<PathBuf>,
    pub queue: QueueSnapshot,
}

impl Application {
    pub fn bootstrap(paths: AppPaths) -> Result<Self, RuntimeError> {
        paths.ensure()?;
        let config = AppConfig::load(&paths)?;
        config.save(&paths)?;
        let activity = ActivityHub::new();
        let presets = PresetStore::new(paths.clone());
        presets.ensure_defaults()?;
        let queue = Arc::new(QueueSupervisor::new(activity.clone()));
        Ok(Self { planning: PlanningService::new(paths.clone()), presets, paths, queue, activity })
    }
    pub fn current() -> Result<Self, RuntimeError> {
        Self::bootstrap(AppPaths::current())
    }
    pub fn config(&self) -> Result<AppConfig, RuntimeError> {
        AppConfig::load(&self.paths)
    }
    pub fn save_config(&self, value: &AppConfig) -> Result<(), RuntimeError> {
        value.save(&self.paths)
    }
    pub async fn bootstrap_snapshot(&self) -> Result<BootstrapSnapshot, RuntimeError> {
        let config = self.config()?;
        let tools = discover_tools(
            (!config.ffmpeg_path.is_empty()).then(|| PathBuf::from(&config.ffmpeg_path)).as_deref(),
            (!config.ffprobe_path.is_empty())
                .then(|| PathBuf::from(&config.ffprobe_path))
                .as_deref(),
            &self.paths,
        )
        .ok();
        Ok(BootstrapSnapshot {
            config,
            ffmpeg_path: tools.as_ref().map(|value| value.ffmpeg.clone()),
            ffprobe_path: tools.map(|value| value.ffprobe),
            queue: self.queue.snapshot().await.as_ref().clone(),
        })
    }
    pub async fn plan(&self, request: PlanRequest) -> Result<EncodePlan, RuntimeError> {
        self.planning.plan(request).await
    }
    pub async fn encode(
        &self,
        plan: &EncodePlan,
        cancel: CancellationToken,
    ) -> Result<Vec<crate::execution::ExecutionResult>, RuntimeError> {
        let paths = self.paths.for_workdir(&plan.workdir);
        paths.ensure()?;
        crate::execution::execute_plan(plan, &paths, &self.activity, cancel, None).await
    }
    pub async fn preview(
        &self,
        request: PlanRequest,
        options: PreviewOptions,
        cancel: CancellationToken,
    ) -> Result<vc_core::PreviewResult, RuntimeError> {
        crate::preview::preview(
            &self.planning,
            &self.paths,
            &self.activity,
            request,
            options,
            cancel,
            None,
        )
        .await
    }
    pub async fn enqueue_and_start(&self, plan: &EncodePlan) -> Result<(), RuntimeError> {
        self.queue.enqueue(plan.items.clone()).await?;
        self.queue.set_workdir(plan.workdir.clone()).await;
        self.start_queue_with_tools(plan.ffmpeg_path.clone(), plan.ffprobe_path.clone()).await
    }

    pub async fn start_queue(&self) -> Result<(), RuntimeError> {
        let config = self.config()?;
        let tools = discover_tools(
            (!config.ffmpeg_path.is_empty()).then(|| PathBuf::from(config.ffmpeg_path)).as_deref(),
            (!config.ffprobe_path.is_empty())
                .then(|| PathBuf::from(config.ffprobe_path))
                .as_deref(),
            &self.paths,
        )?;
        let workdir = if config.workdir_path.is_empty() {
            self.paths.workdir.clone()
        } else {
            PathBuf::from(config.workdir_path)
        };
        self.queue.set_default_workdir(workdir).await;
        self.start_queue_with_tools(tools.ffmpeg, tools.ffprobe).await
    }

    async fn start_queue_with_tools(
        &self,
        ffmpeg: PathBuf,
        ffprobe: PathBuf,
    ) -> Result<(), RuntimeError> {
        let tools = discover_tools(Some(&ffmpeg), Some(&ffprobe), &self.paths)?;
        self.queue
            .start(ExecutionContext {
                paths: self.paths.clone(),
                tools,
                activity: self.activity.clone(),
            })
            .await
    }
    pub fn default_settings(&self) -> Result<EncodeSettings, RuntimeError> {
        if let Some(value) = SettingsStore::new(self.paths.clone()).load()? {
            return Ok(value);
        }
        let config = self.config()?;
        if let Some(name) = config.default_preset_name {
            if let Ok(value) = self.presets.load(&name) {
                return Ok(value);
            }
        }
        Ok(EncodeSettings::default())
    }

    pub fn save_settings(&self, settings: &EncodeSettings) -> Result<(), RuntimeError> {
        SettingsStore::new(self.paths.clone()).save(settings)
    }

    pub fn translator(&self, language: &str) -> Result<Translator, RuntimeError> {
        Translator::load(&self.paths, language)
    }

    pub async fn refresh_capabilities(&self) -> Result<(), RuntimeError> {
        let config = self.config()?;
        let tools = discover_tools(
            (!config.ffmpeg_path.is_empty()).then(|| PathBuf::from(config.ffmpeg_path)).as_deref(),
            (!config.ffprobe_path.is_empty())
                .then(|| PathBuf::from(config.ffprobe_path))
                .as_deref(),
            &self.paths,
        )?;
        ensure_capabilities(&self.paths, &tools, true).await.map(|_| ())
    }
}
