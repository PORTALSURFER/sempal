use super::*;

#[test]
fn legacy_analysis_runtime_migration_is_selective_and_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE analysis_jobs (
            id INTEGER PRIMARY KEY,
            job_type TEXT NOT NULL,
            status TEXT NOT NULL,
            readiness_managed INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE analysis_job_progress_snapshots (
            job_type TEXT PRIMARY KEY,
            pending INTEGER NOT NULL DEFAULT 0,
            running INTEGER NOT NULL DEFAULT 0,
            done INTEGER NOT NULL DEFAULT 0,
            failed INTEGER NOT NULL DEFAULT 0
        ) WITHOUT ROWID;
        INSERT INTO analysis_jobs VALUES
            (1, 'wav_metadata_v1', 'pending', 0),
            (2, 'wav_metadata_v1', 'running', 0),
            (3, 'embedding_backfill_v1', 'failed', 0),
            (4, 'rebuild_index_v1', 'done', 0),
            (5, 'wav_metadata_v1', 'running', 1),
            (6, 'third_party_analysis_v1', 'pending', 0);
        INSERT INTO analysis_job_progress_snapshots
            (job_type, pending, running, done, failed)
        VALUES
            ('wav_metadata_v1', 1, 2, 0, 0),
            ('embedding_backfill_v1', 0, 0, 0, 1),
            ('rebuild_index_v1', 0, 0, 1, 0),
            ('third_party_analysis_v1', 1, 0, 0, 0);",
    )
    .unwrap();

    retire_legacy_analysis_runtime_state(&conn).unwrap();
    retire_legacy_analysis_runtime_state(&conn).unwrap();

    let retained_jobs = conn
        .prepare(
            "SELECT job_type, readiness_managed
             FROM analysis_jobs
             ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        retained_jobs,
        vec![
            (String::from("wav_metadata_v1"), 1),
            (String::from("third_party_analysis_v1"), 0),
        ]
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM analysis_job_progress_snapshots",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
}

#[test]
fn legacy_analysis_runtime_migration_rolls_back_and_retries_after_interruption() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE analysis_jobs (
            id INTEGER PRIMARY KEY,
            job_type TEXT NOT NULL,
            readiness_managed INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE analysis_job_progress_snapshots (
            job_type TEXT PRIMARY KEY
        ) WITHOUT ROWID;
        INSERT INTO analysis_jobs VALUES (1, 'wav_metadata_v1', 0);
        INSERT INTO analysis_job_progress_snapshots VALUES ('wav_metadata_v1');
        CREATE TRIGGER interrupt_legacy_cleanup
        BEFORE DELETE ON analysis_job_progress_snapshots
        BEGIN
            SELECT RAISE(ABORT, 'synthetic migration interruption');
        END;",
    )
    .unwrap();

    assert!(retire_legacy_analysis_runtime_state(&conn).is_err());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM analysis_jobs", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        1,
        "the failed transaction must restore the earlier job deletion"
    );

    conn.execute("DROP TRIGGER interrupt_legacy_cleanup", [])
        .unwrap();
    retire_legacy_analysis_runtime_state(&conn).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM analysis_jobs", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
}

#[test]
fn analysis_jobs_backfill_blank_identity_columns() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE analysis_jobs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sample_id TEXT NOT NULL,
            source_id TEXT NOT NULL DEFAULT '',
            relative_path TEXT NOT NULL DEFAULT '',
            job_type TEXT NOT NULL,
            status TEXT NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL
        );
        INSERT INTO analysis_jobs (
            sample_id, source_id, relative_path, job_type, status, attempts, created_at
        ) VALUES (
            'source-a::Pack/a.wav', '', '', 'analyze_sample', 'pending', 0, 0
        );",
    )
    .unwrap();

    ensure_analysis_jobs_optional_columns(&conn).unwrap();

    let row: (String, String) = conn
        .query_row(
            "SELECT source_id, relative_path FROM analysis_jobs",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(row.0, "source-a");
    assert_eq!(row.1, "Pack/a.wav");
}

#[test]
fn pending_rename_migration_adds_extended_metadata_columns() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE pending_wav_renames (
            path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL,
            modified_ns INTEGER NOT NULL,
            content_hash TEXT,
            tag INTEGER NOT NULL,
            looped INTEGER NOT NULL,
            locked INTEGER NOT NULL,
            last_played_at INTEGER
        );",
    )
    .unwrap();

    ensure_pending_rename_optional_columns(&conn).unwrap();

    let columns = table_columns(&conn, "pending_wav_renames").unwrap();
    assert!(columns.contains("sound_type"));
    assert!(columns.contains("user_tag"));
    assert!(columns.contains("normal_tags"));
    assert!(columns.contains("file_identity"));
    assert!(columns.contains("staged_generation"));
    assert!(columns.contains("staged_at"));
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*)
             FROM sqlite_master
             WHERE type = 'index' AND name = 'idx_pending_wav_renames_generation'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*)
             FROM sqlite_master
             WHERE type = 'index' AND name = 'idx_pending_wav_renames_identity_facts'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
}

