use super::plan::plan_similarity_prep_start;
use super::store::DbSimilarityPrepStore;
use crate::egui_app::controller::{analysis_jobs, EguiController, SimilarityPrepStage, StatusMessage};
use crate::egui_app::ui::style::StatusTone;

impl EguiController {
    pub fn prepare_similarity_for_selected_source(&mut self) {
        let force_full_analysis = self.runtime.similarity_prep_force_full_analysis_next;
        self.runtime.similarity_prep_force_full_analysis_next = false;
        self.prepare_similarity_for_selected_source_with_options(force_full_analysis);
    }

    pub fn prepare_similarity_for_selected_source_with_options(
        &mut self,
        force_full_analysis: bool,
    ) {
        self.runtime.similarity_prep_last_error = None;
        if let Err(err) = crate::analysis::embedding::warmup_panns() {
            tracing::warn!("PANNs warmup failed: {err}");
        }
        if self.runtime.similarity_prep.is_some() {
            self.refresh_similarity_prep_progress();
            self.set_status_message(StatusMessage::SimilarityPrepAlreadyRunning);
            return;
        }
        if self.runtime.jobs.scan_in_progress() {
            self.set_status_message(StatusMessage::SimilarityScanAlreadyRunning);
            return;
        }
        if self.runtime.jobs.umap_build_in_progress() {
            self.set_status_message(StatusMessage::TsneBuildAlreadyRunning);
            return;
        }
        let Some(source) = self.current_source() else {
            self.set_status_message(StatusMessage::SelectSourceFirst {
                tone: StatusTone::Warning,
            });
            return;
        };
        let store = DbSimilarityPrepStore;
        let plan = plan_similarity_prep_start(
            &store,
            &source,
            self.ui.map.umap_version.clone(),
            force_full_analysis,
        );
        self.runtime.similarity_prep = Some(plan.state);
        self.apply_similarity_prep_duration_cap();
        self.apply_similarity_prep_fast_mode();
        self.apply_similarity_prep_full_analysis(force_full_analysis);
        self.apply_similarity_prep_worker_boost();
        self.show_similarity_prep_start(&source);
        if plan.skip_scan {
            if plan.needs_embeddings || force_full_analysis {
                self.ensure_similarity_prep_progress(0, true);
                self.set_similarity_embedding_detail();
                self.enqueue_similarity_backfill(source, force_full_analysis);
            } else {
                self.set_similarity_analysis_detail();
                self.refresh_similarity_prep_progress();
            }
        } else {
            self.set_similarity_scan_detail();
            self.request_hard_sync();
        }
    }

    pub fn set_similarity_prep_force_full_analysis_next(&mut self, enabled: bool) {
        self.runtime.similarity_prep_force_full_analysis_next = enabled;
    }

    pub fn similarity_prep_force_full_analysis_next(&self) -> bool {
        self.runtime.similarity_prep_force_full_analysis_next
    }

    pub fn similarity_prep_in_progress(&self) -> bool {
        self.runtime.similarity_prep.is_some()
            || self.runtime.jobs.scan_in_progress()
            || self.runtime.jobs.umap_build_in_progress()
            || self.runtime.jobs.umap_cluster_build_in_progress()
    }

    pub fn similarity_prep_has_error(&self) -> bool {
        self.runtime.similarity_prep_last_error.is_some()
    }

    pub fn similarity_prep_is_finalizing(&self) -> bool {
        self.runtime
            .similarity_prep
            .as_ref()
            .is_some_and(|state| state.stage == SimilarityPrepStage::Finalizing)
    }

    pub fn similarity_prep_debug_snapshot(&self) -> String {
        let Some(state) = self.runtime.similarity_prep.as_ref() else {
            return "similarity_prep=idle".to_string();
        };
        let mut out = format!(
            "stage={:?} skip_backfill={} scan_in_progress={} umap_in_progress={} clusters_in_progress={}",
            state.stage,
            state.skip_backfill,
            self.runtime.jobs.scan_in_progress(),
            self.runtime.jobs.umap_build_in_progress(),
            self.runtime.jobs.umap_cluster_build_in_progress()
        );
        if let Some(source) = self.find_source_by_id(&state.source_id)
            && let Ok(progress) = analysis_jobs::current_progress_for_source(&source)
        {
            out.push_str(&format!(" analysis_progress={progress}"));
        }
        out
    }
}
