use crate::error::RuntimeError;
use crate::ffmpeg::{capabilities::ensure_capabilities, discover_tools, probe::probe_media_info};
use crate::platform::paths::AppPaths;
use crate::scanner::collect_video_files;
use std::path::PathBuf;
use std::sync::Arc;
use vc_core::planning::{
    PlanningInput, build_output_path, choose_output_root, plan_item, skipped_item,
};
use vc_core::{EncodePlanItem, EncodeSettings, VideoFileItem};

#[derive(Clone, Debug)]
pub struct PlanRequest {
    pub input_path: PathBuf,
    pub output_dir: Option<PathBuf>,
    pub workdir: Option<PathBuf>,
    pub ffmpeg_path: Option<PathBuf>,
    pub ffprobe_path: Option<PathBuf>,
    pub settings: EncodeSettings,
    pub force_capability_refresh: bool,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct EncodePlan {
    pub items: Vec<EncodePlanItem>,
    pub ffmpeg_path: PathBuf,
    pub ffprobe_path: PathBuf,
    pub input_root: PathBuf,
    pub output_root: PathBuf,
    pub workdir: PathBuf,
}

#[derive(Clone)]
pub struct PlanningService {
    paths: AppPaths,
}

impl PlanningService {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }
    pub async fn plan(&self, request: PlanRequest) -> Result<EncodePlan, RuntimeError> {
        let files = collect_video_files(&request.input_path, request.settings.recursive)?;
        if files.is_empty() {
            return Err(RuntimeError::Planning("No processable video files were found.".into()));
        }
        let input_root = request.input_path.canonicalize().map_err(|error| {
            RuntimeError::Planning(format!(
                "Cannot access input path {}: {error}",
                request.input_path.display()
            ))
        })?;
        let workdir = request.workdir.clone().unwrap_or_else(|| self.paths.workdir.clone());
        self.paths.for_workdir(&workdir).ensure()?;
        let tools = discover_tools(
            request.ffmpeg_path.as_deref(),
            request.ffprobe_path.as_deref(),
            &self.paths,
        )?;
        let capabilities =
            ensure_capabilities(&self.paths, &tools, request.force_capability_refresh).await?;
        let input_is_file = input_root.is_file();
        let output_root = choose_output_root(
            &input_root,
            input_is_file,
            request.output_dir.as_deref(),
            request.settings.codec,
        );
        std::fs::create_dir_all(&output_root)?;
        let mut items = Vec::with_capacity(files.len());
        for file in files {
            let output = build_output_path(
                &file,
                if input_is_file { file.path.parent().unwrap_or(&input_root) } else { &input_root },
                !input_is_file,
                &output_root,
                request.settings.codec,
                request.settings.container,
            );
            let media = probe_media_info(&tools.ffprobe, &file.path).await?;
            let item = plan_item(PlanningInput {
                source: media,
                output_path: output.clone(),
                settings: request.settings.clone(),
                capabilities: capabilities.clone(),
                output_exists: output.exists(),
            });
            items.push(item.unwrap_or_else(|reason| {
                skipped_item(file.path, output, request.settings.clone(), reason)
            }));
        }
        Ok(EncodePlan {
            items,
            ffmpeg_path: tools.ffmpeg,
            ffprobe_path: tools.ffprobe,
            input_root,
            output_root,
            workdir,
        })
    }
}

pub fn video_file_items(paths: &[PathBuf]) -> Vec<VideoFileItem> {
    paths
        .iter()
        .map(|path| VideoFileItem {
            path: path.clone(),
            relative_path: path.file_name().map(PathBuf::from).unwrap_or_default(),
        })
        .collect()
}

pub type SharedPlanningService = Arc<PlanningService>;
