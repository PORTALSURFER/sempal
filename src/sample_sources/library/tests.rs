use super::*;
use rusqlite::OptionalExtension;
use std::fs;
use tempfile::tempdir;

fn with_config_home<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
    let _guard = crate::app_dirs::ConfigBaseGuard::set(dir.to_path_buf());
    f()
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
            }],
        };
        save(&state).unwrap();
        let loaded = load().unwrap();
        assert_eq!(loaded.sources.len(), 2);
        assert_eq!(loaded.collections.len(), 1);
        assert_eq!(loaded.collections[0].members.len(), 1);
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
        assert!(db_path.exists(), "expected database at {}", db_path.display());
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
fn creates_labels_user_table() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let _ = load().unwrap();
        let conn = Connection::open(database_path().unwrap()).unwrap();
        let exists: Option<String> = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='labels_user'",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert_eq!(exists.as_deref(), Some("labels_user"));
    });
}

#[test]
fn applies_workload_pragmas_and_indices() {
    let temp = tempdir().unwrap();
    with_config_home(temp.path(), || {
        let _ = load().unwrap();
        let conn = Connection::open(database_path().unwrap()).unwrap();

        let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap();
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");

        let synchronous: i64 = conn.query_row("PRAGMA synchronous", [], |row| row.get(0)).unwrap();
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

        let reused = lookup_source_id_for_root(&root).unwrap().expect("expected mapping");
        assert_eq!(reused.as_str(), id.as_str());
    });
}