#[test]
fn pending_rename_destination_migration_adds_retained_hash() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE pending_wav_rename_destinations (
            path TEXT PRIMARY KEY,
            scan_generation INTEGER NOT NULL
        );",
    )
    .unwrap();

    ensure_pending_rename_destination_schema(&conn).unwrap();

    let columns = table_columns(&conn, "pending_wav_rename_destinations").unwrap();
    assert!(columns.contains("retained_hash"));
}

#[test]
fn wav_file_migration_adds_stable_file_identity_column() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE wav_files (
            path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL,
            modified_ns INTEGER NOT NULL
        );",
    )
    .unwrap();

    ensure_wav_files_optional_columns(&conn).unwrap();

    let columns = table_columns(&conn, "wav_files").unwrap();
    assert!(columns.contains("file_identity"));
}

#[test]
fn canonical_identity_migration_rebuilds_derived_readiness_state() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE wav_files (
            path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL,
            modified_ns INTEGER NOT NULL,
            file_identity TEXT
        );
        CREATE TABLE pending_wav_renames (
            path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL,
            modified_ns INTEGER NOT NULL,
            content_hash TEXT,
            tag INTEGER NOT NULL,
            looped INTEGER NOT NULL,
            locked INTEGER NOT NULL,
            last_played_at INTEGER,
            file_identity TEXT
        );
        CREATE TABLE source_readiness_targets (
            source_id TEXT NOT NULL,
            scope_kind TEXT NOT NULL,
            scope_id TEXT NOT NULL
        );
        CREATE TABLE source_readiness_artifacts (
            source_id TEXT NOT NULL,
            scope_kind TEXT NOT NULL,
            scope_id TEXT NOT NULL
        );
        CREATE TABLE source_readiness_sources (
            source_id TEXT PRIMARY KEY
        );
        CREATE TABLE metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE analysis_jobs (
            id INTEGER PRIMARY KEY,
            sample_id TEXT NOT NULL,
            readiness_managed INTEGER NOT NULL,
            readiness_scope_kind TEXT,
            readiness_scope_id TEXT
        );
        INSERT INTO wav_files (path, file_size, modified_ns, file_identity) VALUES
            ('unix-v2.wav', 1, 1, 'unix-v2:10:20:30'),
            ('windows-v2.wav', 1, 1, 'windows-v2:40:50:60'),
            ('canonical.wav', 1, 1, 'unix:70:80:90'),
            ('obsolete.wav', 1, 1, 'unix:100:110');
        INSERT INTO pending_wav_renames (
            path, file_size, modified_ns, content_hash, tag, looped, locked,
            last_played_at, file_identity
        ) VALUES (
            'pending.wav', 1, 1, NULL, 0, 0, 0, NULL, 'windows:120:130'
        );
        INSERT INTO source_readiness_targets VALUES
            ('source', 'file', 'unix-v2:10:20:30'),
            ('source', 'file', 'unix:100:110');
        INSERT INTO source_readiness_artifacts VALUES
            ('source', 'file', 'windows-v2:40:50:60'),
            ('source', 'file', 'windows:120:130');
        INSERT INTO analysis_jobs VALUES
            (1, 'unix-v2:10:20:30', 1, 'file', 'unix-v2:10:20:30'),
            (2, 'manual-job', 0, NULL, NULL);
        INSERT INTO source_readiness_sources VALUES ('source');
        INSERT INTO metadata VALUES ('readiness_target_fingerprint_v1', 'stale');
        INSERT INTO metadata VALUES ('unrelated', 'preserved');",
    )
    .unwrap();

    migrate_canonical_file_identities(&conn).unwrap();

    let identities = conn
        .prepare("SELECT path, file_identity FROM wav_files ORDER BY path")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        identities,
        vec![
            (
                String::from("canonical.wav"),
                Some(String::from("unix:70:80:90")),
            ),
            (String::from("obsolete.wav"), None),
            (
                String::from("unix-v2.wav"),
                Some(String::from("unix:10:20:30")),
            ),
            (
                String::from("windows-v2.wav"),
                Some(String::from("windows:40:50:60")),
            ),
        ]
    );
    assert_eq!(
        conn.query_row(
            "SELECT file_identity FROM pending_wav_renames WHERE path = 'pending.wav'",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .unwrap(),
        None
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM source_readiness_targets", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM source_readiness_artifacts",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT sample_id FROM analysis_jobs", [], |row| row
            .get::<_, String>(0),)
            .unwrap(),
        "manual-job"
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM source_readiness_sources", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT value FROM metadata WHERE key = 'unrelated'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "preserved"
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM metadata
             WHERE key = 'readiness_target_fingerprint_v1'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
}

