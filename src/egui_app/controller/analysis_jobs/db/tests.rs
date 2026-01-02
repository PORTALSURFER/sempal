use super::*;
use rusqlite::{Connection, params};

fn conn_with_schema() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE analysis_jobs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sample_id TEXT NOT NULL,
            job_type TEXT NOT NULL,
            content_hash TEXT,
            status TEXT NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            running_at INTEGER,
            last_error TEXT,
            UNIQUE(sample_id, job_type)
        );
        CREATE TABLE samples (
            sample_id TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            size INTEGER NOT NULL,
            mtime_ns INTEGER NOT NULL,
            duration_seconds REAL,
            sr_used INTEGER,
            analysis_version TEXT
        );
        CREATE TABLE wav_files (
            path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL,
            modified_ns INTEGER NOT NULL,
            tag INTEGER NOT NULL DEFAULT 0,
            missing INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE analysis_features (
            sample_id TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            features BLOB
        );
        CREATE TABLE features (
            sample_id TEXT PRIMARY KEY,
            feat_version INTEGER NOT NULL,
            vec_blob BLOB NOT NULL,
            computed_at INTEGER NOT NULL
        ) WITHOUT ROWID;
        CREATE TABLE embeddings (
            sample_id TEXT PRIMARY KEY,
            model_id TEXT NOT NULL,
            dim INTEGER NOT NULL,
            dtype TEXT NOT NULL,
            l2_normed INTEGER NOT NULL,
            vec BLOB NOT NULL,
            created_at INTEGER NOT NULL
        ) WITHOUT ROWID;
        ",
    )
    .unwrap();
    conn
}

#[test]
fn enqueue_jobs_dedupes_by_sample_and_type() {
    let mut conn = conn_with_schema();
    conn.execute(
        "INSERT INTO wav_files (path, file_size, modified_ns, tag, missing)
         VALUES (?1, ?2, ?3, 0, 0)",
        params!["a.wav", 1, 1],
    )
    .unwrap();
    let jobs = vec![
        ("s::a.wav".to_string(), "h1".to_string()),
        ("s::a.wav".to_string(), "h1".to_string()),
    ];
    let inserted = enqueue_jobs(&mut conn, &jobs, DEFAULT_JOB_TYPE, 123).unwrap();
    assert_eq!(inserted, 2);
    let progress = current_progress(&conn).unwrap();
    assert_eq!(progress.pending, 1);
    assert_eq!(progress.total(), 1);
}

