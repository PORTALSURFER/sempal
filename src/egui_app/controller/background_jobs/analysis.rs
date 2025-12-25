use super::progress;
use super::*;
use crate::egui_app::state::ProgressTaskKind;

pub(super) fn handle_analysis_message(
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
                                super::analysis_jobs::current_progress_for_source(&source)
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
                return;
            }
            if progress.pending == 0 && progress.running == 0 {
                if let Some(source) = controller.current_source() {
                    if let Ok(failures) =
                        super::analysis_jobs::failed_samples_for_source(&source)
                    {
                        controller
                            .ui_cache
                            .browser
                            .analysis_failures
                            .insert(source.id.clone(), failures);
                    }
                    controller.ui_cache.browser.features.remove(&source.id);
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
