use super::store::{DbSimilarityPrepStore, SimilarityPrepStore};
use crate::egui_app::controller::{EguiController, SimilarityPrepStage};

impl EguiController {
    pub(super) fn refresh_similarity_prep_progress(&mut self) {
        let Some(state) = self.runtime.similarity_prep.as_ref() else {
            return;
        };
        let store = DbSimilarityPrepStore;
        match state.stage {
            SimilarityPrepStage::AwaitScan => {
                if self.runtime.jobs.scan_in_progress() {
                    self.ensure_similarity_prep_progress(0, false);
                    self.set_similarity_scan_detail();
                    return;
                }
                if self.ui.progress.visible {
                    return;
                }
                self.show_similarity_prep_progress(0, false);
                self.set_similarity_scan_detail();
            }
            SimilarityPrepStage::AwaitEmbeddings => {
                let Some(source) = self.find_source_by_id(&state.source_id) else {
                    return;
                };
                let progress = match store.current_analysis_progress(&source) {
                    Ok(progress) => progress,
                    Err(_) => {
                        if !self.ui.progress.visible {
                            self.show_similarity_prep_progress(0, false);
                            self.set_similarity_analysis_detail();
                        }
                        return;
                    }
                };
                if progress.pending == 0 && progress.running == 0 {
                    if !store.source_has_embeddings(&source) {
                        self.ensure_similarity_prep_progress(0, true);
                        self.set_similarity_embedding_detail();
                        self.enqueue_similarity_backfill(source, false);
                        return;
                    }
                    self.handle_similarity_analysis_progress(&progress);
                    return;
                }
                self.ensure_similarity_prep_progress(progress.total(), true);
                self.ui.progress.total = progress.total();
                self.ui.progress.completed = progress.completed();
                let jobs_completed = progress.completed();
                let jobs_total = progress.total();
                let samples_completed = progress.samples_completed();
                let samples_total = progress.samples_total;
                let mut detail = format!(
                    "Analyzing… Jobs {jobs_completed}/{jobs_total} • Samples {samples_completed}/{samples_total}"
                );
                if progress.failed > 0 {
                    detail.push_str(&format!(" • {} failed", progress.failed));
                }
                self.ui.progress.detail = Some(detail);
            }
            SimilarityPrepStage::Finalizing => {
                self.ensure_similarity_finalize_progress();
                self.set_similarity_finalize_detail();
            }
        }
    }
}
