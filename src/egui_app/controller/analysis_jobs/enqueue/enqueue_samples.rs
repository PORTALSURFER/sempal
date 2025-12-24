use super::enqueue_helpers::{fast_content_hash, library_db_path, now_epoch_seconds};
use crate::egui_app::controller::analysis_jobs::db;
use crate::egui_app::controller::analysis_jobs::types::AnalysisProgress;
use rusqlite::params;

struct EnqueueSamplesRequest<'a> {
    source_id: &'a crate::sample_sources::SourceId,
    changed_samples: &'a [crate::sample_sources::scanner::ChangedSample],
}

pub(in crate::egui_app::controller) fn enqueue_jobs_for_source(
    source_id: &crate::sample_sources::SourceId,
    changed_samples: &[crate::sample_sources::scanner::ChangedSample],
) -> Result<(usize, AnalysisProgress), String> {
    let request = EnqueueSamplesRequest {
        source_id,
        changed_samples,
    };
    enqueue_samples(request)
}

fn enqueue_samples(
    request: EnqueueSamplesRequest<'_>,
) -> Result<(usize, AnalysisProgress), String> {
    if request.changed_samples.is_empty() {
        let db_path = library_db_path()?;
        let conn = db::open_library_db(&db_path)?;
        return Ok((0, db::current_progress(&conn)?));
    }
    let sample_metadata: Vec<db::SampleMetadata> = request
        .changed_samples
        .iter()
        .map(|sample| db::SampleMetadata {
            sample_id: db::build_sample_id(request.source_id.as_str(), &sample.relative_path),
            content_hash: sample.content_hash.clone(),
            size: sample.file_size,
            mtime_ns: sample.modified_ns,
        })
        .collect();
    let db_path = library_db_path()?;
    let mut conn = db::open_library_db(&db_path)?;
    let sample_ids: Vec<String> = sample_metadata
        .iter()
        .map(|sample| sample.sample_id.clone())
        .collect();
    let current_version = crate::analysis::version::analysis_version();
    let existing_states = db::sample_analysis_states(&conn, &sample_ids)?;
    db::upsert_samples(&mut conn, &sample_metadata)?;
    let mut invalidate = Vec::new();
    let mut jobs = Vec::new();
    for sample in &sample_metadata {
        let state = existing_states.get(&sample.sample_id);
        let hash_changed = state
            .map(|state| state.content_hash != sample.content_hash)
            .unwrap_or(true);
        let analysis_stale = state
            .and_then(|state| state.analysis_version.as_deref())
            .map(|version| version != current_version)
            .unwrap_or(true);
        if hash_changed || analysis_stale {
            invalidate.push(sample.sample_id.clone());
            jobs.push((sample.sample_id.clone(), sample.content_hash.clone()));
        }
    }
    if !invalidate.is_empty() {
        db::invalidate_analysis_artifacts(&mut conn, &invalidate)?;
    }

    let created_at = now_epoch_seconds();
    let inserted = db::enqueue_jobs(&mut conn, &jobs, db::ANALYZE_SAMPLE_JOB_TYPE, created_at)?;
    let progress = db::current_progress(&conn)?;
    Ok((inserted, progress))
}

struct EnqueueSourceRequest<'a> {
    source: &'a crate::sample_sources::SampleSource,
}

pub(in crate::egui_app::controller) fn enqueue_jobs_for_source_backfill(
    source: &crate::sample_sources::SampleSource,
) -> Result<(usize, AnalysisProgress), String> {
    let request = EnqueueSourceRequest { source };
    enqueue_source_backfill(request)
}

