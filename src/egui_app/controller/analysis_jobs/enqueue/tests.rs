use super::enqueue_embeddings::enqueue_jobs_for_embedding_backfill;
use super::enqueue_samples::{
    enqueue_jobs_for_source, enqueue_jobs_for_source_backfill,
    enqueue_jobs_for_source_backfill_full, enqueue_jobs_for_source_missing_features,
};
use crate::app_dirs::ConfigBaseGuard;
use crate::egui_app::controller::analysis_jobs::db;
use crate::sample_sources::scanner::ChangedSample;
use crate::sample_sources::{SampleSource, SourceDatabase};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use tempfile::{TempDir, tempdir};

struct TestEnv {
    _config_dir: TempDir,
    _config_guard: ConfigBaseGuard,
    _source_dir: TempDir,
    source: SampleSource,
}

impl TestEnv {
    fn new() -> Self {
        let config_dir = tempdir().unwrap();
        let config_guard = ConfigBaseGuard::set(config_dir.path().to_path_buf());
        let source_dir = tempdir().unwrap();
        let source = SampleSource::new(source_dir.path().to_path_buf());
        crate::sample_sources::library::save(&crate::sample_sources::library::LibraryState {
            sources: vec![source.clone()],
            collections: vec![],
        })
        .unwrap();
        Self {
            _config_dir: config_dir,
            _config_guard: config_guard,
            _source_dir: source_dir,
            source,
        }
    }

    fn create_files(&self, files: &[&str]) {
        for file in files {
            let path = self.source.root.join(file);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, b"test").unwrap();
        }
    }
}

fn seed_source_db(source: &SampleSource, entries: &[(&str, &str)]) {
    let source_db = SourceDatabase::open(&source.root).unwrap();
    let mut batch = source_db.write_batch().unwrap();
    for (path, hash) in entries {
        batch
            .upsert_file_with_hash(Path::new(path), 1, 1, hash)
            .unwrap();
    }
    batch.commit().unwrap();
}

fn clear_analysis_tables(conn: &Connection) {
    conn.execute_batch(
        "DELETE FROM analysis_jobs;
         DELETE FROM samples;
         DELETE FROM features;
         DELETE FROM embeddings;",
    )
    .unwrap();
}

fn sample_id(source: &SampleSource, relative_path: &str) -> String {
    format!("{}::{}", source.id.as_str(), relative_path)
}

fn insert_sample_row(conn: &Connection, sample_id: &str, hash: &str, version: Option<&str>) {
    if let Some(version) = version {
        conn.execute(
            "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version)
             VALUES (?1, ?2, 1, 1, NULL, NULL, ?3)",
            params![sample_id, hash, version],
        )
        .unwrap();
    } else {
        conn.execute(
            "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version)
             VALUES (?1, ?2, 1, 1, NULL, NULL, NULL)",
            params![sample_id, hash],
        )
        .unwrap();
    }
}

fn insert_features_row(conn: &Connection, sample_id: &str) {
    conn.execute(
        "INSERT INTO features (sample_id, feat_version, vec_blob, computed_at)
         VALUES (?1, 1, X'01020304', 1)",
        params![sample_id],
    )
    .unwrap();
}

fn insert_embeddings_row(conn: &Connection, sample_id: &str, model_id: &str) {
    conn.execute(
        "INSERT INTO embeddings (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
         VALUES (?1, ?2, ?3, ?4, 1, X'01020304', 0)",
        params![
            sample_id,
            model_id,
            crate::analysis::embedding::EMBEDDING_DIM as i64,
            crate::analysis::embedding::EMBEDDING_DTYPE_F32
        ],
    )
    .unwrap();
}

