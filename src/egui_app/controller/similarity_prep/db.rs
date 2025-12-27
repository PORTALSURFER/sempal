use crate::egui_app::controller::analysis_jobs;
use crate::sample_sources::{SampleSource, SourceDatabase, SourceId};

pub(super) fn read_source_scan_timestamp(source: &SampleSource) -> Option<i64> {
    let db = SourceDatabase::open(&source.root).ok()?;
    db.get_metadata(crate::sample_sources::db::META_LAST_SCAN_COMPLETED_AT)
        .ok()
        .flatten()
        .and_then(|value| value.parse().ok())
}

pub(super) fn read_source_prep_timestamp(source: &SampleSource) -> Option<i64> {
    let db = SourceDatabase::open(&source.root).ok()?;
    db.get_metadata(crate::sample_sources::db::META_LAST_SIMILARITY_PREP_SCAN_AT)
        .ok()
        .flatten()
        .and_then(|value| value.parse().ok())
}

pub(super) fn record_similarity_prep_scan_timestamp(source: &SampleSource, scan_completed_at: i64) {
    if let Ok(db) = SourceDatabase::open(&source.root) {
        let _ = db.set_metadata(
            crate::sample_sources::db::META_LAST_SIMILARITY_PREP_SCAN_AT,
            &scan_completed_at.to_string(),
        );
    }
}

pub(super) fn source_has_embeddings(source: &SampleSource) -> bool {
    let Ok(conn) = analysis_jobs::open_source_db(&source.root) else {
        return false;
    };
    let model_id = crate::analysis::embedding::EMBEDDING_MODEL_ID;
    let sample_id_prefix = format!("{}::%", source.id.as_str());
    let count: Result<i64, _> = conn.query_row(
        "SELECT COUNT(*) FROM embeddings WHERE model_id = ?1 AND sample_id LIKE ?2",
        rusqlite::params![model_id, sample_id_prefix],
        |row| row.get(0),
    );
    count.map(|value| value > 0).unwrap_or(false)
}

pub(super) fn count_umap_layout_rows(
    conn: &rusqlite::Connection,
    model_id: &str,
    umap_version: &str,
    sample_id_prefix: &str,
) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM layout_umap
         WHERE model_id = ?1 AND umap_version = ?2 AND sample_id LIKE ?3",
        rusqlite::params![model_id, umap_version, sample_id_prefix],
        |row| row.get(0),
    )
    .map_err(|err| format!("Count layout rows failed: {err}"))
}

pub(super) fn open_source_db_for_similarity(
    source_id: &SourceId,
) -> Result<rusqlite::Connection, String> {
    let state = crate::sample_sources::library::load().map_err(|err| err.to_string())?;
    let source = state
        .sources
        .iter()
        .find(|source| &source.id == source_id)
        .ok_or_else(|| "Source not found for similarity prep".to_string())?;
    analysis_jobs::open_source_db(&source.root)
}
