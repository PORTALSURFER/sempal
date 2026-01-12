use super::plan::plan_similarity_prep_start;
use super::store::DbSimilarityPrepStore;
use super::state::SimilarityPrepStage;
use crate::egui_app::controller::EguiController;
use crate::egui_app::controller::ui::status_message::StatusMessage;
use crate::egui_app::ui::style::StatusTone;

impl EguiController {
    /// Run similarity prep for the selected source using current settings.
    pub fn prepare_similarity_for_selected_source(&mut self) {
        let force_full_analysis = self.runtime.similarity_prep_force_full_analysis_next;
        self.runtime.similarity_prep_force_full_analysis_next = false;
        self.prepare_similarity_for_selected_source_with_options(force_full_analysis);
    }

    /// Run similarity prep for the selected source with explicit options.
    pub fn prepare_similarity_for_selected_source_with_options(
        &mut self,
        force_full_analysis: bool,
    ) {
        self.runtime.similarity_prep_last_error = None;
        
        // Cooldown to prevent rapid repeated attempts (e.g., from map view every frame)
        const SIMILARITY_PREP_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(5);
        if let Some(last_attempt) = self.runtime.similarity_prep_last_attempt {
            if last_attempt.elapsed() < SIMILARITY_PREP_COOLDOWN {
                return; // Too soon, skip
            }
        }
        self.runtime.similarity_prep_last_attempt = Some(std::time::Instant::now());
        
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
            self.ensure_similarity_prep_progress(0, true);
            self.set_similarity_embedding_detail();
            self.enqueue_similarity_backfill(source, force_full_analysis);
        } else {
            self.set_similarity_analysis_detail();
            self.refresh_similarity_prep_progress();
            if !force_full_analysis {
                self.set_status_message(StatusMessage::SimilarityAlreadyUpToDate);
            }
        }
    }

    /// Toggle forcing a full analysis on the next similarity prep run.
    pub fn set_similarity_prep_force_full_analysis_next(&mut self, enabled: bool) {
        self.runtime.similarity_prep_force_full_analysis_next = enabled;
    }

    /// Return whether the next similarity prep will force full analysis.
    pub fn similarity_prep_force_full_analysis_next(&self) -> bool {
        self.runtime.similarity_prep_force_full_analysis_next
    }

    /// Return true when similarity prep or related jobs are running.
    pub fn similarity_prep_in_progress(&self) -> bool {
        self.runtime.similarity_prep.is_some()
            || self.runtime.jobs.scan_in_progress()
            || self.runtime.jobs.umap_build_in_progress()
            || self.runtime.jobs.umap_cluster_build_in_progress()
    }

    /// Return true if the last similarity prep run recorded an error.
    pub fn similarity_prep_has_error(&self) -> bool {
        self.runtime.similarity_prep_last_error.is_some()
    }

    /// Return true if similarity prep is in the finalizing stage.
    pub fn similarity_prep_is_finalizing(&self) -> bool {
        self.runtime
            .similarity_prep
            .as_ref()
            .is_some_and(|state| state.stage == SimilarityPrepStage::Finalizing)
    }

    /// Return a debug summary of the current similarity prep state.
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
            && let Ok(progress) =
                crate::egui_app::controller::library::analysis_jobs::current_progress_for_source(&source)
        {
            out.push_str(&format!(" analysis_progress={progress}"));
        }
        out
    }
}
