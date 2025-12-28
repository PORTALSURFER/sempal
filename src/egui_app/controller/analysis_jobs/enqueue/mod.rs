mod enqueue_embeddings;
mod enqueue_helpers;
mod enqueue_samples;

pub(in crate::egui_app::controller) use enqueue_embeddings::{
    enqueue_jobs_for_embedding_backfill, enqueue_jobs_for_embedding_samples,
};
pub(in crate::egui_app::controller) use enqueue_samples::enqueue_jobs_for_source;
pub(in crate::egui_app::controller) use enqueue_samples::enqueue_jobs_for_source_backfill;
pub(in crate::egui_app::controller) use enqueue_samples::enqueue_jobs_for_source_backfill_full;
pub(in crate::egui_app::controller) use enqueue_samples::enqueue_jobs_for_source_missing_features;

#[cfg(test)]
mod tests {
    use super::enqueue_embeddings::enqueue_jobs_for_embedding_backfill;
    use super::enqueue_samples::{
        enqueue_jobs_for_source_backfill, enqueue_jobs_for_source_backfill_full,
        enqueue_jobs_for_source_missing_features,
    };
    use crate::app_dirs::ConfigBaseGuard;
    use crate::egui_app::controller::analysis_jobs::db;
    use crate::sample_sources::SampleSource;
    use rusqlite::params;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn backfill_enqueues_when_source_has_no_features() {
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

        let conn = db::open_source_db(&source.root).unwrap();
        conn.execute_batch(
            "DELETE FROM analysis_jobs;
             DELETE FROM samples;
             DELETE FROM features;
             DELETE FROM embeddings;",
        )
        .unwrap();

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
    fn backfill_full_enqueues_even_when_up_to_date() {
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
        std::fs::create_dir_all(source.root.join("Pack")).unwrap();
        std::fs::write(source.root.join("Pack/a.wav"), b"test").unwrap();
        std::fs::write(source.root.join("Pack/b.wav"), b"test").unwrap();

        let source_db = crate::sample_sources::SourceDatabase::open(&source.root).unwrap();
        let mut batch = source_db.write_batch().unwrap();
        batch
            .upsert_file_with_hash(Path::new("Pack/a.wav"), 1, 1, "ha")
            .unwrap();
        batch
            .upsert_file_with_hash(Path::new("Pack/b.wav"), 1, 1, "hb")
            .unwrap();
        batch.commit().unwrap();

        let conn = db::open_source_db(&source.root).unwrap();
        conn.execute_batch(
            "DELETE FROM analysis_jobs;
             DELETE FROM samples;
             DELETE FROM features;
             DELETE FROM embeddings;",
        )
        .unwrap();
        let version = crate::analysis::version::analysis_version();
        for (rel, hash) in [("Pack/a.wav", "ha"), ("Pack/b.wav", "hb")] {
            let sample_id = format!("{}::{}", source.id.as_str(), rel);
            conn.execute(
                "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version)
                 VALUES (?1, ?2, 1, 1, NULL, NULL, ?3)",
                params![&sample_id, hash, version],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO features (sample_id, feat_version, vec_blob, computed_at)
                 VALUES (?1, 1, X'01020304', 1)",
                params![&sample_id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO embeddings (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
                 VALUES (?1, ?2, ?3, ?4, 1, X'01020304', 0)",
                params![
                    &sample_id,
                    crate::analysis::embedding::EMBEDDING_MODEL_ID,
                    crate::analysis::embedding::EMBEDDING_DIM as i64,
                    crate::analysis::embedding::EMBEDDING_DTYPE_F32
                ],
            )
            .unwrap();
        }

        let (inserted, _progress) = enqueue_jobs_for_source_backfill_full(&source).unwrap();
        assert_eq!(inserted, 2);

        let (second_inserted, _progress) = enqueue_jobs_for_source_backfill_full(&source).unwrap();
        assert_eq!(second_inserted, 0);
    }

    #[test]
    fn missing_features_skips_missing_files_and_marks_them() {
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
        std::fs::create_dir_all(source.root.join("Pack")).unwrap();
        std::fs::write(source.root.join("Pack/a.wav"), b"test").unwrap();

        let source_db = crate::sample_sources::SourceDatabase::open(&source.root).unwrap();
        let mut batch = source_db.write_batch().unwrap();
        batch
            .upsert_file_with_hash(Path::new("Pack/a.wav"), 1, 1, "ha")
            .unwrap();
        batch
            .upsert_file_with_hash(Path::new("Pack/missing.wav"), 1, 1, "hb")
            .unwrap();
        batch.commit().unwrap();

        let (_inserted, _progress) = enqueue_jobs_for_source_missing_features(&source).unwrap();

        let pending: i64 = db::open_source_db(&source.root)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM analysis_jobs WHERE status='pending' AND job_type=?1",
                params![db::ANALYZE_SAMPLE_JOB_TYPE],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 1);

        let source_db = crate::sample_sources::SourceDatabase::open(&source.root).unwrap();
        let entries = source_db.list_files().unwrap();
        let missing_entry = entries
            .iter()
            .find(|entry| entry.relative_path == Path::new("Pack/missing.wav"))
            .unwrap();
        assert!(missing_entry.missing);
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
        let source_db = crate::sample_sources::SourceDatabase::open(&source.root).unwrap();
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

        let conn = db::open_source_db(&source.root).unwrap();
        conn.execute_batch(
            "DELETE FROM analysis_jobs;
             DELETE FROM samples;
             DELETE FROM features;
             DELETE FROM embeddings;",
        )
        .unwrap();

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
