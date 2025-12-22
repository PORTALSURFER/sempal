use super::db;
use super::types::AnalysisProgress;
use rusqlite::types::Value;
use rusqlite::{OptionalExtension, params, params_from_iter};
use serde_json;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(in crate::egui_app::controller) fn enqueue_jobs_for_source(
    source_id: &crate::sample_sources::SourceId,
    changed_samples: &[crate::sample_sources::scanner::ChangedSample],
) -> Result<(usize, AnalysisProgress), String> {
    if changed_samples.is_empty() {
        let db_path = library_db_path()?;
        let conn = db::open_library_db(&db_path)?;
        return Ok((0, db::current_progress(&conn)?));
    }
    let sample_metadata: Vec<db::SampleMetadata> = changed_samples
        .iter()
        .map(|sample| db::SampleMetadata {
            sample_id: db::build_sample_id(source_id.as_str(), &sample.relative_path),
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

pub(in crate::egui_app::controller) fn enqueue_jobs_for_source_backfill(
    source: &crate::sample_sources::SampleSource,
) -> Result<(usize, AnalysisProgress), String> {
    let db_path = library_db_path()?;
    let mut conn = db::open_library_db(&db_path)?;
    let prefix = format!("{}::%", source.id.as_str());
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
    let source_db =
        crate::sample_sources::SourceDatabase::open(&source.root).map_err(|err| err.to_string())?;
    let entries = source_db.list_files().map_err(|err| err.to_string())?;
    if entries.is_empty() {
        return Ok((0, db::current_progress(&conn)?));
    }

    let (sample_metadata, jobs, invalidate) = {
        let mut features_stmt = conn
            .prepare(
                "SELECT 1 FROM features WHERE sample_id = ?1 AND feat_version = 1 LIMIT 1",
            )
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
            let sample_id = db::build_sample_id(source.id.as_str(), &entry.relative_path);
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
                .query_row(params![&sample_id, db::ANALYZE_SAMPLE_JOB_TYPE], |row| row.get(0))
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

pub(in crate::egui_app::controller) fn enqueue_jobs_for_source_missing_features(
    source: &crate::sample_sources::SampleSource,
) -> Result<(usize, AnalysisProgress), String> {
    let db_path = library_db_path()?;
    let mut conn = db::open_library_db(&db_path)?;

    let source_db =
        crate::sample_sources::SourceDatabase::open(&source.root).map_err(|err| err.to_string())?;
    let mut entries = source_db.list_files().map_err(|err| err.to_string())?;
    entries.retain(|entry| !entry.missing);
    if entries.is_empty() {
        return Ok((0, db::current_progress(&conn)?));
    }

    let (sample_metadata, jobs, invalidate) = {
        let mut features_stmt = conn
            .prepare(
                "SELECT 1 FROM features WHERE sample_id = ?1 AND feat_version = 1 LIMIT 1",
            )
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
            let sample_id = db::build_sample_id(source.id.as_str(), &entry.relative_path);
            let absolute = source.root.join(&entry.relative_path);
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
                .query_row(params![&sample_id, db::ANALYZE_SAMPLE_JOB_TYPE], |row| row.get(0))
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

pub(in crate::egui_app::controller) fn enqueue_jobs_for_embedding_backfill(
    source: &crate::sample_sources::SampleSource,
) -> Result<(usize, AnalysisProgress), String> {
    const BATCH_SIZE: usize = 32;

    let db_path = library_db_path()?;
    let mut conn = db::open_library_db(&db_path)?;

    let active_jobs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM analysis_jobs
             WHERE job_type = ?1 AND sample_id LIKE ?2 AND status IN ('pending','running')",
            params![db::EMBEDDING_BACKFILL_JOB_TYPE, format!("embed_backfill::{}::%", source.id)],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if active_jobs > 0 {
        return Ok((0, db::current_progress(&conn)?));
    }

    let sample_ids = {
        let mut stmt = conn
            .prepare(
                "SELECT s.sample_id
                 FROM samples s
                 LEFT JOIN embeddings e ON e.sample_id = s.sample_id
                 WHERE s.sample_id LIKE ?1
                   AND (e.sample_id IS NULL OR e.model_id != ?2)
                 ORDER BY s.sample_id ASC",
            )
            .map_err(|err| format!("Prepare embedding backfill query failed: {err}"))?;
        let mut sample_ids = Vec::new();
        let rows = stmt
            .query_map(
                params![
                    format!("{}::%", source.id.as_str()),
                    crate::analysis::embedding::EMBEDDING_MODEL_ID
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|err| format!("Failed to query embedding backfill rows: {err}"))?;
        for row in rows {
            sample_ids.push(row.map_err(|err| format!("Failed to decode sample_id: {err}"))?);
        }
        sample_ids
    };

    if sample_ids.is_empty() {
        return Ok((0, db::current_progress(&conn)?));
    }

    let created_at = now_epoch_seconds();
    let mut jobs = Vec::new();
    for (idx, chunk) in sample_ids.chunks(BATCH_SIZE).enumerate() {
        let job_id = format!("embed_backfill::{}::{}", source.id.as_str(), idx);
        let payload =
            serde_json::to_string(chunk).map_err(|err| format!("Encode backfill payload: {err}"))?;
        jobs.push((job_id, payload));
    }
    let inserted = db::enqueue_jobs(&mut conn, &jobs, db::EMBEDDING_BACKFILL_JOB_TYPE, created_at)?;
    let progress = db::current_progress(&conn)?;
    Ok((inserted, progress))
}

pub(in crate::egui_app::controller) fn enqueue_inference_jobs_for_sources(
    source_ids: &[String],
    preferred_model_id: Option<&str>,
) -> Result<(usize, AnalysisProgress), String> {
    let db_path = library_db_path()?;
    let mut conn = db::open_library_db(&db_path)?;
    super::inference::ensure_bundled_model(&conn)?;

    let model_id = resolve_model_id(&conn, preferred_model_id)?;
    let Some(model_id) = model_id else {
        return Ok((0, db::current_progress(&conn)?));
    };

    let jobs = collect_inference_jobs(&conn, &model_id, Some(source_ids))?;
    if jobs.is_empty() {
        return Ok((0, db::current_progress(&conn)?));
    }

    let created_at = now_epoch_seconds();
    let inserted = db::enqueue_jobs(&mut conn, &jobs, db::INFERENCE_JOB_TYPE, created_at)?;
    let progress = db::current_progress(&conn)?;
    Ok((inserted, progress))
}

pub(in crate::egui_app::controller) fn enqueue_inference_jobs_for_all_features(
    preferred_model_id: Option<&str>,
) -> Result<(usize, AnalysisProgress), String> {
    let db_path = library_db_path()?;
    let mut conn = db::open_library_db(&db_path)?;
    super::inference::ensure_bundled_model(&conn)?;

    let model_id = resolve_model_id(&conn, preferred_model_id)?;
    let Some(model_id) = model_id else {
        return Ok((0, db::current_progress(&conn)?));
    };

    let jobs = collect_inference_jobs(&conn, &model_id, None)?;
    if jobs.is_empty() {
        return Ok((0, db::current_progress(&conn)?));
    }

    let created_at = now_epoch_seconds();
    let inserted = db::enqueue_jobs(&mut conn, &jobs, db::INFERENCE_JOB_TYPE, created_at)?;
    let progress = db::current_progress(&conn)?;
    Ok((inserted, progress))
}

fn resolve_model_id(
    conn: &rusqlite::Connection,
    preferred_model_id: Option<&str>,
) -> Result<Option<String>, String> {
    if let Some(model_id) = preferred_model_id {
        let exists: Option<String> = conn
            .query_row(
                "SELECT model_id FROM models WHERE model_id = ?1",
                params![model_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| format!("Failed to query preferred model id: {err}"))?;
        if exists.is_some() {
            return Ok(Some(model_id.to_string()));
        }
    }
    let latest_model_id: Option<String> = conn
        .query_row(
            "SELECT model_id
             FROM models
             ORDER BY created_at DESC, model_id DESC
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| format!("Failed to query latest model id: {err}"))?;
    Ok(latest_model_id)
}

fn library_db_path() -> Result<std::path::PathBuf, String> {
    let dir = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(dir.join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME))
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64
}

fn compute_content_hash(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .map_err(|err| format!("Failed to open {}: {err}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn fast_content_hash(size: u64, modified_ns: i64) -> String {
    format!("fast-{}-{}", size, modified_ns)
}

fn collect_inference_jobs(
    conn: &rusqlite::Connection,
    model_id: &str,
    source_ids: Option<&[String]>,
) -> Result<Vec<(String, String)>, String> {
    let kind: Option<String> = conn
        .query_row(
            "SELECT kind FROM models WHERE model_id = ?1",
            params![model_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| format!("Failed to query model kind: {err}"))?;

    let (mut sql, mut params_vec, sample_alias) = if matches!(
        kind.as_deref(),
        Some("logreg_v1") | Some("mlp_v1")
    ) {
        (
            String::from(
                "SELECT e.sample_id, s.content_hash
                 FROM embeddings e
                 JOIN samples s ON s.sample_id = e.sample_id
                 LEFT JOIN predictions p
                   ON p.sample_id = e.sample_id AND p.model_id = ?1
                 WHERE e.model_id = ?2
                   AND (p.sample_id IS NULL OR p.content_hash != s.content_hash)",
            ),
            vec![
                Value::Text(model_id.to_string()),
                Value::Text(crate::analysis::embedding::EMBEDDING_MODEL_ID.to_string()),
            ],
            "e",
        )
    } else {
        (
            String::from(
                "SELECT f.sample_id, s.content_hash
                 FROM features f
                 JOIN samples s ON s.sample_id = f.sample_id
                 LEFT JOIN predictions p
                   ON p.sample_id = f.sample_id AND p.model_id = ?1
                 WHERE f.feat_version = ?2
                   AND (p.sample_id IS NULL OR p.content_hash != s.content_hash)",
            ),
            vec![
                Value::Text(model_id.to_string()),
                Value::Integer(crate::analysis::FEATURE_VERSION_V1),
            ],
            "f",
        )
    };

    if let Some(source_ids) = source_ids
        && !source_ids.is_empty()
    {
        sql.push_str(" AND (");
        for (idx, source_id) in source_ids.iter().enumerate() {
            if idx > 0 {
                sql.push_str(" OR ");
            }
            sql.push_str(sample_alias);
            sql.push_str(".sample_id LIKE ?");
            sql.push_str(&(params_vec.len() + 1).to_string());
            params_vec.push(Value::Text(format!("{source_id}::%")));
        }
        sql.push(')');
    }
    sql.push_str(" ORDER BY ");
    sql.push_str(sample_alias);
    sql.push_str(".sample_id ASC");

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|err| format!("Failed to prepare inference enqueue query: {err}"))?;
    let mut jobs: Vec<(String, String)> = Vec::new();
    let rows = stmt
        .query_map(params_from_iter(params_vec), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| format!("Failed to query inference rows: {err}"))?;
    for row in rows {
        jobs.push(row.map_err(|err| format!("Failed to decode inference row: {err}"))?);
    }
    Ok(jobs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_dirs::ConfigBaseGuard;
    use crate::sample_sources::SampleSource;
    use tempfile::tempdir;

    #[test]
    fn backfill_enqueues_when_source_has_no_features() {
        let config_dir = tempdir().unwrap();
        let _guard = ConfigBaseGuard::set(config_dir.path().to_path_buf());

        let source_root = tempdir().unwrap();
        let source = SampleSource::new(source_root.path().to_path_buf());

        // Register source root in library DB so workers can resolve paths later.
        let _ =
            crate::sample_sources::library::save(&crate::sample_sources::library::LibraryState {
                sources: vec![source.clone()],
                collections: vec![],
            })
            .unwrap();
        let source_db = crate::sample_sources::SourceDatabase::open(&source.root).unwrap();
        std::fs::create_dir_all(source.root.join("Pack")).unwrap();
        std::fs::write(source.root.join("Pack/a.wav"), b"test").unwrap();
        std::fs::write(source.root.join("Pack/b.wav"), b"test").unwrap();
        std::fs::write(source.root.join("Pack/c.wav"), b"test").unwrap();
        let mut batch = source_db.write_batch().unwrap();
        batch
            .upsert_file_with_hash(Path::new("Pack/a.wav"), 1, 1, "ha")
            .unwrap();
        batch
            .upsert_file_with_hash(Path::new("Pack/b.wav"), 1, 1, "hb")
            .unwrap();
        batch
            .upsert_file_with_hash(Path::new("Pack/c.wav"), 1, 1, "hc")
            .unwrap();
        batch.commit().unwrap();
        drop(source_db);
        let source_db = crate::sample_sources::SourceDatabase::open(&source.root).unwrap();
        let entries = source_db.list_files().unwrap();
        assert_eq!(entries.len(), 3);
        for entry in &entries {
            if entry.missing {
                source_db.set_missing(&entry.relative_path, false).unwrap();
            }
        }
        drop(source_db);

        // Populate per-source DB with a fake entry (no audio file needed for enqueue).
        let db = crate::sample_sources::SourceDatabase::open(&source.root).unwrap();
        let mut batch = db.write_batch().unwrap();
        batch
            .upsert_file_with_hash(Path::new("Pack/one.wav"), 10, 123, "h1")
            .unwrap();
        batch.commit().unwrap();

        let (inserted, progress) = enqueue_jobs_for_source_backfill(&source).unwrap();
        assert!(inserted > 0);
        assert!(progress.total() > 0);

        let (second_inserted, _) = enqueue_jobs_for_source_backfill(&source).unwrap();
        assert_eq!(second_inserted, 0);
    }

    #[test]
    fn missing_features_only_enqueues_unanalyzed_samples() {
        let config_dir = tempdir().unwrap();
        let _guard = ConfigBaseGuard::set(config_dir.path().to_path_buf());

        let source_root = tempdir().unwrap();
        let source = SampleSource::new(source_root.path().to_path_buf());

        let _ =
            crate::sample_sources::library::save(&crate::sample_sources::library::LibraryState {
                sources: vec![source.clone()],
                collections: vec![],
            })
            .unwrap();

        let db_path = crate::app_dirs::app_root_dir()
            .unwrap()
            .join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
        let conn = db::open_library_db(&db_path).unwrap();

        let a = format!("{}::Pack/a.wav", source.id.as_str());
        let b = format!("{}::Pack/b.wav", source.id.as_str());
        let c = format!("{}::Pack/c.wav", source.id.as_str());
        conn.execute(
            "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version)
             VALUES (?1, ?2, 1, 1, NULL, NULL, NULL)",
            params![&a, "ha"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version)
             VALUES (?1, ?2, 1, 1, NULL, NULL, ?3)",
            params![&b, "hb", crate::analysis::version::analysis_version()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version)
             VALUES (?1, ?2, 1, 1, NULL, NULL, NULL)",
            params![&c, "hc"],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO features (sample_id, feat_version, vec_blob, computed_at)
             VALUES (?1, 1, X'01020304', 1)",
            params![&b],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO analysis_jobs (sample_id, job_type, content_hash, status, attempts, created_at)
             VALUES (?1, ?2, ?3, 'pending', 0, 1)",
            params![&c, db::ANALYZE_SAMPLE_JOB_TYPE, "hc"],
        )
        .unwrap();

        let (_inserted, _progress) = enqueue_jobs_for_source_missing_features(&source).unwrap();

        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM analysis_jobs WHERE status='pending' AND job_type=?1",
                params![db::ANALYZE_SAMPLE_JOB_TYPE],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 1);
    }

    #[test]
    fn embedding_backfill_enqueues_missing_or_mismatched() {
        let config_dir = tempdir().unwrap();
        let _guard = ConfigBaseGuard::set(config_dir.path().to_path_buf());

        let source_root = tempdir().unwrap();
        let source = SampleSource::new(source_root.path().to_path_buf());
        let _ =
            crate::sample_sources::library::save(&crate::sample_sources::library::LibraryState {
                sources: vec![source.clone()],
                collections: vec![],
            })
            .unwrap();

        let db_path = crate::app_dirs::app_root_dir()
            .unwrap()
            .join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
        let conn = db::open_library_db(&db_path).unwrap();

        let a = format!("{}::Pack/a.wav", source.id.as_str());
        let b = format!("{}::Pack/b.wav", source.id.as_str());
        let c = format!("{}::Pack/c.wav", source.id.as_str());
        for (sample_id, hash) in [(&a, "ha"), (&b, "hb"), (&c, "hc")] {
            conn.execute(
                "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version)
                 VALUES (?1, ?2, 1, 1, NULL, NULL, NULL)",
                params![sample_id, hash],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO embeddings (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
             VALUES (?1, ?2, ?3, ?4, 1, X'01020304', 0)",
            params![
                &b,
                crate::analysis::embedding::EMBEDDING_MODEL_ID,
                crate::analysis::embedding::EMBEDDING_DIM as i64,
                crate::analysis::embedding::EMBEDDING_DTYPE_F32
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO embeddings (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
             VALUES (?1, 'old_model', ?2, ?3, 1, X'01020304', 0)",
            params![
                &c,
                crate::analysis::embedding::EMBEDDING_DIM as i64,
                crate::analysis::embedding::EMBEDDING_DTYPE_F32
            ],
        )
        .unwrap();

        let (inserted, _progress) = enqueue_jobs_for_embedding_backfill(&source).unwrap();
        assert!(inserted > 0);

        let (second_inserted, _progress) = enqueue_jobs_for_embedding_backfill(&source).unwrap();
        assert_eq!(second_inserted, 0);
    }
}
