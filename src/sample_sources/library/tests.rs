use super::*;
use rusqlite::OptionalExtension;
use std::collections::HashSet;
use std::fs;
use tempfile::tempdir;

fn with_config_home<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
    let _guard = crate::app_dirs::ConfigBaseGuard::set(dir.to_path_buf());
    f()
}

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
CREATE TABLE collections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    export_path TEXT,
    sort_order INTEGER NOT NULL
);
CREATE TABLE collection_members (
    collection_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    sort_order INTEGER NOT NULL,
    PRIMARY KEY (collection_id, source_id, relative_path),
    FOREIGN KEY(collection_id) REFERENCES collections(id) ON DELETE CASCADE
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

fn create_legacy_schema(conn: &Connection) {
    conn.execute_batch(LEGACY_SCHEMA_SQL).unwrap();
}

fn table_columns(conn: &Connection, table: &str) -> HashSet<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({})", table))
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .collect()
}

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

fn assert_has_columns(conn: &Connection, table: &str, columns: &[&str]) {
    let present = table_columns(conn, table);
    for column in columns {
        assert!(present.contains(*column), "expected {}.{}", table, column);
    }
}

#[test]
fn saves_and_loads_sources_and_collections() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let state = LibraryState {
            sources: vec![
                SampleSource::new(PathBuf::from("one")),
                SampleSource::new(PathBuf::from("two")),
            ],
            collections: vec![Collection {
                id: CollectionId::new(),
                name: "Test".into(),
                members: vec![CollectionMember {
                    source_id: SourceId::new(),
                    relative_path: PathBuf::from("file.wav"),
                    clip_root: None,
                }],
                export_path: None,
                hotkey: Some(2),
            }],
        };
        save(&state).unwrap();
        let loaded = load().unwrap();
        assert_eq!(loaded.sources.len(), 2);
        assert_eq!(loaded.collections.len(), 1);
        assert_eq!(loaded.collections[0].members.len(), 1);
        assert_eq!(loaded.collections[0].hotkey, Some(2));
    });
}

#[test]
fn preserves_collection_and_member_order() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let collection_one_id = CollectionId::new();
        let collection_two_id = CollectionId::new();
        let state = LibraryState {
            sources: vec![],
            collections: vec![
                Collection {
                    id: collection_one_id.clone(),
                    name: "First".into(),
                    members: vec![
                        CollectionMember {
                            source_id: SourceId::new(),
                            relative_path: PathBuf::from("alpha.wav"),
                            clip_root: None,
                        },
                        CollectionMember {
                            source_id: SourceId::new(),
                            relative_path: PathBuf::from("beta.wav"),
                            clip_root: None,
                        },
                    ],
                    export_path: None,
                    hotkey: None,
                },
                Collection {
                    id: collection_two_id.clone(),
                    name: "Second".into(),
                    members: vec![CollectionMember {
                        source_id: SourceId::new(),
                        relative_path: PathBuf::from("gamma.wav"),
                        clip_root: None,
                    }],
                    export_path: None,
                    hotkey: None,
                },
            ],
        };

        save(&state).unwrap();
        let loaded = load().unwrap();

        assert_eq!(loaded.collections.len(), 2);
        assert_eq!(loaded.collections[0].id.as_str(), collection_one_id.as_str());
        assert_eq!(loaded.collections[1].id.as_str(), collection_two_id.as_str());
        assert_eq!(loaded.collections[0].members.len(), 2);
        assert_eq!(
            loaded.collections[0].members[0].relative_path,
            PathBuf::from("alpha.wav")
        );
        assert_eq!(
            loaded.collections[0].members[1].relative_path,
            PathBuf::from("beta.wav")
        );
        assert_eq!(loaded.collections[1].members.len(), 1);
        assert_eq!(
            loaded.collections[1].members[0].relative_path,
            PathBuf::from("gamma.wav")
        );
    });
}

#[test]
fn database_lives_under_app_root() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let _ = load().unwrap();
        let db_path = temp
            .path()
            .join(app_dirs::APP_DIR_NAME)
            .join(LIBRARY_DB_FILE_NAME);
        assert!(
            db_path.exists(),
            "expected database at {}",
            db_path.display()
        );
        let metadata = fs::metadata(db_path).unwrap();
        assert!(metadata.is_file());
    });
}

#[test]
fn migrates_legacy_collection_export_paths() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        // Ensure schema exists.
        let _ = load().unwrap();
        let db_path = database_path().unwrap();
        let conn = Connection::open(&db_path).unwrap();
        conn.execute("DELETE FROM collection_members", []).unwrap();
        conn.execute("DELETE FROM collections", []).unwrap();
        conn.execute(
            "DELETE FROM metadata WHERE key = ?1",
            [COLLECTION_EXPORT_PATHS_VERSION_KEY],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO collections (id, name, export_path, sort_order) VALUES (?1, ?2, ?3, 0)",
            params!["abc", "Demo/Name", "exports"],
        )
        .unwrap();
        drop(conn);

        let state = load().unwrap();
        assert_eq!(state.collections.len(), 1);
        let expected_path = PathBuf::from("exports").join("Demo_Name");
        assert_eq!(state.collections[0].export_path, Some(expected_path));

        let conn = Connection::open(database_path().unwrap()).unwrap();
        let version: Option<String> = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                [COLLECTION_EXPORT_PATHS_VERSION_KEY],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert_eq!(version.as_deref(), Some(COLLECTION_EXPORT_PATHS_VERSION_V2));
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
            collections: vec![],
        })
        .unwrap();

        // Simulate removal by saving with no sources; mapping should still be remembered.
        save(&LibraryState {
            sources: vec![],
            collections: vec![],
        })
        .unwrap();

        let reused = lookup_source_id_for_root(&root)
            .unwrap()
            .expect("expected mapping");
        assert_eq!(reused.as_str(), id.as_str());
    });
}

#[test]
fn migrates_legacy_schema_to_latest() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let db_path = database_path().unwrap();
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let conn = Connection::open(&db_path).unwrap();
        create_legacy_schema(&conn);
        drop(conn);

        let _ = load().unwrap();
        let conn = Connection::open(database_path().unwrap()).unwrap();

        assert_has_columns(&conn, "analysis_jobs", &["content_hash"]);
        assert_has_columns(
            &conn,
            "samples",
            &["duration_seconds", "sr_used", "analysis_version"],
        );
        assert_has_columns(&conn, "collection_members", &["clip_root"]);
        assert_has_columns(&conn, "collections", &["hotkey"]);

        let embedding_columns = table_columns(&conn, "embeddings");
        for column in ["vec", "dtype", "l2_normed", "created_at"] {
            assert!(embedding_columns.contains(column));
        }
        assert!(!embedding_columns.contains("vec_blob"));

        for table in ["features", "layout_umap", "hdbscan_clusters", "ann_index_meta"] {
            assert_table_exists(&conn, table);
        }
    });
}
