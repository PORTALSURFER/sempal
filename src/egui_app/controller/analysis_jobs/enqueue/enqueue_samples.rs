use super::enqueue_helpers::{fast_content_hash, library_db_path, now_epoch_seconds};
use crate::egui_app::controller::analysis_jobs::db;
use crate::egui_app::controller::analysis_jobs::types::AnalysisProgress;
use rusqlite::{OptionalExtension, params};

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
    let jobs: Vec<(String, String)> = sample_metadata
        .iter()
        .map(|sample| (sample.sample_id.clone(), sample.content_hash.clone()))
        .collect();
    let db_path = library_db_path()?;
    let mut conn = db::open_library_db(&db_path)?;
    db::upsert_samples(&mut conn, &sample_metadata)?;
    let sample_ids: Vec<String> = sample_metadata
        .iter()
        .map(|sample| sample.sample_id.clone())
        .collect();
    db::invalidate_analysis_artifacts(&mut conn, &sample_ids)?;

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

    let (sample_metadata, jobs, invalidate) = {
        let mut features_stmt = conn
            .prepare("SELECT 1 FROM features WHERE sample_id = ?1 AND feat_version = 1 LIMIT 1")
            .map_err(|err| format!("Prepare feature lookup failed: {err}"))?;
        let mut version_stmt = conn
            .prepare("SELECT analysis_version FROM samples WHERE sample_id = ?1")
            .map_err(|err| format!("Prepare analysis version lookup failed: {err}"))?;
        let mut job_stmt = conn
            .prepare(
                "SELECT status FROM analysis_jobs WHERE sample_id = ?1 AND job_type = ?2 LIMIT 1",
            )
            .map_err(|err| format!("Prepare job lookup failed: {err}"))?;

        let mut sample_metadata = Vec::with_capacity(entries.len());
        let mut jobs = Vec::with_capacity(entries.len());
        let mut invalidate = Vec::new();

        for entry in entries {
            let sample_id = db::build_sample_id(request.source.id.as_str(), &entry.relative_path);
            let has_features: Option<i64> = features_stmt
                .query_row(params![&sample_id], |row| row.get(0))
                .optional()
                .map_err(|err| format!("Feature lookup failed: {err}"))?;
            let analysis_version: Option<String> = version_stmt
                .query_row(params![&sample_id], |row| row.get::<_, Option<String>>(0))
                .optional()
                .map_err(|err| format!("Analysis version lookup failed: {err}"))?
                .flatten();
            let has_current_analysis = matches!(
                analysis_version.as_deref(),
                Some(version) if version == crate::analysis::version::analysis_version()
            );
            if has_features.is_some() && has_current_analysis {
                continue;
            }
            if has_features.is_some() && !has_current_analysis {
                invalidate.push(sample_id.clone());
            }
            let status: Option<String> = job_stmt
                .query_row(params![&sample_id, db::ANALYZE_SAMPLE_JOB_TYPE], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(|err| format!("Job lookup failed: {err}"))?;
            if matches!(status.as_deref(), Some("pending") | Some("running")) {
                continue;
            }

            let content_hash = match entry.content_hash {
                Some(hash) if !hash.trim().is_empty() => hash,
                _ => fast_content_hash(entry.file_size, entry.modified_ns),
            };
            sample_metadata.push(db::SampleMetadata {
                sample_id: sample_id.clone(),
                content_hash: content_hash.clone(),
                size: entry.file_size,
                mtime_ns: entry.modified_ns,
            });
            jobs.push((sample_id.clone(), content_hash));
        }

        (sample_metadata, jobs, invalidate)
    };

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

    let (sample_metadata, jobs, invalidate) = {
        let mut features_stmt = conn
            .prepare("SELECT 1 FROM features WHERE sample_id = ?1 AND feat_version = 1 LIMIT 1")
            .map_err(|err| format!("Prepare feature lookup failed: {err}"))?;
        let mut version_stmt = conn
            .prepare("SELECT analysis_version FROM samples WHERE sample_id = ?1")
            .map_err(|err| format!("Prepare analysis version lookup failed: {err}"))?;
        let mut job_stmt = conn
            .prepare(
                "SELECT status FROM analysis_jobs WHERE sample_id = ?1 AND job_type = ?2 LIMIT 1",
            )
            .map_err(|err| format!("Prepare job lookup failed: {err}"))?;

        let mut sample_metadata = Vec::new();
        let mut jobs = Vec::new();
        let mut invalidate = Vec::new();

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
            let has_features: Option<i64> = features_stmt
                .query_row(params![&sample_id], |row| row.get(0))
                .optional()
                .map_err(|err| format!("Feature lookup failed: {err}"))?;
            let analysis_version: Option<String> = version_stmt
                .query_row(params![&sample_id], |row| row.get::<_, Option<String>>(0))
                .optional()
                .map_err(|err| format!("Analysis version lookup failed: {err}"))?
                .flatten();
            let has_current_analysis = matches!(
                analysis_version.as_deref(),
                Some(version) if version == crate::analysis::version::analysis_version()
            );
            if has_features.is_some() && has_current_analysis {
                continue;
            }
            if has_features.is_some() && !has_current_analysis {
                invalidate.push(sample_id.clone());
            }
            let status: Option<String> = job_stmt
                .query_row(params![&sample_id, db::ANALYZE_SAMPLE_JOB_TYPE], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(|err| format!("Job lookup failed: {err}"))?;
            if matches!(status.as_deref(), Some("pending") | Some("running")) {
                continue;
            }

            let content_hash = match entry.content_hash {
                Some(hash) if !hash.trim().is_empty() => hash,
                _ => fast_content_hash(entry.file_size, entry.modified_ns),
            };
            if content_hash.trim().is_empty() {
                continue;
            }

            sample_metadata.push(db::SampleMetadata {
                sample_id: sample_id.clone(),
                content_hash: content_hash.clone(),
                size: entry.file_size,
                mtime_ns: entry.modified_ns,
            });
            jobs.push((sample_id.clone(), content_hash));
        }
        (sample_metadata, jobs, invalidate)
    };
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