#[test]
fn backfill_enqueues_when_source_has_no_features() {
    let env = TestEnv::new();
    env.create_files(&["Pack/a.wav", "Pack/b.wav", "Pack/c.wav"]);
    seed_source_db(
        &env.source,
        &[("Pack/a.wav", "ha"), ("Pack/b.wav", "hb"), ("Pack/c.wav", "hc")],
    );
    let source_db = SourceDatabase::open(&env.source.root).unwrap();
    let entries = source_db.list_files().unwrap();
    assert_eq!(entries.len(), 3);
    for entry in &entries {
        if entry.missing {
            source_db.set_missing(&entry.relative_path, false).unwrap();
        }
    }

    let db = SourceDatabase::open(&env.source.root).unwrap();
    let mut batch = db.write_batch().unwrap();
    batch
        .upsert_file_with_hash(Path::new("Pack/one.wav"), 10, 123, "h1")
        .unwrap();
    batch.commit().unwrap();

    let (inserted, progress) = enqueue_jobs_for_source_backfill(&env.source).unwrap();
    assert!(inserted > 0);
    assert!(progress.total() > 0);

    let (second_inserted, _) = enqueue_jobs_for_source_backfill(&env.source).unwrap();
    assert_eq!(second_inserted, 0);
}

#[test]
fn missing_features_only_enqueues_unanalyzed_samples() {
    let env = TestEnv::new();
    let conn = db::open_source_db(&env.source.root).unwrap();
    clear_analysis_tables(&conn);

    let a = sample_id(&env.source, "Pack/a.wav");
    let b = sample_id(&env.source, "Pack/b.wav");
    let c = sample_id(&env.source, "Pack/c.wav");
    insert_sample_row(&conn, &a, "ha", None);
    insert_sample_row(
        &conn,
        &b,
        "hb",
        Some(crate::analysis::version::analysis_version()),
    );
    insert_sample_row(&conn, &c, "hc", None);
    insert_features_row(&conn, &b);
    conn.execute(
        "INSERT INTO analysis_jobs (sample_id, job_type, content_hash, status, attempts, created_at)
         VALUES (?1, ?2, ?3, 'pending', 0, 1)",
        params![&c, db::ANALYZE_SAMPLE_JOB_TYPE, "hc"],
    )
    .unwrap();

    let (_inserted, _progress) = enqueue_jobs_for_source_missing_features(&env.source).unwrap();

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
    let env = TestEnv::new();
    env.create_files(&["Pack/a.wav", "Pack/b.wav"]);
    seed_source_db(&env.source, &[("Pack/a.wav", "ha"), ("Pack/b.wav", "hb")]);

    let conn = db::open_source_db(&env.source.root).unwrap();
    clear_analysis_tables(&conn);
    let version = crate::analysis::version::analysis_version();
    for (rel, hash) in [("Pack/a.wav", "ha"), ("Pack/b.wav", "hb")] {
        let sample_id = sample_id(&env.source, rel);
        insert_sample_row(&conn, &sample_id, hash, Some(version));
        insert_features_row(&conn, &sample_id);
        insert_embeddings_row(
            &conn,
            &sample_id,
            crate::analysis::embedding::EMBEDDING_MODEL_ID,
        );
    }

    let (inserted, _progress) = enqueue_jobs_for_source_backfill_full(&env.source).unwrap();
    assert_eq!(inserted, 2);

    let (second_inserted, _progress) = enqueue_jobs_for_source_backfill_full(&env.source).unwrap();
    assert_eq!(second_inserted, 0);
}

