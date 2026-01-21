use super::*;
use rusqlite::OptionalExtension;
use std::collections::HashSet;
use tempfile::tempdir;

fn with_config_home<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
    let _guard = crate::app_dirs::ConfigBaseGuard::set(dir.to_path_buf());
    f()
}

#[allow(dead_code)]
const LEGACY_SCHEMA_SQL: &str = r#"
CREATE TABLE metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE sources (
    id TEXT PRIMARY KEY,
    root TEXT NOT NULL,
    sort_order INTEGER NOT NULL
);
CREATE TABLE analysis_jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sample_id TEXT NOT NULL,
    job_type TEXT NOT NULL,
    status TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    last_error TEXT,
    UNIQUE(sample_id, job_type)
);
CREATE INDEX idx_analysis_jobs_status_created_id
    ON analysis_jobs (status, created_at, id);
CREATE INDEX idx_analysis_jobs_status_sample_id
    ON analysis_jobs (status, sample_id);
CREATE TABLE samples (
    sample_id TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    size INTEGER NOT NULL,
    mtime_ns INTEGER NOT NULL
);
CREATE TABLE analysis_features (
    sample_id TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    features BLOB
);
CREATE TABLE embeddings (
    sample_id TEXT PRIMARY KEY,
    model_id TEXT NOT NULL,
    dim INTEGER NOT NULL,
    vec_blob BLOB NOT NULL
) WITHOUT ROWID;
"#;

#[allow(dead_code)]
fn create_legacy_schema(conn: &Connection) {
    conn.execute_batch(LEGACY_SCHEMA_SQL).unwrap();
}

#[allow(dead_code)]
fn table_columns(conn: &Connection, table: &str) -> HashSet<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({})", table))
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .collect()
}

#[allow(dead_code)]
fn assert_table_exists(conn: &Connection, table: &str) {
    let exists: Option<String> = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(exists.as_deref(), Some(table));
}

#[allow(dead_code)]
fn assert_has_columns(conn: &Connection, table: &str, columns: &[&str]) {
    let present = table_columns(conn, table);
    for column in columns {
        assert!(present.contains(*column), "expected {}.{}", table, column);
    }
}

#[test]
fn recovers_from_library_lock_poisoning() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let result = std::panic::catch_unwind(|| {
            let _guard = LIBRARY_LOCK.lock().unwrap();
            panic!("poison library lock");
        });
        assert!(result.is_err());

        let state = LibraryState {
            sources: Vec::new(),
        };
        save(&state).unwrap();
        let loaded = load().unwrap();
        assert!(loaded.sources.is_empty());
    });
}





#[test]
fn creates_embedding_tables() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let _ = load().unwrap();
        let conn = Connection::open(database_path().unwrap()).unwrap();
        for table in [
            "embeddings",
            "ann_index_meta",
            "layout_umap",
            "hdbscan_clusters",
        ] {
            let exists: Option<String> = conn
                .query_row(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .optional()
                .unwrap();
            assert_eq!(exists.as_deref(), Some(table));
        }
    });
}

#[test]
fn applies_workload_pragmas_and_indices() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let _ = load().unwrap();
        let conn = Connection::open(database_path().unwrap()).unwrap();

        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");

        let synchronous: i64 = conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .unwrap();
        assert_eq!(synchronous, 2, "expected PRAGMA synchronous=NORMAL (2)");

        let busy_timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(busy_timeout, 5000);

        let idx: Option<String> = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='index' AND name='idx_analysis_jobs_status_created_id'",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert_eq!(idx.as_deref(), Some("idx_analysis_jobs_status_created_id"));
    });
}

#[test]
fn reuses_known_source_id_for_same_root() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let root = normalize_path(Path::new("some/root"));
        let id = SourceId::new();
        save(&LibraryState {
            sources: vec![SampleSource::new_with_id(id.clone(), root.clone())],
        })
        .unwrap();

        // Simulate removal by saving with no sources; mapping should still be remembered.
        save(&LibraryState {
            sources: vec![],
        })
        .unwrap();

        let reused = lookup_source_id_for_root(&root)
            .unwrap()
            .expect("expected mapping");
        assert_eq!(reused.as_str(), id.as_str());
    });
}