fn enqueue_source_backfill(
    request: EnqueueSourceRequest<'_>,
) -> Result<(usize, AnalysisProgress), String> {
    let db_path = library_db_path()?;
    let mut conn = db::open_library_db(&db_path)?;
    let prefix = format!("{}::%", request.source.id.as_str());
    let existing_jobs_total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM analysis_jobs WHERE sample_id LIKE ?1",
            params![&prefix],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if existing_jobs_total > 0 {
        let active_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM analysis_jobs WHERE sample_id LIKE ?1 AND status IN ('pending','running')",
                params![&prefix],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if active_jobs > 0 {
            return Ok((0, db::current_progress(&conn)?));
        }
    }
    let source_db = crate::sample_sources::SourceDatabase::open(&request.source.root)
        .map_err(|err| err.to_string())?;
    let entries = source_db.list_files().map_err(|err| err.to_string())?;
    if entries.is_empty() {
        return Ok((0, db::current_progress(&conn)?));
    }

    let mut staged_samples = Vec::with_capacity(entries.len());
    for entry in entries {
        let sample_id = db::build_sample_id(request.source.id.as_str(), &entry.relative_path);
        let content_hash = match entry.content_hash {
            Some(hash) if !hash.trim().is_empty() => hash,
            _ => fast_content_hash(entry.file_size, entry.modified_ns),
        };
        staged_samples.push(db::SampleMetadata {
            sample_id,
            content_hash,
            size: entry.file_size,
            mtime_ns: entry.modified_ns,
        });
    }
    if staged_samples.is_empty() {
        return Ok((0, db::current_progress(&conn)?));
    }
    stage_backfill_samples(&mut conn, &staged_samples)?;
    let (sample_metadata, jobs, invalidate) =
        collect_backfill_updates(&mut conn, db::ANALYZE_SAMPLE_JOB_TYPE)?;

    if !invalidate.is_empty() {
        db::invalidate_analysis_artifacts(&mut conn, &invalidate)?;
    }
    db::upsert_samples(&mut conn, &sample_metadata)?;

    let created_at = now_epoch_seconds();
    let inserted = db::enqueue_jobs(&mut conn, &jobs, db::ANALYZE_SAMPLE_JOB_TYPE, created_at)?;
    let progress = db::current_progress(&conn)?;
    Ok((inserted, progress))
}

struct EnqueueMissingFeaturesRequest<'a> {
    source: &'a crate::sample_sources::SampleSource,
}

pub(in crate::egui_app::controller) fn enqueue_jobs_for_source_missing_features(
    source: &crate::sample_sources::SampleSource,
) -> Result<(usize, AnalysisProgress), String> {
    let request = EnqueueMissingFeaturesRequest { source };
    enqueue_missing_features(request)
}

fn enqueue_missing_features(
    request: EnqueueMissingFeaturesRequest<'_>,
) -> Result<(usize, AnalysisProgress), String> {
    let db_path = library_db_path()?;
    let mut conn = db::open_library_db(&db_path)?;

    let source_db = crate::sample_sources::SourceDatabase::open(&request.source.root)
        .map_err(|err| err.to_string())?;
    let mut entries = source_db.list_files().map_err(|err| err.to_string())?;
    entries.retain(|entry| !entry.missing);
    if entries.is_empty() {
        return Ok((0, db::current_progress(&conn)?));
    }

    let mut staged_samples = Vec::new();
    for entry in entries {
        let sample_id = db::build_sample_id(request.source.id.as_str(), &entry.relative_path);
        let absolute = request.source.root.join(&entry.relative_path);
        if !absolute.exists() {
            if !entry.missing {
                let _ = source_db.set_missing(&entry.relative_path, true);
            }
            continue;
        }
        if entry.missing {
            let _ = source_db.set_missing(&entry.relative_path, false);
        }
        let content_hash = match entry.content_hash {
            Some(hash) if !hash.trim().is_empty() => hash,
            _ => fast_content_hash(entry.file_size, entry.modified_ns),
        };
        if content_hash.trim().is_empty() {
            continue;
        }
        staged_samples.push(db::SampleMetadata {
            sample_id,
            content_hash,
            size: entry.file_size,
            mtime_ns: entry.modified_ns,
        });
    }
    if staged_samples.is_empty() {
        return Ok((0, db::current_progress(&conn)?));
    }
    stage_backfill_samples(&mut conn, &staged_samples)?;
    let (sample_metadata, jobs, invalidate) =
        collect_backfill_updates(&mut conn, db::ANALYZE_SAMPLE_JOB_TYPE)?;
    if !invalidate.is_empty() {
        db::invalidate_analysis_artifacts(&mut conn, &invalidate)?;
    }

    if jobs.is_empty() {
        return Ok((0, db::current_progress(&conn)?));
    }
    db::upsert_samples(&mut conn, &sample_metadata)?;
    let created_at = now_epoch_seconds();
    let inserted = db::enqueue_jobs(&mut conn, &jobs, db::ANALYZE_SAMPLE_JOB_TYPE, created_at)?;
    let progress = db::current_progress(&conn)?;
    Ok((inserted, progress))
}

fn stage_backfill_samples(
    conn: &mut rusqlite::Connection,
    samples: &[db::SampleMetadata],
) -> Result<(), String> {
    prepare_backfill_staging(conn)?;
    let mut stmt = prepare_backfill_insert(conn)?;
    for sample in samples {
        let size = i64::try_from(sample.size)
            .map_err(|_| "Sample size exceeds storage limits".to_string())?;
        stmt.execute(params![&sample.sample_id, &sample.content_hash, size, sample.mtime_ns])
            .map_err(|err| format!("Insert backfill staging row failed: {err}"))?;
    }
    Ok(())
}

