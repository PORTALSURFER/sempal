use super::state;
use super::store::DbSimilarityPrepStore;
use crate::egui_app::controller::{analysis_jobs, jobs, EguiController, SimilarityPrepStage, SimilarityPrepState};
use crate::egui_app::state::ProgressTaskKind;
use crate::sample_sources::SourceId;

fn clear_similarity_prep_state(state: &mut Option<SimilarityPrepState>) -> bool {
    if state.is_some() {
        *state = None;
        true
    } else {
        false
    }
}

fn matches_similarity_stage(
    state: &Option<SimilarityPrepState>,
    source_id: &SourceId,
    stage: SimilarityPrepStage,
) -> bool {
    state
        .as_ref()
        .is_some_and(|entry| entry.source_id == *source_id && entry.stage == stage)
}

impl EguiController {
    pub(in crate::egui_app::controller) fn handle_similarity_scan_finished(
        &mut self,
        source_id: &SourceId,
        scan_changed: bool,
    ) {
        if !matches_similarity_stage(
            &self.runtime.similarity_prep,
            source_id,
            SimilarityPrepStage::AwaitScan,
        ) {
            return;
        }
        if let Some(source) = self.find_source_by_id(source_id) {
            let store = DbSimilarityPrepStore;
            let scan_completed_at = store.read_scan_timestamp(&source);
            let transition = if let Some(state) = self.runtime.similarity_prep.as_mut() {
                state::apply_scan_finished(state, scan_completed_at, scan_changed)
            } else {
                return;
            };
            if transition.should_enqueue_embeddings {
                self.ensure_similarity_prep_progress(0, true);
                self.set_similarity_embedding_detail();
                self.enqueue_similarity_backfill(source, transition.force_full);
            } else {
                self.refresh_similarity_prep_progress();
            }
        }
    }

    pub(in crate::egui_app::controller) fn handle_similarity_analysis_progress(
        &mut self,
        progress: &analysis_jobs::AnalysisProgress,
    ) {
        if progress.pending > 0 || progress.running > 0 {
            return;
        }
        let (source_id, umap_version) = {
            let Some(state) = self.runtime.similarity_prep.as_mut() else {
                return;
            };
            let Some(request) = state::start_finalize_if_ready(state) else {
                return;
            };
            (request.source_id, request.umap_version)
        };
        self.show_similarity_prep_finalizing();
        self.start_similarity_finalize(source_id, umap_version);
    }

    pub(in crate::egui_app::controller) fn handle_similarity_prep_result(
        &mut self,
        result: jobs::SimilarityPrepResult,
    ) {
        let state = self.runtime.similarity_prep.take();
        if state.as_ref().map(|s| &s.source_id) != Some(&result.source_id) {
            return;
        }
        self.restore_similarity_prep_duration_cap();
        self.restore_similarity_prep_fast_mode();
        self.restore_similarity_prep_full_analysis();
        self.restore_similarity_prep_worker_count();
        if self.ui.progress.task == Some(ProgressTaskKind::Analysis) {
            self.clear_progress();
        }
        match result.result {
            Ok(outcome) => {
                if let Some(scan_completed_at) = state.as_ref().and_then(|s| s.scan_completed_at) {
                    if let Some(source) = self.find_source_by_id(&result.source_id) {
                        let store = DbSimilarityPrepStore;
                        store.record_prep_scan_timestamp(&source, scan_completed_at);
                    }
                }
                self.show_similarity_prep_ready(&outcome);
            }
            Err(err) => {
                self.show_similarity_prep_failed(err);
            }
        }
    }

    pub(in crate::egui_app::controller) fn cancel_similarity_prep(
        &mut self,
        source_id: &SourceId,
    ) {
        let matches = self
            .runtime
            .similarity_prep
            .as_ref()
            .is_some_and(|state| &state.source_id == source_id);
        if !matches {
            return;
        }
        clear_similarity_prep_state(&mut self.runtime.similarity_prep);
        self.restore_similarity_prep_duration_cap();
        self.restore_similarity_prep_fast_mode();
        self.restore_similarity_prep_full_analysis();
        self.restore_similarity_prep_worker_count();
        if self.ui.progress.task == Some(ProgressTaskKind::Analysis) {
            self.clear_progress();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_state_reports_reset() {
        let mut state = None;
        assert!(!clear_similarity_prep_state(&mut state));
        state = Some(state::build_initial_state(state::SimilarityPrepInit {
            source_id: SourceId::new(),
            umap_version: "v1".to_string(),
            scan_completed_at: None,
            skip_scan: false,
            needs_embeddings: false,
            force_full_analysis: false,
        }));
        assert!(clear_similarity_prep_state(&mut state));
        assert!(state.is_none());
    }
}
