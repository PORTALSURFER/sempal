use super::*;
use rusqlite::{Connection, params};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::Duration;

fn conn_with_schema() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE samples (
            sample_id TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            size INTEGER NOT NULL,
            mtime_ns INTEGER NOT NULL,
            duration_seconds REAL,
            sr_used INTEGER,
            analysis_version TEXT,
            bpm REAL
        );
        CREATE TABLE embeddings (
            sample_id TEXT PRIMARY KEY,
            model_id TEXT NOT NULL,
            dim INTEGER NOT NULL,
            dtype TEXT NOT NULL,
            l2_normed INTEGER NOT NULL,
            vec BLOB NOT NULL,
            created_at INTEGER NOT NULL
        ) WITHOUT ROWID;
        CREATE TABLE features (
            sample_id TEXT PRIMARY KEY,
            feat_version INTEGER NOT NULL,
            vec_blob BLOB NOT NULL,
            computed_at INTEGER NOT NULL
        ) WITHOUT ROWID;
        CREATE TABLE analysis_cache_features (
            content_hash TEXT NOT NULL,
            analysis_version TEXT NOT NULL,
            feat_version INTEGER NOT NULL,
            vec_blob BLOB NOT NULL,
            computed_at INTEGER NOT NULL,
            duration_seconds REAL NOT NULL,
            sr_used INTEGER NOT NULL,
            PRIMARY KEY (content_hash, analysis_version, feat_version)
        );
        CREATE TABLE analysis_cache_embeddings (
            content_hash TEXT NOT NULL,
            analysis_version TEXT NOT NULL,
            model_id TEXT NOT NULL,
            dim INTEGER NOT NULL,
            dtype TEXT NOT NULL,
            l2_normed INTEGER NOT NULL,
            vec BLOB NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (content_hash, analysis_version, model_id)
        );",
    )
    .unwrap();
    conn
}

fn insert_sample(conn: &Connection, sample_id: &str, content_hash: &str) {
    conn.execute(
        "INSERT INTO samples (sample_id, content_hash, size, mtime_ns)
         VALUES (?1, ?2, 1, 1)",
        params![sample_id, content_hash],
    )
    .unwrap();
}

fn make_job(sample_ids: &[&str], root: &Path) -> db::ClaimedJob {
    let payload = serde_json::to_string(sample_ids).unwrap();
    db::ClaimedJob {
        id: 1,
        sample_id: sample_ids.first().unwrap_or(&"").to_string(),
        content_hash: Some(payload),
        job_type: "embedding_backfill".to_string(),
        source_root: root.to_path_buf(),
    }
}

fn make_work(id: &str) -> EmbeddingWork {
    EmbeddingWork {
        content_hash: format!("hash-{id}"),
        absolute_path: PathBuf::from(format!("dummy/{id}.wav")),
        sample_ids: vec![id.to_string()],
    }
}

#[test]
fn drain_batch_caps_at_limit() {
    let mut queue = VecDeque::new();
    queue.push_back(make_work("a"));
    queue.push_back(make_work("b"));
    queue.push_back(make_work("c"));

    let batch = drain_batch(&mut queue, 2);
    assert_eq!(batch.len(), 2);
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.front().unwrap().sample_ids[0], "c");
}

#[test]
fn collect_results_limits_error_list() {
    let (tx, rx) = channel();
    tx.send(Err("err-1".to_string())).unwrap();
    tx.send(Ok(EmbeddingComputation {
        content_hash: "hash-a".to_string(),
        sample_ids: vec!["a".to_string()],
        embedding: vec![0.0_f32; 2],
        created_at: 1,
    }))
    .unwrap();
    tx.send(Err("err-2".to_string())).unwrap();
    tx.send(Err("err-3".to_string())).unwrap();
    tx.send(Err("err-4".to_string())).unwrap();
    drop(tx);

    let (results, errors) = collect_results(rx);
    assert_eq!(results.len(), 1);
    assert_eq!(errors.len(), 3);
    assert_eq!(errors[0], "err-1");
    assert_eq!(errors[2], "err-3");
}

#[test]
fn backfill_retry_succeeds_after_failures() {
    let mut attempts = 0;
    let result = retry_backfill_write_with(
        || {
            attempts += 1;
            if attempts < 3 {
                Err("nope".to_string())
            } else {
                Ok(())
            }
        },
        4,
        Duration::from_millis(0),
    );
    assert!(result.is_ok());
    assert_eq!(attempts, 3);
}

#[test]
fn backfill_retry_stops_after_limit() {
    let mut attempts = 0;
    let result = retry_backfill_write_with(
        || {
            attempts += 1;
            Err("nope".to_string())
        },
        3,
        Duration::from_millis(0),
    );
    assert!(result.is_err());
    assert_eq!(attempts, 3);
}

#[test]
fn plan_uses_cached_embedding_when_available() {
    let conn = conn_with_schema();
    insert_sample(&conn, "s::a.wav", "hash-a");
    let vec = vec![0.0_f32; crate::analysis::similarity::SIMILARITY_DIM];
    let blob = crate::analysis::vector::encode_f32_le_blob(&vec);
    conn.execute(
        "INSERT INTO analysis_cache_embeddings
            (content_hash, analysis_version, model_id, dim, dtype, l2_normed, vec, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, 42)",
        params![
            "hash-a",
            "v1",
            crate::analysis::similarity::SIMILARITY_MODEL_ID,
            crate::analysis::similarity::SIMILARITY_DIM as i64,
            crate::analysis::similarity::SIMILARITY_DTYPE_F32,
            blob
        ],
    )
    .unwrap();

    let temp = tempfile::TempDir::new().unwrap();
    let job = make_job(&["s::a.wav"], temp.path());
    let plan =
        build_backfill_plan(&conn, &job, &["s::a.wav".to_string()], true, "v1").expect("plan");

    assert!(plan.work.is_empty());
    assert_eq!(plan.ready.len(), 1);
    assert_eq!(plan.ready[0].sample_id, "s::a.wav");
    assert_eq!(plan.ready[0].created_at, 42);
}

#[test]
fn plan_builds_work_when_cache_misses() {
    let conn = conn_with_schema();
    insert_sample(&conn, "s::a.wav", "hash-a");
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.wav"), b"data").unwrap();
    let job = make_job(&["s::a.wav"], temp.path());
    let plan =
        build_backfill_plan(&conn, &job, &["s::a.wav".to_string()], false, "v1").expect("plan");

    assert!(plan.ready.is_empty());
    assert_eq!(plan.work.len(), 1);
    assert_eq!(plan.work[0].sample_ids, vec!["s::a.wav".to_string()]);
}

#[test]
fn plan_reuses_content_hash_for_work() {
    let conn = conn_with_schema();
    insert_sample(&conn, "s::a.wav", "hash-a");
    insert_sample(&conn, "s::b.wav", "hash-a");
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.wav"), b"data").unwrap();
    std::fs::write(temp.path().join("b.wav"), b"data").unwrap();
    let job = make_job(&["s::a.wav", "s::b.wav"], temp.path());
    let plan = build_backfill_plan(
        &conn,
        &job,
        &["s::a.wav".to_string(), "s::b.wav".to_string()],
        false,
        "v1",
    )
    .expect("plan");

    assert_eq!(plan.work.len(), 1);
    assert!(plan.ready.is_empty());
    assert_eq!(plan.work[0].content_hash, "hash-a");
    assert_eq!(plan.work[0].sample_ids.len(), 2);
}