#[test]
fn claim_next_job_marks_running_and_increments_attempts() {
    let mut conn = conn_with_schema();
    let jobs = vec![("s::a.wav".to_string(), "h1".to_string())];
    enqueue_jobs(&mut conn, &jobs, DEFAULT_JOB_TYPE, 123).unwrap();
        let job = claim_next_job(&mut conn, std::path::Path::new("/tmp"))
        .unwrap()
        .expect("job claimed");
    assert_eq!(job.sample_id, "s::a.wav");
    assert_eq!(job.content_hash.as_deref(), Some("h1"));
    assert_eq!(job.job_type, DEFAULT_JOB_TYPE);
    let (status, attempts): (String, i64) = conn
        .query_row(
            "SELECT status, attempts FROM analysis_jobs WHERE id = ?1",
            params![job.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "running");
    assert_eq!(attempts, 1);
}

#[test]
fn mark_done_clears_error_and_updates_status() {
    let conn = conn_with_schema();
    conn.execute(
        "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at, last_error)
         VALUES ('s::a.wav', 'x', 'running', 1, 0, 'oops')",
        [],
    )
    .unwrap();
    let job_id: i64 = conn
        .query_row("SELECT id FROM analysis_jobs", [], |row| row.get(0))
        .unwrap();
    mark_done(&conn, job_id).unwrap();
    let (status, last_error): (String, Option<String>) = conn
        .query_row(
            "SELECT status, last_error FROM analysis_jobs WHERE id = ?1",
            params![job_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "done");
    assert_eq!(last_error, None);
}

#[test]
fn mark_failed_updates_status_and_error() {
    let conn = conn_with_schema();
    conn.execute(
        "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at)
         VALUES ('s::a.wav', 'x', 'running', 1, 0)",
        [],
    )
    .unwrap();
    let job_id: i64 = conn
        .query_row("SELECT id FROM analysis_jobs", [], |row| row.get(0))
        .unwrap();
    mark_failed(&conn, job_id, "boom").unwrap();
    let (status, last_error): (String, Option<String>) = conn
        .query_row(
            "SELECT status, last_error FROM analysis_jobs WHERE id = ?1",
            params![job_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "failed");
    assert_eq!(last_error.as_deref(), Some("boom"));
}

#[test]
fn reset_running_to_pending_updates_rows() {
    let conn = conn_with_schema();
    conn.execute(
        "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at, running_at)
         VALUES ('s::a.wav', 'x', 'running', 1, 0, 5)",
        [],
    )
    .unwrap();
    let changed = reset_running_to_pending(&conn).unwrap();
    assert_eq!(changed, 1);
    let (status, running_at): (String, Option<i64>) = conn
        .query_row(
            "SELECT status, running_at FROM analysis_jobs WHERE sample_id = 's::a.wav'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "pending");
    assert_eq!(running_at, None);
}

#[test]
fn fail_stale_running_jobs_ignores_recent_claims() {
    let conn = conn_with_schema();
    conn.execute(
        "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at, running_at)
         VALUES ('s::old.wav', 'x', 'running', 1, 0, 10)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at, running_at)
         VALUES ('s::fresh.wav', 'x', 'running', 1, 0, 100)",
        [],
    )
    .unwrap();
    let changed = fail_stale_running_jobs(&conn, 50).unwrap();
    assert_eq!(changed, 1);
    let status_old: String = conn
        .query_row(
            "SELECT status FROM analysis_jobs WHERE sample_id = 's::old.wav'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let status_fresh: String = conn
        .query_row(
            "SELECT status FROM analysis_jobs WHERE sample_id = 's::fresh.wav'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status_old, "failed");
    assert_eq!(status_fresh, "running");
}

#[test]
fn fail_stale_running_jobs_marks_failed() {
    let conn = conn_with_schema();
    conn.execute(
        "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at, running_at)
         VALUES ('s::a.wav', 'x', 'running', 1, 0, 10)",
        [],
    )
    .unwrap();
    let changed = fail_stale_running_jobs(&conn, 20).unwrap();
    assert_eq!(changed, 1);
    let (status, last_error): (String, Option<String>) = conn
        .query_row(
            "SELECT status, last_error FROM analysis_jobs WHERE sample_id = 's::a.wav'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "failed");
    assert!(last_error.unwrap_or_default().contains("Timed out"));
}

#[test]
fn prune_jobs_for_missing_sources_removes_orphans() {
    let conn = conn_with_schema();
    conn.execute(
        "INSERT INTO wav_files (path, file_size, modified_ns, tag, missing)
         VALUES ('a.wav', 1, 1, 0, 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at)
         VALUES ('s::a.wav', ?1, 'pending', 0, 0)",
        params![ANALYZE_SAMPLE_JOB_TYPE],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at)
         VALUES ('missing::b.wav', ?1, 'pending', 0, 0)",
        params![ANALYZE_SAMPLE_JOB_TYPE],
    )
    .unwrap();
    let removed = prune_jobs_for_missing_sources(&conn).unwrap();
    assert_eq!(removed, 1);
    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM analysis_jobs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(remaining, 1);
}

#[test]
fn purge_orphaned_samples_removes_rows_from_all_tables() {
    let mut conn = conn_with_schema();
    conn.execute(
        "INSERT INTO wav_files (path, file_size, modified_ns, tag, missing)
         VALUES ('a.wav', 1, 1, 0, 0)",
        [],
    )
    .unwrap();
    for sample_id in ["s::a.wav", "missing::b.wav"] {
        conn.execute(
            "INSERT INTO samples (sample_id, content_hash, size, mtime_ns)
             VALUES (?1, 'h', 1, 1)",
            params![sample_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at)
             VALUES (?1, ?2, 'pending', 0, 0)",
            params![sample_id, ANALYZE_SAMPLE_JOB_TYPE],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO analysis_features (sample_id, content_hash, features)
             VALUES (?1, 'h', NULL)",
            params![sample_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO features (sample_id, feat_version, vec_blob, computed_at)
             VALUES (?1, 1, x'00', 0)",
            params![sample_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO embeddings (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
             VALUES (?1, 'm', 1, 'f32', 1, x'00', 0)",
            params![sample_id],
        )
        .unwrap();
    }
    let removed = purge_orphaned_samples(&mut conn).unwrap();
    assert_eq!(removed, 5);
    for table in [
        "samples",
        "analysis_jobs",
        "analysis_features",
        "features",
        "embeddings",
    ] {
        let count: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE sample_id = 'missing::b.wav'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}

#[test]
fn update_analysis_metadata_updates_matching_hash() {
    let conn = conn_with_schema();
    conn.execute(
        "INSERT INTO samples (sample_id, content_hash, size, mtime_ns)
         VALUES ('s::a.wav', 'h1', 10, 5)",
        [],
    )
    .unwrap();
    update_analysis_metadata(
        &conn,
        "s::a.wav",
        Some("h1"),
        1.25,
        crate::analysis::audio::ANALYSIS_SAMPLE_RATE,
        "analysis_v1_test",
    )
    .unwrap();
    let (duration, sr, version): (Option<f64>, Option<i64>, Option<String>) = conn
        .query_row(
            "SELECT duration_seconds, sr_used, analysis_version FROM samples WHERE sample_id = 's::a.wav'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(duration, Some(1.25));
    assert_eq!(
        sr,
        Some(crate::analysis::audio::ANALYSIS_SAMPLE_RATE as i64)
    );
    assert_eq!(version.as_deref(), Some("analysis_v1_test"));
}

#[test]
fn upsert_analysis_features_overwrites_existing() {
    let conn = conn_with_schema();
    upsert_analysis_features(&conn, "s::a.wav", b"one", 1, 100).unwrap();
    upsert_analysis_features(&conn, "s::a.wav", b"two", 1, 200).unwrap();
    let (version, blob, computed_at): (i64, Vec<u8>, i64) = conn
        .query_row(
            "SELECT feat_version, vec_blob, computed_at FROM features WHERE sample_id = 's::a.wav'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(version, 1);
    assert_eq!(blob, b"two");
    assert_eq!(computed_at, 200);
}
