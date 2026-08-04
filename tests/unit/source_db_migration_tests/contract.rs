use super::*;
use fixtures::{assert_source_db_schema_contract, with_legacy_db};

#[test]
fn fresh_source_database_satisfies_schema_contract() {
    let dir = tempfile::tempdir().unwrap();
    let db = SourceDatabase::open_for_user_metadata_write(dir.path()).unwrap();

    assert_source_db_schema_contract(&db.connection);
    assert_eq!(
        schema_version(&db.connection),
        schema::SOURCE_DB_SCHEMA_VERSION
    );
}

#[test]
fn current_stamped_source_database_repairs_schema_contract() {
    let dir = with_legacy_db(&format!(
        "CREATE TABLE wav_files (
            path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL,
            modified_ns INTEGER NOT NULL
        );
        INSERT INTO wav_files (path, file_size, modified_ns)
        VALUES ('folder/one.WAV', 10, 5);

        CREATE TABLE analysis_jobs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sample_id TEXT NOT NULL,
            job_type TEXT NOT NULL,
            content_hash TEXT,
            status TEXT NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            last_error TEXT,
            UNIQUE(sample_id, job_type)
        );
        INSERT INTO analysis_jobs (
            sample_id, job_type, content_hash, status, attempts, created_at, last_error
        )
        VALUES ('source-a::folder/one.WAV', 'analyze_sample', 'hash-a', 'pending', 0, 10, NULL);

        CREATE TABLE samples (
            sample_id TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            size INTEGER NOT NULL,
            mtime_ns INTEGER NOT NULL
        );
        CREATE TABLE features (
            sample_id TEXT PRIMARY KEY,
            feat_version INTEGER NOT NULL,
            vec_blob BLOB NOT NULL,
            computed_at INTEGER NOT NULL
        ) WITHOUT ROWID;
        CREATE TABLE analysis_cache_features (
            content_hash TEXT PRIMARY KEY,
            analysis_version TEXT NOT NULL,
            feat_version INTEGER NOT NULL,
            vec_blob BLOB NOT NULL,
            computed_at INTEGER NOT NULL,
            duration_seconds REAL NOT NULL,
            sr_used INTEGER NOT NULL
        ) WITHOUT ROWID;
        CREATE TABLE file_ops_journal (
            id TEXT PRIMARY KEY,
            op_type TEXT NOT NULL,
            stage TEXT NOT NULL,
            source_root TEXT,
            source_relative TEXT,
            target_relative TEXT NOT NULL,
            staged_relative TEXT,
            file_size INTEGER,
            modified_ns INTEGER,
            tag INTEGER,
            looped INTEGER,
            last_played_at INTEGER,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE pending_wav_renames (
            path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL,
            modified_ns INTEGER NOT NULL,
            content_hash TEXT,
            tag INTEGER NOT NULL,
            looped INTEGER NOT NULL,
            locked INTEGER NOT NULL,
            last_played_at INTEGER
        );
        PRAGMA user_version = {};",
        schema::SOURCE_DB_SCHEMA_VERSION
    ));

    let db = SourceDatabase::open_for_user_metadata_write(dir.path()).unwrap();

    assert_source_db_schema_contract(&db.connection);
    assert_eq!(
        schema_version(&db.connection),
        schema::SOURCE_DB_SCHEMA_VERSION
    );
    assert_eq!(db.list_files().unwrap().len(), 1);
    assert_eq!(db.list_search_entry_rows().unwrap().len(), 1);
    assert_eq!(analysis_job_count(&db.connection), 1);
}

#[test]
fn v11_source_database_adds_directory_truth_without_losing_existing_rows() {
    let dir = with_legacy_db(
        "CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         INSERT INTO metadata (key, value) VALUES ('revision', '7');
         CREATE TABLE wav_files (
             path TEXT PRIMARY KEY,
             file_size INTEGER NOT NULL,
             modified_ns INTEGER NOT NULL
         );
         INSERT INTO wav_files (path, file_size, modified_ns)
         VALUES ('legacy/one.wav', 42, 99);
         PRAGMA user_version = 11;",
    );

    let db = SourceDatabase::open_for_test_fixture_source_write(dir.path()).unwrap();

    assert_eq!(
        schema_version(&db.connection),
        schema::SOURCE_DB_SCHEMA_VERSION
    );
    assert_source_db_schema_contract(&db.connection);
    assert_eq!(db.get_revision().unwrap(), 7);
    assert_eq!(
        db.entry_for_path(std::path::Path::new("legacy/one.wav"))
            .unwrap()
            .map(|entry| (entry.file_size, entry.modified_ns)),
        Some((42, 99))
    );
}

fn schema_version(connection: &rusqlite::Connection) -> i64 {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

fn analysis_job_count(connection: &rusqlite::Connection) -> i64 {
    connection
        .query_row("SELECT COUNT(*) FROM analysis_jobs", [], |row| row.get(0))
        .unwrap()
}
