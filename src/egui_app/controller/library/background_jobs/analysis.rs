use super::progress;
use super::*;
use crate::egui_app::state::ProgressTaskKind;
use crate::egui_app::state::RunningJobSnapshot;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn handle_analysis_message(
    controller: &mut EguiController,
    message: AnalysisJobMessage,
) {
    match message {
        AnalysisJobMessage::Progress {
            source_id,
            progress,
        } => {
            if let Some(state) = controller.runtime.similarity_prep.as_ref() {
                if source_id.as_ref() != Some(&state.source_id) {
                    return;
                }
            }
            let selected_source = controller.selection_state.ctx.selected_source.clone();
            let mut progress = progress;
            if source_id.is_none() {
                if let Some(selected_id) = selected_source.as_ref() {
                    if let Some(source) = controller.current_source() {
                        if &source.id == selected_id {
                            if let Ok(scoped) =
                                analysis_jobs::current_progress_for_source(&source)
                            {
                                progress = scoped;
                            }
                        }
                    }
                }
            }
            let selected_matches = match source_id.as_ref() {
                None => true,
                Some(id) => selected_source
                    .as_ref()
                    .map(|selected| selected == id)
                    .unwrap_or(false),
            };
            if let Some(source_id) = source_id.as_ref() {
                if controller
                    .runtime
                    .similarity_prep
                    .as_ref()
                    .is_some_and(|state| &state.source_id == source_id)
                {
                    controller.handle_similarity_analysis_progress(&progress);
                }
            }
            if !selected_matches {
                return;
            }
            if progress.total() == 0 {
                if controller.ui.progress.task == Some(ProgressTaskKind::Analysis) {
                    controller.clear_progress();
                }
                let _ = controller.apply_pending_backend_switch();
                return;
            }
            if progress.pending == 0 && progress.running == 0 {
                if let Some(source) = controller.current_source() {
                    controller.queue_analysis_failures_refresh(&source);
                    controller.ui_cache.browser.features.remove(&source.id);
                }
                if controller.ui.progress.task == Some(ProgressTaskKind::Analysis) {
                    controller.clear_progress();
                }
                let _ = controller.apply_pending_backend_switch();
                return;
            }
            if controller.ui.progress.task.is_none()
                || controller.ui.progress.task == Some(ProgressTaskKind::Analysis)
            {
                progress::ensure_progress_visible(
                    controller,
                    ProgressTaskKind::Analysis,
                    "Analyzing samples",
                    progress.total(),
                    true,
                );
                let jobs_completed = progress.completed();
                let jobs_total = progress.total();
                let samples_completed = progress.samples_completed();
                let samples_total = progress.samples_total;
                let mut detail = format!(
                    "Jobs {jobs_completed}/{jobs_total} • Samples {samples_completed}/{samples_total}"
                );
                if progress.failed > 0 {
                    detail.push_str(&format!(" • {} failed", progress.failed));
                }
                let running_jobs = if let Some(source) = controller.current_source() {
                    analysis_jobs::current_running_jobs_for_source(&source, 3)
                        .ok()
                        .map(|jobs| {
                            let now_epoch = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .ok()
                                .map(|duration| duration.as_secs() as i64);
                            let stale_after = analysis_jobs::stale_running_job_seconds();
                            jobs.into_iter()
                                .map(|job| {
                                    let label = analysis_jobs::parse_sample_id(
                                        job.sample_id.as_str(),
                                    )
                                    .ok()
                                    .map(|(_, path)| path.to_string_lossy().to_string())
                                    .unwrap_or(job.sample_id);
                                    RunningJobSnapshot::from_heartbeat(
                                        label,
                                        job.last_heartbeat_at,
                                        Some(stale_after),
                                        now_epoch,
                                    )
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                controller.ui.progress.set_analysis_snapshot(Some(
                    crate::egui_app::state::AnalysisProgressSnapshot {
                        pending: progress.pending,
                        running: progress.running,
                        failed: progress.failed,
                        samples_completed,
                        samples_total,
                        running_jobs,
                        stale_after_secs: Some(analysis_jobs::stale_running_job_seconds()),
                    },
                ));
                progress::update_progress_totals(
                    controller,
                    ProgressTaskKind::Analysis,
                    progress.total(),
                    progress.completed(),
                    Some(detail),
                );
            }
        }
        AnalysisJobMessage::EnqueueFinished { inserted, progress } => {
            controller.runtime.analysis.resume();
            if inserted > 0 {
                controller.set_status(format!("Queued {inserted} analysis jobs"), StatusTone::Info);
            }
            if let Some(source_id) = controller.selection_state.ctx.selected_source.clone() {
                controller.ui_cache.browser.features.remove(&source_id);
            }
            let _ = controller
                .runtime
                .jobs
                .message_sender()
                .send(JobMessage::Analysis(AnalysisJobMessage::Progress {
                    source_id: controller.selection_state.ctx.selected_source.clone(),
                    progress,
                }));
        }
        AnalysisJobMessage::EnqueueFailed(err) => {
            controller.set_status(format!("Analysis enqueue failed: {err}"), StatusTone::Error);
        }
        AnalysisJobMessage::EmbeddingBackfillEnqueueFinished { inserted, progress } => {
            controller.runtime.analysis.resume();
            if inserted > 0 {
                controller.set_status(
                    format!("Queued {inserted} embedding backfill jobs"),
                    StatusTone::Info,
                );
            }
            let _ = controller
                .runtime
                .jobs
                .message_sender()
                .send(JobMessage::Analysis(AnalysisJobMessage::Progress {
                    source_id: controller.selection_state.ctx.selected_source.clone(),
                    progress,
                }));
        }
        AnalysisJobMessage::EmbeddingBackfillEnqueueFailed(err) => {
            controller.set_status(
                format!("Embedding backfill enqueue failed: {err}"),
                StatusTone::Error,
            );
        }
    }
}
