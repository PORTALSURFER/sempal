use super::state;
use super::store::SimilarityPrepStore;
use crate::egui_app::controller::SimilarityPrepState;
use crate::sample_sources::SampleSource;

pub(super) struct SimilarityPrepStartPlan {
    pub(super) needs_embeddings: bool,
    pub(super) skip_scan: bool,
    pub(super) state: SimilarityPrepState,
}

pub(super) fn plan_similarity_prep_start(
    store: &impl SimilarityPrepStore,
    source: &SampleSource,
    umap_version: String,
    force_full_analysis: bool,
) -> SimilarityPrepStartPlan {
    let scan_completed_at = store.read_scan_timestamp(source);
    let prep_scan_at = store.read_prep_timestamp(source);
    let skip_scan = scan_completed_at.is_some() && scan_completed_at == prep_scan_at;
    let needs_embeddings = !store.source_has_embeddings(source);
    let state = state::build_initial_state(state::SimilarityPrepInit {
        source_id: source.id.clone(),
        umap_version,
        scan_completed_at,
        skip_scan,
        needs_embeddings,
        force_full_analysis,
    });
    SimilarityPrepStartPlan {
        needs_embeddings,
        skip_scan,
        state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_app::controller::analysis_jobs;
    use crate::sample_sources::SourceId;
    use std::path::PathBuf;

    struct FakeStore {
        scan_completed_at: Option<i64>,
        prep_completed_at: Option<i64>,
        has_embeddings: bool,
    }

    impl SimilarityPrepStore for FakeStore {
        fn read_scan_timestamp(&self, _source: &SampleSource) -> Option<i64> {
            self.scan_completed_at
        }

        fn read_prep_timestamp(&self, _source: &SampleSource) -> Option<i64> {
            self.prep_completed_at
        }

        fn source_has_embeddings(&self, _source: &SampleSource) -> bool {
            self.has_embeddings
        }

        fn record_prep_scan_timestamp(&self, _source: &SampleSource, _scan_completed_at: i64) {}

        fn current_analysis_progress(
            &self,
            _source: &SampleSource,
        ) -> Result<analysis_jobs::AnalysisProgress, String> {
            Err("not needed".to_string())
        }

        fn current_embedding_backfill_progress(
            &self,
            _source: &SampleSource,
        ) -> Result<analysis_jobs::AnalysisProgress, String> {
            Err("not needed".to_string())
        }

        fn open_source_db_for_similarity(
            &self,
            _source_id: &SourceId,
        ) -> Result<rusqlite::Connection, String> {
            Err("not needed".to_string())
        }

        fn count_umap_layout_rows(
            &self,
            _conn: &rusqlite::Connection,
            _model_id: &str,
            _umap_version: &str,
            _sample_id_prefix: &str,
        ) -> Result<i64, String> {
            Err("not needed".to_string())
        }
    }

    fn sample_source() -> SampleSource {
        SampleSource::new(PathBuf::from("/tmp/source"))
    }

    #[test]
    fn plan_initial_state_tracks_skip_and_backfill() {
        let store = FakeStore {
            scan_completed_at: Some(10),
            prep_completed_at: Some(10),
            has_embeddings: true,
        };
        let plan = plan_similarity_prep_start(&store, &sample_source(), "v1".to_string(), false);
        assert!(plan.skip_scan);
        assert!(plan.state.skip_backfill);
        assert!(!plan.needs_embeddings);

        let store = FakeStore {
            scan_completed_at: Some(10),
            prep_completed_at: Some(10),
            has_embeddings: false,
        };
        let plan = plan_similarity_prep_start(&store, &sample_source(), "v1".to_string(), false);
        assert!(plan.skip_scan);
        assert!(!plan.state.skip_backfill);
        assert!(plan.needs_embeddings);
    }
}
