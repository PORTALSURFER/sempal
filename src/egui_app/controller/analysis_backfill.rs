use super::*;

impl EguiController {
    pub fn backfill_missing_features_for_selected_source(&mut self) {
        let Some(source) = self.current_source() else {
            self.set_status("Select a source first", StatusTone::Warning);
            return;
        };
        let tx = self.runtime.jobs.message_sender();
        std::thread::spawn(move || {
            let result = super::analysis_jobs::enqueue_jobs_for_source_missing_features(&source);
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


    pub fn backfill_embeddings_for_selected_source(&mut self) {
        let Some(source) = self.current_source() else {
            self.set_status("Select a source first", StatusTone::Warning);
            return;
        };
        let tx = self.runtime.jobs.message_sender();
        std::thread::spawn(move || {
            let result = super::analysis_jobs::enqueue_jobs_for_embedding_backfill(&source);
            match result {
                Ok((inserted, progress)) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::EmbeddingBackfillEnqueueFinished {
                            inserted,
                            progress,
                        },
                    ));
                }
                Err(err) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::EmbeddingBackfillEnqueueFailed(err),
                    ));
                }
            }
        });
    }

    pub fn has_any_sources(&self) -> bool {
        !self.library.sources.is_empty()
    }
}
