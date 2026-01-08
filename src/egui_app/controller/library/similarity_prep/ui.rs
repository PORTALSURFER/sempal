use crate::egui_app::controller::EguiController;
use crate::egui_app::controller::ui::status_message::StatusMessage;
use crate::egui_app::controller::jobs;
use crate::sample_sources::SampleSource;

impl EguiController {
    pub(crate) fn show_similarity_prep_start(&mut self, source: &SampleSource) {
        self.runtime.similarity_prep_last_error = None;
        self.set_status_message(StatusMessage::PreparingSimilarity {
            source: source.root.display().to_string(),
        });
        self.show_similarity_prep_progress(0, false);
    }

    pub(crate) fn show_similarity_prep_finalizing(&mut self) {
        self.set_status_message(StatusMessage::FinalizingSimilarityPrep);
        self.show_similarity_finalize_progress();
        self.set_similarity_finalize_detail();
    }

    pub(crate) fn show_similarity_prep_ready(&mut self, outcome: &jobs::SimilarityPrepOutcome) {
        self.runtime.similarity_prep_last_error = None;
        self.ui.map.bounds = None;
        self.ui.map.last_query = None;
        self.ui.map.cached_cluster_centroids_key = None;
        self.ui.map.cached_cluster_centroids = None;
        self.ui.map.auto_cluster_build_requested_key = None;
        self.set_status_message(StatusMessage::SimilarityReady {
            cluster_count: outcome.cluster_stats.cluster_count,
            noise_ratio: outcome.cluster_stats.noise_ratio,
        });
    }

    pub(crate) fn show_similarity_prep_failed(&mut self, err: String) {
        self.runtime.similarity_prep_last_error = Some(err.clone());
        self.set_status_message(StatusMessage::SimilarityPrepFailed { err });
    }
}
