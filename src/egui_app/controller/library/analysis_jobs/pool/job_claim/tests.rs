use super::*;
use super::super::job_progress::ProgressPollerWakeup;
use crate::egui_app::controller::library::analysis_jobs::db as analysis_db;
use crate::sample_sources::SampleSource;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::NamedTempFile;
use tempfile::TempDir;

#[test]
fn claim_selection_orders_sources_round_robin() {
    let dir_a = TempDir::new().unwrap();
    let dir_b = TempDir::new().unwrap();
    let source_a = SampleSource::new(dir_a.path().to_path_buf());
    let source_b = SampleSource::new(dir_b.path().to_path_buf());
    let conn_a = analysis_db::open_source_db(&source_a.root).unwrap();
    let conn_b = analysis_db::open_source_db(&source_b.root).unwrap();
    let sample_a = format!("{}::a.wav", source_a.id);
    let sample_b = format!("{}::b.wav", source_b.id);
    conn_a
        .execute(
            "INSERT INTO analysis_jobs (sample_id, source_id, relative_path, job_type, status, attempts, created_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', 0, 0)",
            rusqlite::params![
                sample_a,
                source_a.id.to_string(),
                "a.wav",
                analysis_db::ANALYZE_SAMPLE_JOB_TYPE
            ],
        )
        .unwrap();
    conn_b
        .execute(
            "INSERT INTO analysis_jobs (sample_id, source_id, relative_path, job_type, status, attempts, created_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', 0, 0)",
            rusqlite::params![
                sample_b,
                source_b.id.to_string(),
                "b.wav",
                analysis_db::ANALYZE_SAMPLE_JOB_TYPE
            ],
        )
        .unwrap();
    let reset_done = Arc::new(Mutex::new(HashSet::new()));
    let mut selector = selection::ClaimSelector::with_sources_for_tests(
        vec![
            super::claim::SourceClaimDb {
                source: source_a,
                conn: conn_a,
            },
            super::claim::SourceClaimDb {
                source: source_b,
                conn: conn_b,
            },
        ],
        1,
        reset_done,
    );
    let first = match selector.select_next(None) {
        selection::ClaimSelection::Job(job) => job,
        _ => panic!("expected a job from first source"),
    };
    let second = match selector.select_next(None) {
        selection::ClaimSelection::Job(job) => job,
        _ => panic!("expected a job from second source"),
    };

    assert!(first.sample_id.ends_with("a.wav"));
    assert!(second.sample_id.ends_with("b.wav"));
}

#[test]
fn clears_inflight_when_db_open_fails() {
    let file = NamedTempFile::new().unwrap();
    let source_root = file.path().to_path_buf();
    let job = analysis_db::ClaimedJob {
        id: 42,
        sample_id: "source::missing.wav".to_string(),
        content_hash: None,
        job_type: analysis_db::ANALYZE_SAMPLE_JOB_TYPE.to_string(),
        source_root: source_root.clone(),
    };
    let queue = DecodedQueue::new(4);
    assert!(queue.try_mark_inflight(job.id));
    let (tx, _rx) = mpsc::channel::<JobMessage>();
    let mut connections = HashMap::new();
    let progress_cache = Arc::new(RwLock::new(ProgressCache::default()));
    let progress_wakeup = ProgressPollerWakeup::new();

    let deferred = db::finalize_immediate_job(
        &mut connections,
        &queue,
        &tx,
        job,
        Err("failed".to_string()),
        false,
        &progress_cache,
        &progress_wakeup,
    );

    assert!(deferred.is_some());
    assert!(queue.try_mark_inflight(42));
    assert!(connections.is_empty());
}