#[test]
fn collection_membership_schema_backfills_legacy_collection_column() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE wav_files (
            path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL,
            modified_ns INTEGER NOT NULL,
            collection INTEGER
        );
        INSERT INTO wav_files (path, file_size, modified_ns, collection)
        VALUES ('nested/One.WAV', 10, 5, 2);",
    )
    .unwrap();

    ensure_collection_membership_schema(&conn).unwrap();

    let collection: i64 = conn
        .query_row(
            "SELECT collection
             FROM wav_file_collections
             WHERE path = 'nested/One.WAV'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(collection, 2);
}

#[test]
fn aspect_descriptor_schema_creates_current_and_cache_tables() {
    let conn = Connection::open_in_memory().unwrap();

    ensure_aspect_descriptor_tables(&conn).unwrap();

    let current_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM sqlite_master
             WHERE type = 'table' AND name = 'similarity_aspect_descriptors'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let cache_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM sqlite_master
             WHERE type = 'table' AND name = 'analysis_cache_aspect_descriptors'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(current_exists, 1);
    assert_eq!(cache_exists, 1);
}

#[test]
fn readiness_schema_repairs_current_stamped_analysis_storage() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE analysis_jobs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sample_id TEXT NOT NULL,
            source_id TEXT NOT NULL DEFAULT '',
            relative_path TEXT NOT NULL DEFAULT '',
            job_type TEXT NOT NULL,
            status TEXT NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            UNIQUE(sample_id, job_type)
        );
        CREATE TABLE source_readiness_sources (
            source_id TEXT PRIMARY KEY,
            source_generation INTEGER NOT NULL,
            availability TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        ) WITHOUT ROWID;
        CREATE TABLE source_readiness_artifacts (
            source_id TEXT NOT NULL,
            scope_kind TEXT NOT NULL,
            scope_id TEXT NOT NULL,
            stage TEXT NOT NULL,
            artifact_version TEXT NOT NULL,
            source_generation INTEGER NOT NULL,
            content_generation TEXT NOT NULL,
            completed_at INTEGER NOT NULL,
            PRIMARY KEY (source_id, scope_kind, scope_id, stage)
        ) WITHOUT ROWID;",
    )
    .unwrap();

    ensure_analysis_jobs_optional_columns(&conn).unwrap();
    ensure_source_readiness_schema(&conn).unwrap();

    let job_columns = table_columns(&conn, "analysis_jobs").unwrap();
    assert!(job_columns.contains("readiness_managed"));
    assert!(job_columns.contains("readiness_claim_generation"));
    assert!(job_columns.contains("source_generation"));
    assert!(job_columns.contains("lease_expires_at"));
    let source_columns = table_columns(&conn, "source_readiness_sources").unwrap();
    assert!(source_columns.contains("readiness_revision"));
    assert!(source_columns.contains("contract_version"));
    assert!(source_columns.contains("membership_digest"));
    assert!(source_columns.contains("membership_count"));
    let artifact_columns = table_columns(&conn, "source_readiness_artifacts").unwrap();
    assert!(artifact_columns.contains("relative_path"));
    assert!(artifact_columns.contains("artifact_ref"));
    for table in [
        "source_readiness_sources",
        "source_readiness_targets",
        "source_readiness_artifacts",
    ] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
                )",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists, "missing readiness table {table}");
    }
}

#[test]
fn source_index_migration_adds_lossless_path_encoding_to_legacy_tables() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE source_index_entries (
            path TEXT PRIMARY KEY,
            classification TEXT NOT NULL,
            file_size INTEGER,
            modified_ns INTEGER,
            file_identity TEXT,
            diagnostic TEXT,
            format_policy_version INTEGER NOT NULL
        ) WITHOUT ROWID;
        INSERT INTO source_index_entries (
            path, classification, file_size, modified_ns, format_policy_version
        ) VALUES ('notes.txt', 'unsupported_non_audio', 5, 10, 1);",
    )
    .unwrap();

    ensure_source_index_schema(&conn).unwrap();

    assert!(
        table_columns(&conn, "source_index_entries")
            .unwrap()
            .contains("path_encoding")
    );
    assert_eq!(
        conn.query_row(
            "SELECT path_encoding FROM source_index_entries WHERE path = 'notes.txt'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
}
