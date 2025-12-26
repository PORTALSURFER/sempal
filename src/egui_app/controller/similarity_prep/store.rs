use super::db;
use crate::egui_app::controller::analysis_jobs;
use crate::sample_sources::{SampleSource, SourceId};

pub(super) trait SimilarityPrepStore {
    fn read_scan_timestamp(&self, source: &SampleSource) -> Option<i64>;
    fn read_prep_timestamp(&self, source: &SampleSource) -> Option<i64>;
    fn source_has_embeddings(&self, source: &SampleSource) -> bool;
    fn record_prep_scan_timestamp(&self, source: &SampleSource, scan_completed_at: i64);
    fn current_analysis_progress(
        &self,
        source: &SampleSource,
    ) -> Result<analysis_jobs::AnalysisProgress, String>;
    fn open_source_db_for_similarity(
        &self,
        source_id: &SourceId,
    ) -> Result<rusqlite::Connection, String>;
    fn count_umap_layout_rows(
        &self,
        conn: &rusqlite::Connection,
        model_id: &str,
        umap_version: &str,
        sample_id_prefix: &str,
    ) -> Result<i64, String>;
}

pub(super) struct DbSimilarityPrepStore;

impl SimilarityPrepStore for DbSimilarityPrepStore {
    fn read_scan_timestamp(&self, source: &SampleSource) -> Option<i64> {
        db::read_source_scan_timestamp(source)
    }

    fn read_prep_timestamp(&self, source: &SampleSource) -> Option<i64> {
        db::read_source_prep_timestamp(source)
    }

    fn source_has_embeddings(&self, source: &SampleSource) -> bool {
        db::source_has_embeddings(source)
    }

    fn record_prep_scan_timestamp(&self, source: &SampleSource, scan_completed_at: i64) {
        db::record_similarity_prep_scan_timestamp(source, scan_completed_at);
    }

    fn current_analysis_progress(
        &self,
        source: &SampleSource,
    ) -> Result<analysis_jobs::AnalysisProgress, String> {
        analysis_jobs::current_progress_for_source(source).map_err(|err| err.to_string())
    }

    fn open_source_db_for_similarity(
        &self,
        source_id: &SourceId,
    ) -> Result<rusqlite::Connection, String> {
        db::open_source_db_for_similarity(source_id)
    }

    fn count_umap_layout_rows(
        &self,
        conn: &rusqlite::Connection,
        model_id: &str,
        umap_version: &str,
        sample_id_prefix: &str,
    ) -> Result<i64, String> {
        db::count_umap_layout_rows(conn, model_id, umap_version, sample_id_prefix)
    }
}
