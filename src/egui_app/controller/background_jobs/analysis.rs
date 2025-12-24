use super::progress;
use super::*;
use crate::egui_app::state::ProgressTaskKind;

pub(super) fn handle_analysis_message(
    controller: &mut EguiController,
    message: AnalysisJobMessage,
) {
    match message {
        AnalysisJobMessage::Progress(progress) => {
            controller.handle_similarity_analysis_progress(&progress);
            if progress.total() == 0 {
                if controller.ui.progress.task == Some(ProgressTaskKind::Analysis) {
                    controller.clear_progress();
                }
                return;
            }
            if progress.pending == 0 && progress.running == 0 {
                if let Some(source_id) = controller.selection_state.ctx.selected_source.clone()
                    && let Ok(failures) =
                        super::analysis_jobs::failed_samples_for_source(&source_id)
                {
                    controller
                        .ui_cache
                        .browser
                        .analysis_failures
                        .insert(source_id, failures);
                }
                if let Some(source_id) = controller.selection_state.ctx.selected_source.clone() {
                    controller.ui_cache.browser.features.remove(&source_id);
                }
                if controller.ui.progress.task == Some(ProgressTaskKind::Analysis) {
                    controller.clear_progress();
                }
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
                .send(JobMessage::Analysis(AnalysisJobMessage::Progress(progress)));
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
                .send(JobMessage::Analysis(AnalysisJobMessage::Progress(progress)));
        }
        AnalysisJobMessage::EmbeddingBackfillEnqueueFailed(err) => {
            controller.set_status(
                format!("Embedding backfill enqueue failed: {err}"),
                StatusTone::Error,
            );
        }
    }
}