#[test]
fn backfill_retries_failed_jobs() {
    let env = TestEnv::new();
    env.create_files(&["Pack/a.wav"]);
    seed_source_db(&env.source, &[("Pack/a.wav", "ha")]);

    let conn = db::open_source_db(&env.source.root).unwrap();
    clear_analysis_tables(&conn);
    let version = crate::analysis::version::analysis_version();
    let sample_id = sample_id(&env.source, "Pack/a.wav");
    insert_sample_row(&conn, &sample_id, "ha", Some(version));
    insert_features_row(&conn, &sample_id);
    insert_embeddings_row(
        &conn,
        &sample_id,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
    );
    conn.execute(
        "INSERT INTO analysis_jobs (sample_id, source_id, relative_path, job_type, content_hash, status, attempts, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'failed', 1, 0)",
        params![
            &sample_id,
            env.source.id.as_str(),
            "Pack/a.wav",
            db::ANALYZE_SAMPLE_JOB_TYPE,
            "ha"
        ],
    )
    .unwrap();

    let (inserted, _progress) = enqueue_jobs_for_source_backfill(&env.source).unwrap();
    assert_eq!(inserted, 1);
    let (status, last_error): (String, Option<String>) = conn
        .query_row(
            "SELECT status, last_error FROM analysis_jobs WHERE sample_id = ?1",
            params![&sample_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "pending");
    assert!(last_error.is_none());
}

#[test]
fn missing_features_skips_missing_files_and_marks_them() {
    let env = TestEnv::new();
    env.create_files(&["Pack/a.wav"]);
    seed_source_db(
        &env.source,
        &[("Pack/a.wav", "ha"), ("Pack/missing.wav", "hb")],
    );

    let (_inserted, _progress) = enqueue_jobs_for_source_missing_features(&env.source).unwrap();

    let pending: i64 = db::open_source_db(&env.source.root)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM analysis_jobs WHERE status='pending' AND job_type=?1",
            params![db::ANALYZE_SAMPLE_JOB_TYPE],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pending, 1);

    let source_db = SourceDatabase::open(&env.source.root).unwrap();
    let entries = source_db.list_files().unwrap();
    let missing_entry = entries
        .iter()
        .find(|entry| entry.relative_path == Path::new("Pack/missing.wav"))
        .unwrap();
    assert!(missing_entry.missing);
}

#[test]
fn enqueue_invalidates_when_analysis_version_stale() {
    let env = TestEnv::new();
    let conn = db::open_source_db(&env.source.root).unwrap();
    clear_analysis_tables(&conn);

    let sample_id = sample_id(&env.source, "Pack/a.wav");
    conn.execute(
        "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version)
         VALUES (?1, ?2, 1, 1, 1.0, 1, ?3)",
        params![&sample_id, "ha", "stale_version"],
    )
    .unwrap();
    insert_features_row(&conn, &sample_id);
    insert_embeddings_row(
        &conn,
        &sample_id,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
    );

    let changed_samples = vec![ChangedSample {
        relative_path: PathBuf::from("Pack/a.wav"),
        file_size: 1,
        modified_ns: 1,
        content_hash: "ha".to_string(),
    }];

    let (_inserted, _progress) = enqueue_jobs_for_source(&env.source, &changed_samples).unwrap();

    let feature_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM features WHERE sample_id = ?1",
            params![&sample_id],
            |row| row.get(0),
        )
        .unwrap();
    let embedding_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM embeddings WHERE sample_id = ?1",
            params![&sample_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(feature_count, 0);
    assert_eq!(embedding_count, 0);
}

#[test]
fn enqueue_invalidates_when_content_hash_changes() {
    let env = TestEnv::new();
    let conn = db::open_source_db(&env.source.root).unwrap();
    clear_analysis_tables(&conn);

    let sample_id = sample_id(&env.source, "Pack/a.wav");
    conn.execute(
        "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version)
         VALUES (?1, ?2, 1, 1, 1.0, 1, ?3)",
        params![&sample_id, "old_hash", crate::analysis::version::analysis_version()],
    )
    .unwrap();
    insert_features_row(&conn, &sample_id);
    insert_embeddings_row(
        &conn,
        &sample_id,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
    );

    let changed_samples = vec![ChangedSample {
        relative_path: PathBuf::from("Pack/a.wav"),
        file_size: 1,
        modified_ns: 1,
        content_hash: "new_hash".to_string(),
    }];

    let (_inserted, _progress) = enqueue_jobs_for_source(&env.source, &changed_samples).unwrap();

    let feature_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM features WHERE sample_id = ?1",
            params![&sample_id],
            |row| row.get(0),
        )
        .unwrap();
    let embedding_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM embeddings WHERE sample_id = ?1",
            params![&sample_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(feature_count, 0);
    assert_eq!(embedding_count, 0);
}