#[test]
fn decode_heartbeat_keeps_running_job_fresh() {
    let dir = TempDir::new().unwrap();
    let conn = analysis_db::open_source_db(dir.path()).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64;
    conn.execute(
        "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at, running_at)
         VALUES (?1, ?2, 'running', 1, ?3, ?4)",
        rusqlite::params![
            "source::long.wav",
            analysis_db::ANALYZE_SAMPLE_JOB_TYPE,
            now,
            now - 120
        ],
    )
    .unwrap();
    let job_id: i64 = conn
        .query_row(
            "SELECT id FROM analysis_jobs WHERE sample_id = ?1",
            rusqlite::params!["source::long.wav"],
            |row| row.get(0),
        )
        .unwrap();

    let (stop, handle) =
        db::spawn_decode_heartbeat(dir.path().to_path_buf(), job_id, Duration::from_millis(10));
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        let running_at: Option<i64> = conn
            .query_row(
                "SELECT running_at FROM analysis_jobs WHERE id = ?1",
                rusqlite::params![job_id],
                |row| row.get(0),
            )
            .unwrap_or(None);
        if running_at.is_some_and(|ts| ts >= now - 1) {
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        sleep(Duration::from_millis(10));
    }
    let stale_before = now - 1;
    let changed = analysis_db::fail_stale_running_jobs(&conn, stale_before).unwrap();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = handle.join();

    assert_eq!(changed, 0);
}

#[test]
fn mid_loop_db_open_failure_clears_inflight_and_marks_failed() {
    let dir = TempDir::new().unwrap();
    let source_root = dir.path().join("source");
    std::fs::create_dir_all(&source_root).unwrap();
    let conn = analysis_db::open_source_db(&source_root).unwrap();
    conn.execute(
        "INSERT INTO analysis_jobs (sample_id, source_id, relative_path, job_type, status, attempts, created_at, running_at)
         VALUES (?1, ?2, ?3, ?4, 'running', 1, 0, 0)",
        rusqlite::params![
            "source::a.wav",
            "source",
            "a.wav",
            analysis_db::ANALYZE_SAMPLE_JOB_TYPE
        ],
    )
    .unwrap();
    let job_id: i64 = conn
        .query_row(
            "SELECT id FROM analysis_jobs WHERE sample_id = ?1",
            rusqlite::params!["source::a.wav"],
            |row| row.get(0),
        )
        .unwrap();
    drop(conn);
    let job = analysis_db::ClaimedJob {
        id: job_id,
        sample_id: "source::a.wav".to_string(),
        content_hash: None,
        job_type: analysis_db::ANALYZE_SAMPLE_JOB_TYPE.to_string(),
        source_root: source_root.clone(),
    };
    let queue = DecodedQueue::new(4);
    assert!(queue.try_mark_inflight(job.id));
    let (tx, _rx) = mpsc::channel::<JobMessage>();
    let progress_cache = Arc::new(RwLock::new(ProgressCache::default()));
    let progress_wakeup = ProgressPollerWakeup::new();
    let mut connections = HashMap::new();

    let backup_root = dir.path().join("source_backup");
    std::fs::rename(&source_root, &backup_root).unwrap();
    let deferred = db::finalize_immediate_job(
        &mut connections,
        &queue,
        &tx,
        job.clone(),
        Err("Failed to open source DB".to_string()),
        false,
        &progress_cache,
        &progress_wakeup,
    );
    assert!(queue.try_mark_inflight(job.id));
    assert!(deferred.is_some());

    std::fs::rename(&backup_root, &source_root).unwrap();
    let mut deferred_updates = vec![deferred.unwrap()];
    db::flush_deferred_updates(
        &mut connections,
        &queue,
        &tx,
        &progress_cache,
        &progress_wakeup,
        &mut deferred_updates,
        false,
    );
    assert!(deferred_updates.is_empty());
    let conn = analysis_db::open_source_db(&source_root).unwrap();
    let (status, last_error): (String, Option<String>) = conn
        .query_row(
            "SELECT status, last_error FROM analysis_jobs WHERE id = ?1",
            rusqlite::params![job_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "failed");
    assert_eq!(last_error.as_deref(), Some("Failed to open source DB"));
}
