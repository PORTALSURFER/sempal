use super::*;
use fixtures::{column_names, with_legacy_db};

#[test]
fn v10_migration_adds_index_only_schema_without_changing_sample_metadata() {
    let dir = with_legacy_db(
        "CREATE TABLE metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        INSERT INTO metadata (key, value) VALUES ('revision', '41');
        CREATE TABLE wav_files (
            path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL,
            modified_ns INTEGER NOT NULL,
            tag INTEGER NOT NULL DEFAULT 0,
            looped INTEGER NOT NULL DEFAULT 0,
            locked INTEGER NOT NULL DEFAULT 0,
            missing INTEGER NOT NULL DEFAULT 0,
            extension TEXT NOT NULL DEFAULT '',
            last_played_at INTEGER,
            last_curated_at INTEGER
        );
        INSERT INTO wav_files (
            path, file_size, modified_ns, tag, looped, locked, missing,
            extension, last_played_at, last_curated_at
        ) VALUES ('kept.wav', 10, 5, 3, 1, 1, 0, 'wav', 20, 30);
        PRAGMA user_version = 10;",
    );

    let db = SourceDatabase::open_for_test_fixture_source_write(dir.path()).unwrap();

    assert_eq!(
        column_names(&db.connection, "source_index_entries"),
        vec![
            "path",
            "path_encoding",
            "classification",
            "file_size",
            "modified_ns",
            "file_identity",
            "diagnostic",
            "format_policy_version",
        ]
    );
    let preserved = db
        .connection
        .query_row(
            "SELECT tag, looped, locked, last_played_at, last_curated_at
             FROM wav_files WHERE path = 'kept.wav'",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(preserved, (3, 1, 1, 20, 30));
    assert_eq!(db.get_revision().unwrap(), 41);
}

#[test]
fn legacy_read_only_database_projects_an_empty_index_without_migrating() {
    let dir = with_legacy_db(
        "CREATE TABLE metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE wav_files (
            path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL,
            modified_ns INTEGER NOT NULL
        );
        PRAGMA user_version = 10;",
    );

    let db = SourceDatabase::open_for_ui_read(dir.path()).unwrap();
    let snapshot = db.source_index_snapshot().unwrap();

    assert_eq!(snapshot.revision, 0);
    assert!(snapshot.entries.is_empty());
    assert!(column_names(&db.connection, "source_index_entries").is_empty());
}

#[test]
fn failed_index_schema_migration_keeps_existing_rows_and_old_stamp() {
    let dir = with_legacy_db(
        "CREATE TABLE metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE wav_files (
            path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL,
            modified_ns INTEGER NOT NULL,
            tag INTEGER NOT NULL DEFAULT 0
        );
        INSERT INTO wav_files (path, file_size, modified_ns, tag)
        VALUES ('kept.wav', 10, 5, 2);
        CREATE VIEW source_index_entries AS SELECT path FROM wav_files;
        PRAGMA user_version = 10;",
    );

    assert!(SourceDatabase::open_for_test_fixture_source_write(dir.path()).is_err());

    let connection = rusqlite::Connection::open(dir.path().join(DB_FILE_NAME)).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT file_size, modified_ns, tag FROM wav_files WHERE path = 'kept.wav'",
                [],
                |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?
                ))
            )
            .unwrap(),
        (10, 5, 2)
    );
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        10
    );
}
