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

    pub fn recompute_weak_labels_for_selected_source(&mut self) {
        let Some(source) = self.current_source() else {
            self.set_status("Select a source first", StatusTone::Warning);
            return;
        };
        let source_id_str = source.id.to_string();
        let tx = self.runtime.jobs.message_sender();
        std::thread::spawn(move || {
            let result = super::analysis_jobs::recompute_weak_labels_for_source(&source);
            match result {
                Ok(outcome) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::WeakLabelsRecomputed {
                            source_id: source_id_str,
                            processed: outcome.processed,
                            skipped: outcome.skipped,
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

    pub fn recompute_weak_labels_for_all_sources(&mut self) {
        if self.library.sources.is_empty() {
            self.set_status("No sources configured", StatusTone::Warning);
            return;
        }
        let sources_list = self.library.sources.clone();
        let sources = sources_list.len();
        let tx = self.runtime.jobs.message_sender();
        std::thread::spawn(move || {
            let result = super::analysis_jobs::recompute_weak_labels_for_sources(&sources_list);
            match result {
                Ok(outcome) => {
                    let _ = tx.send(super::jobs::JobMessage::Analysis(
                        super::analysis_jobs::AnalysisJobMessage::WeakLabelsRecomputedAll {
                            sources,
                            processed: outcome.processed,
                            skipped: outcome.skipped,
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

    pub fn has_any_sources(&self) -> bool {
        !self.library.sources.is_empty()
    }
}
