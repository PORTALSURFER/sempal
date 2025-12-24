use super::enqueue_helpers::{library_db_path, now_epoch_seconds};
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

fn enqueue_embedding_backfill(
    request: EnqueueEmbeddingBackfillRequest<'_>,
) -> Result<(usize, AnalysisProgress), String> {
    const BATCH_SIZE: usize = 32;

    let db_path = library_db_path()?;
    let mut conn = db::open_library_db(&db_path)?;

    let active_jobs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM analysis_jobs
             WHERE job_type = ?1 AND sample_id LIKE ?2 AND status IN ('pending','running')",
            params![
                db::EMBEDDING_BACKFILL_JOB_TYPE,
                format!("embed_backfill::{}::%", request.source.id)
            ],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if active_jobs > 0 {
        return Ok((0, db::current_progress(&conn)?));
    }

    let source_db = crate::sample_sources::SourceDatabase::open(&request.source.root)
        .map_err(|err| err.to_string())?;
    let mut entries = source_db.list_files().map_err(|err| err.to_string())?;
    entries.retain(|entry| !entry.missing);
    if entries.is_empty() {
        return Ok((0, db::current_progress(&conn)?));
    }

    let mut staged_ids = Vec::with_capacity(entries.len());
    for entry in entries {
        let sample_id = db::build_sample_id(request.source.id.as_str(), &entry.relative_path);
        staged_ids.push(sample_id);
    }
    if staged_ids.is_empty() {
        return Ok((0, db::current_progress(&conn)?));
    }
    stage_embedding_samples(&mut conn, &staged_ids)?;
    let mut sample_ids = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT t.sample_id
                 FROM temp_embedding_backfill_samples t
                 LEFT JOIN embeddings e
                   ON e.sample_id = t.sample_id AND e.model_id = ?1
                 WHERE e.sample_id IS NULL
                 ORDER BY t.sample_id",
            )
            .map_err(|err| format!("Prepare embedding backfill query failed: {err}"))?;
        let mut rows = stmt
            .query(params![crate::analysis::embedding::EMBEDDING_MODEL_ID])
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
        let job_id = format!("embed_backfill::{}::{}", request.source.id.as_str(), idx);
        let payload = serde_json::to_string(chunk)
            .map_err(|err| format!("Encode backfill payload: {err}"))?;
        jobs.push((job_id, payload));
    }
    let inserted = db::enqueue_jobs(
        &mut conn,
        &jobs,
        db::EMBEDDING_BACKFILL_JOB_TYPE,
        created_at,
    )?;
    let progress = db::current_progress(&conn)?;
    Ok((inserted, progress))
}

fn stage_embedding_samples(
    conn: &mut rusqlite::Connection,
    sample_ids: &[String],
) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS temp_embedding_backfill_samples (
            sample_id TEXT PRIMARY KEY
        );
        DELETE FROM temp_embedding_backfill_samples;",
    )
    .map_err(|err| format!("Prepare embedding staging table failed: {err}"))?;
    let mut stmt = conn
        .prepare("INSERT INTO temp_embedding_backfill_samples (sample_id) VALUES (?1)")
        .map_err(|err| format!("Prepare embedding staging insert failed: {err}"))?;
    for sample_id in sample_ids {
        stmt.execute(params![sample_id])
            .map_err(|err| format!("Insert embedding staging row failed: {err}"))?;
    }
    Ok(())
}
