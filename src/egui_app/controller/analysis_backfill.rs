use super::*;

impl EguiController {
    pub fn backfill_missing_features_for_selected_source(&mut self) {
        let Some(source_id) = self.selection_state.ctx.selected_source.clone() else {
            self.set_status("Select a source first", StatusTone::Warning);
            return;
        };
        let tx = self.runtime.jobs.message_sender();
        std::thread::spawn(move || {
            let result = super::analysis_jobs::enqueue_jobs_for_source_missing_features(&source_id);
            match result {
                Ok((inserted, progress)) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::EnqueueFinished {
                            inserted,
                            progress,
                        },
                    ));
                }
                Err(err) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::EnqueueFailed(err),
                    ));
                }
            }
        });
    }

    pub fn recompute_weak_labels_for_selected_source(&mut self) {
        let Some(source_id) = self.selection_state.ctx.selected_source.clone() else {
            self.set_status("Select a source first", StatusTone::Warning);
            return;
        };
        let source_id_str = source_id.to_string();
        let tx = self.runtime.jobs.message_sender();
        std::thread::spawn(move || {
            let result = super::analysis_jobs::recompute_weak_labels_for_source(&source_id);
            match result {
                Ok(updated_samples) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::WeakLabelsRecomputed {
                            source_id: source_id_str,
                            updated_samples,
                        },
                    ));
                }
                Err(err) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::WeakLabelsRecomputeFailed(err),
                    ));
                }
            }
        });
    }
}