#[test]
fn backfill_invalidates_when_analysis_version_stale() {
    let env = TestEnv::new();
    env.create_files(&["Pack/a.wav"]);
    seed_source_db(&env.source, &[("Pack/a.wav", "ha")]);

    let conn = db::open_source_db(&env.source.root).unwrap();
    clear_analysis_tables(&conn);

    let sample_id = sample_id(&env.source, "Pack/a.wav");
    conn.execute(
        "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version)
         VALUES (?1, ?2, 1, 1, 1.0, 1, ?3)",
        params![&sample_id, "ha", "stale_version"],
    )
    .unwrap();
    insert_features_row(&conn, &sample_id);
    insert_embeddings_row(
        &conn,
        &sample_id,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
    );

    let (_inserted, _progress) = enqueue_jobs_for_source_backfill(&env.source).unwrap();

    let feature_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM features WHERE sample_id = ?1",
            params![&sample_id],
            |row| row.get(0),
        )
        .unwrap();
    let embedding_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM embeddings WHERE sample_id = ?1",
            params![&sample_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(feature_count, 0);
    assert_eq!(embedding_count, 0);
}

#[test]
fn backfill_invalidates_when_content_hash_changes() {
    let env = TestEnv::new();
    env.create_files(&["Pack/a.wav"]);
    seed_source_db(&env.source, &[("Pack/a.wav", "new_hash")]);

    let conn = db::open_source_db(&env.source.root).unwrap();
    clear_analysis_tables(&conn);

    let sample_id = sample_id(&env.source, "Pack/a.wav");
    conn.execute(
        "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version)
         VALUES (?1, ?2, 1, 1, 1.0, 1, ?3)",
        params![&sample_id, "old_hash", crate::analysis::version::analysis_version()],
    )
    .unwrap();
    insert_features_row(&conn, &sample_id);
    insert_embeddings_row(
        &conn,
        &sample_id,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
    );

    let (_inserted, _progress) = enqueue_jobs_for_source_backfill(&env.source).unwrap();

    let feature_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM features WHERE sample_id = ?1",
            params![&sample_id],
            |row| row.get(0),
        )
        .unwrap();
    let embedding_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM embeddings WHERE sample_id = ?1",
            params![&sample_id],
            |row| row.get(0),
        )
        .unwrap();
    let pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM analysis_jobs WHERE sample_id = ?1 AND status = 'pending'",
            params![&sample_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(feature_count, 0);
    assert_eq!(embedding_count, 0);
    assert_eq!(pending, 1);
}

#[test]
fn embedding_backfill_enqueues_missing_or_mismatched() {
    let env = TestEnv::new();
    seed_source_db(
        &env.source,
        &[("Pack/a.wav", "ha"), ("Pack/b.wav", "hb"), ("Pack/c.wav", "hc")],
    );

    let conn = db::open_source_db(&env.source.root).unwrap();
    clear_analysis_tables(&conn);

    let a = sample_id(&env.source, "Pack/a.wav");
    let b = sample_id(&env.source, "Pack/b.wav");
    let c = sample_id(&env.source, "Pack/c.wav");
    for (sample_id, hash) in [(&a, "ha"), (&b, "hb"), (&c, "hc")] {
        insert_sample_row(&conn, sample_id, hash, None);
    }
    insert_embeddings_row(
        &conn,
        &b,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
    );
    insert_embeddings_row(&conn, &c, "old_model");

    let (inserted, _progress) = enqueue_jobs_for_embedding_backfill(&env.source).unwrap();
    assert!(inserted > 0);

    let (second_inserted, _progress) = enqueue_jobs_for_embedding_backfill(&env.source).unwrap();
    assert_eq!(second_inserted, 0);
}
