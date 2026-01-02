use super::enqueue_helpers::now_epoch_seconds;
use crate::egui_app::controller::analysis_jobs::db;
use crate::egui_app::controller::analysis_jobs::types::AnalysisProgress;
use rusqlite::params;

struct EnqueueEmbeddingBackfillRequest<'a> {
    source: &'a crate::sample_sources::SampleSource,
}

pub(in crate::egui_app::controller) fn enqueue_jobs_for_embedding_backfill(
    source: &crate::sample_sources::SampleSource,
) -> Result<(usize, AnalysisProgress), String> {
    let request = EnqueueEmbeddingBackfillRequest { source };
    enqueue_embedding_backfill(request)
}

pub(in crate::egui_app::controller) fn enqueue_jobs_for_embedding_samples(
    source: &crate::sample_sources::SampleSource,
    sample_ids: &[String],
) -> Result<(usize, AnalysisProgress), String> {
    if sample_ids.is_empty() {
        let conn = db::open_source_db(&source.root)?;
        return Ok((0, db::current_progress(&conn)?));
    }

    const BATCH_SIZE: usize = 32;
    let mut conn = db::open_source_db(&source.root)?;
    let created_at = now_epoch_seconds();
    let mut jobs = Vec::new();
    for (idx, chunk) in sample_ids.chunks(BATCH_SIZE).enumerate() {
        let job_id = format!("{}::embed_backfill::manual::{}", source.id.as_str(), idx);
        let payload = serde_json::to_string(chunk)
            .map_err(|err| format!("Encode backfill payload: {err}"))?;
        jobs.push((job_id, payload));
    }
    let inserted = db::enqueue_jobs(
        &mut conn,
        &jobs,
        db::EMBEDDING_BACKFILL_JOB_TYPE,
        created_at,
        source.id.as_str(),
    )?;
    let progress = db::current_progress(&conn)?;
    Ok((inserted, progress))
}

fn enqueue_embedding_backfill(
    request: EnqueueEmbeddingBackfillRequest<'_>,
) -> Result<(usize, AnalysisProgress), String> {
    const BATCH_SIZE: usize = 32;

    let mut conn = db::open_source_db(&request.source.root)?;

    let active_jobs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM analysis_jobs
             WHERE job_type = ?1 AND source_id = ?2 AND status IN ('pending','running')",
            params![
                db::EMBEDDING_BACKFILL_JOB_TYPE,
                request.source.id.as_str()
            ],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if active_jobs > 0 {
        return Ok((0, db::current_progress(&conn)?));
    }

    let mut sample_ids = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT s.sample_id
                 FROM samples s
                 LEFT JOIN embeddings e
                   ON e.sample_id = s.sample_id AND e.model_id = ?1
                 WHERE s.sample_id LIKE ?2
                   AND e.sample_id IS NULL
                 ORDER BY s.sample_id",
            )
            .map_err(|err| format!("Prepare embedding backfill query failed: {err}"))?;
        let mut rows = stmt
            .query(params![
                crate::analysis::embedding::EMBEDDING_MODEL_ID,
                format!("{}::%", request.source.id)
            ])
            .map_err(|err| format!("Failed to query embedding backfill rows: {err}"))?;
        while let Some(row) = rows
            .next()
            .map_err(|err| format!("Failed to query embedding backfill rows: {err}"))?
        {
            let sample_id: String = row.get(0).map_err(|err| err.to_string())?;
            sample_ids.push(sample_id);
        }
    }

    if sample_ids.is_empty() {
        return Ok((0, db::current_progress(&conn)?));
    }

    let created_at = now_epoch_seconds();
    let mut jobs = Vec::new();
    for (idx, chunk) in sample_ids.chunks(BATCH_SIZE).enumerate() {
        let job_id = format!("{}::embed_backfill::{}", request.source.id.as_str(), idx);
        let payload = serde_json::to_string(chunk)
            .map_err(|err| format!("Encode backfill payload: {err}"))?;
        jobs.push((job_id, payload));
    }
    let inserted = db::enqueue_jobs(
        &mut conn,
        &jobs,
        db::EMBEDDING_BACKFILL_JOB_TYPE,
        created_at,
        request.source.id.as_str(),
    )?;
    let progress = db::current_progress(&conn)?;
    Ok((inserted, progress))
}