fn collect_backfill_updates(
    conn: &mut rusqlite::Connection,
    job_type: &str,
) -> Result<(Vec<db::SampleMetadata>, Vec<(String, String)>, Vec<String>), String> {
    let current_version = crate::analysis::version::analysis_version();
    let invalidate = fetch_backfill_invalidations(conn, current_version)?;
    let (sample_metadata, jobs) = fetch_backfill_jobs(
        conn,
        current_version,
        job_type,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
    )?;
    Ok((sample_metadata, jobs, invalidate))
}

fn prepare_backfill_staging(conn: &mut rusqlite::Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS temp_backfill_samples (
            sample_id TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            size INTEGER NOT NULL,
            mtime_ns INTEGER NOT NULL
        );
        DELETE FROM temp_backfill_samples;",
    )
    .map_err(|err| format!("Prepare backfill staging table failed: {err}"))?;
    Ok(())
}

fn prepare_backfill_insert(
    conn: &mut rusqlite::Connection,
) -> Result<rusqlite::Statement<'_>, String> {
    conn.prepare(
        "INSERT INTO temp_backfill_samples (sample_id, content_hash, size, mtime_ns)
         VALUES (?1, ?2, ?3, ?4)",
    )
    .map_err(|err| format!("Prepare backfill staging insert failed: {err}"))
}

fn fetch_backfill_invalidations(
    conn: &mut rusqlite::Connection,
    current_version: &str,
) -> Result<Vec<String>, String> {
    let mut invalidate = Vec::new();
    let mut stmt = conn
        .prepare(
            "SELECT t.sample_id
             FROM temp_backfill_samples t
             JOIN features f ON f.sample_id = t.sample_id AND f.feat_version = 1
             LEFT JOIN samples s ON s.sample_id = t.sample_id
             WHERE s.analysis_version IS NULL OR s.analysis_version != ?1",
        )
        .map_err(|err| format!("Prepare invalidate backfill query failed: {err}"))?;
    let mut rows = stmt
        .query(params![current_version])
        .map_err(|err| format!("Query invalidate backfill rows failed: {err}"))?;
    while let Some(row) = rows
        .next()
        .map_err(|err| format!("Query invalidate backfill rows failed: {err}"))?
    {
        let sample_id: String = row.get(0).map_err(|err| err.to_string())?;
        invalidate.push(sample_id);
    }
    Ok(invalidate)
}

fn fetch_backfill_jobs(
    conn: &mut rusqlite::Connection,
    current_version: &str,
    job_type: &str,
    model_id: &str,
) -> Result<(Vec<db::SampleMetadata>, Vec<(String, String)>), String> {
    let mut sample_metadata = Vec::new();
    let mut jobs = Vec::new();
    let mut stmt = conn
        .prepare(
            "SELECT t.sample_id, t.content_hash, t.size, t.mtime_ns
             FROM temp_backfill_samples t
             LEFT JOIN features f ON f.sample_id = t.sample_id AND f.feat_version = 1
             LEFT JOIN embeddings e ON e.sample_id = t.sample_id AND e.model_id = ?3
             LEFT JOIN samples s ON s.sample_id = t.sample_id
             LEFT JOIN analysis_jobs j ON j.sample_id = t.sample_id AND j.job_type = ?2
             WHERE (f.sample_id IS NULL OR e.sample_id IS NULL OR s.analysis_version IS NULL OR s.analysis_version != ?1)
               AND (j.status IS NULL OR j.status NOT IN ('pending','running'))",
        )
        .map_err(|err| format!("Prepare backfill job query failed: {err}"))?;
    let mut rows = stmt
        .query(params![current_version, job_type, model_id])
        .map_err(|err| format!("Query backfill job rows failed: {err}"))?;
    while let Some(row) = rows
        .next()
        .map_err(|err| format!("Query backfill job rows failed: {err}"))?
    {
        let sample_id: String = row.get(0).map_err(|err| err.to_string())?;
        let content_hash: String = row.get(1).map_err(|err| err.to_string())?;
        if content_hash.trim().is_empty() {
            continue;
        }
        let size: i64 = row.get(2).map_err(|err| err.to_string())?;
        let size = u64::try_from(size)
            .map_err(|_| "Sample size exceeds storage limits".to_string())?;
        let mtime_ns: i64 = row.get(3).map_err(|err| err.to_string())?;
        sample_metadata.push(db::SampleMetadata {
            sample_id: sample_id.clone(),
            content_hash: content_hash.clone(),
            size,
            mtime_ns,
        });
        jobs.push((sample_id, content_hash));
    }
    Ok((sample_metadata, jobs))
}
