use crate::activity::ActivityHub;
use crate::error::RuntimeError;
use crate::execution::{ProgressSink, execute_preview};
use crate::planning::{PlanRequest, PlanningService};
use crate::platform::paths::AppPaths;
use tokio_util::sync::CancellationToken;
use vc_core::{PreviewJob, PreviewOptions, choose_sample_window};

pub async fn preview(
    service: &PlanningService,
    paths: &AppPaths,
    activity: &ActivityHub,
    request: PlanRequest,
    options: PreviewOptions,
    cancel: CancellationToken,
    sink: Option<ProgressSink>,
) -> Result<vc_core::PreviewResult, RuntimeError> {
    if !request.input_path.is_file() {
        return Err(RuntimeError::Planning("Preview requires a single input file.".into()));
    }
    let effective_paths =
        paths.for_workdir(request.workdir.as_deref().unwrap_or(paths.workdir.as_path()));
    effective_paths.ensure()?;
    let plan = service.plan(request).await?;
    let item = plan.items.into_iter().find(|item| item.is_ready()).ok_or_else(|| {
        RuntimeError::Planning("No valid plan item is available for preview.".into())
    })?;
    let media = item
        .media_info
        .as_ref()
        .ok_or_else(|| RuntimeError::Planning("Preview plan has no media info.".into()))?;
    let window = choose_sample_window(media.duration, &options).map_err(RuntimeError::Planning)?;
    let token = item.source_path.file_stem().and_then(|value| value.to_str()).unwrap_or("item");
    let preview_root = effective_paths.previews_dir.join(token);
    std::fs::create_dir_all(&preview_root)?;
    let source_sample = preview_root.join(format!(
        "{token}_source_sample{}",
        item.source_path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| format!(".{value}"))
            .unwrap_or_default()
    ));
    let encoded_sample = preview_root.join(format!(
        "{token}_{}_preview.{}",
        item.settings.codec.as_str(),
        item.settings.container.as_str()
    ));
    let job = PreviewJob {
        source_path: item.source_path.clone(),
        source_sample_path: source_sample,
        encoded_sample_path: encoded_sample,
        window,
        plan_item: item,
    };
    execute_preview(&job, &plan.ffmpeg_path, &effective_paths, activity, cancel, sink).await
}
