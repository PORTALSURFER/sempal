use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use tempfile::tempdir;

use super::super::{DB_FILE_NAME, Rating, SampleCollection, SampleSoundType, SourceDatabase};

use super::super::util::{EXACT_SUBTREE_PATH_PREDICATE, exact_subtree_path_bounds};

#[test]
fn browser_metadata_snapshot_reads_multi_collection_rows_coherently() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();
    db.upsert_file(Path::new("one.wav"), 10, 5).unwrap();
    db.set_tag(Path::new("one.wav"), Rating::new(2)).unwrap();
    db.set_locked(Path::new("one.wav"), true).unwrap();
    db.set_last_played_at(Path::new("one.wav"), 123).unwrap();
    let mut batch = db.write_batch().unwrap();
    batch
        .add_collection(Path::new("one.wav"), SampleCollection::new(3).unwrap())
        .unwrap();
    batch
        .add_collection(Path::new("one.wav"), SampleCollection::new(1).unwrap())
        .unwrap();
    batch.commit().unwrap();
    db.set_last_curated_at(Path::new("one.wav"), 456).unwrap();

    let snapshot = db.browser_metadata_snapshot().unwrap();

    assert_eq!(snapshot.files.len(), 1);
    let file = &snapshot.files[0];
    assert_eq!(file.relative_path, PathBuf::from("one.wav"));
    assert_eq!(file.rating, Rating::new(2));
    assert!(file.locked);
    assert_eq!(file.last_played_at, Some(123));
    assert_eq!(file.last_curated_at, Some(456));
    assert_eq!(
        file.collections,
        vec![
            SampleCollection::new(1).unwrap(),
            SampleCollection::new(3).unwrap()
        ]
    );
}

#[test]
fn browser_metadata_snapshot_has_constant_statement_count_for_large_sources() {
    let dir = tempdir().unwrap();
    let mut db = SourceDatabase::open_for_source_write(dir.path()).unwrap();
    let (_, empty_statement_count) =
        super::metadata_queries::browser_metadata_snapshot_statement_count(&db).unwrap();
    let transaction = db.connection.transaction().unwrap();
    {
        let mut insert = transaction
            .prepare(
                "INSERT INTO wav_files (path, file_size, modified_ns, extension)
                 VALUES (?1, 10, 5, 'wav')",
            )
            .unwrap();
        for index in 0..2_000 {
            insert.execute([format!("sample-{index:04}.wav")]).unwrap();
        }
    }
    transaction.commit().unwrap();

    let started_at = std::time::Instant::now();
    let (snapshot, statement_count) =
        super::metadata_queries::browser_metadata_snapshot_statement_count(&db).unwrap();

    assert_eq!(snapshot.files.len(), 2_000);
    assert_eq!(statement_count, empty_statement_count);
    assert!(
        statement_count <= 6,
        "browser metadata used {statement_count} statements"
    );
    assert!(
        started_at.elapsed() < std::time::Duration::from_secs(5),
        "large browser metadata snapshot exceeded its five-second test budget"
    );
}

#[test]
fn browser_metadata_snapshot_returns_typed_decode_failures() {
    let dir = tempdir().unwrap();
    let connection = Connection::open(dir.path().join(DB_FILE_NAME)).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE wav_files (path TEXT PRIMARY KEY, tag TEXT);
             INSERT INTO wav_files (path, tag) VALUES ('broken.wav', 'not-a-rating');",
        )
        .unwrap();
    drop(connection);
    let db = SourceDatabase::open_for_ui_read(dir.path()).unwrap();

    let error = db.browser_metadata_snapshot().unwrap_err();

    assert!(!error.to_string().is_empty());
}

#[test]
fn list_files_page_orders_supported_audio_and_applies_offsets() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();
    for name in [
        "delta.wav",
        "alpha.wav",
        "._appledouble.wav",
        "drums/._nested.wav",
        "notes.txt",
        "charlie.wav",
        "bravo.wav",
    ] {
        db.upsert_file(Path::new(name), 10, 5).unwrap();
    }

    let page = db.list_files_page(2, 1).unwrap();
    let paths = page
        .into_iter()
        .map(|entry| entry.relative_path)
        .collect::<Vec<_>>();

    assert_eq!(
        paths,
        vec![PathBuf::from("bravo.wav"), PathBuf::from("charlie.wav")]
    );
}

#[test]
fn pending_hash_query_is_bounded_and_excludes_missing_or_hashed_rows() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();
    for name in ["delta.wav", "alpha.wav", "charlie.wav", "bravo.wav"] {
        db.upsert_file(Path::new(name), 10, 5).unwrap();
    }
    db.connection
        .execute(
            "UPDATE wav_files SET content_hash = 'hash' WHERE path = 'alpha.wav'",
            [],
        )
        .unwrap();
    db.set_missing(Path::new("bravo.wav"), true).unwrap();

    let paths = db
        .list_pending_hash_files(1)
        .unwrap()
        .into_iter()
        .map(|entry| entry.relative_path)
        .collect::<Vec<_>>();

    assert_eq!(paths, vec![PathBuf::from("charlie.wav")]);
}

#[test]
fn audio_queries_ignore_appledouble_sidecars() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();
    for name in [
        "._alpha.wav",
        "alpha.wav",
        "drums/._kick.wav",
        "drums/kick.wav",
    ] {
        db.upsert_file(Path::new(name), 10, 5).unwrap();
    }

    let paths = db
        .list_files()
        .unwrap()
        .into_iter()
        .map(|entry| entry.relative_path)
        .collect::<Vec<_>>();

    assert_eq!(
        paths,
        vec![PathBuf::from("alpha.wav"), PathBuf::from("drums/kick.wav")]
    );
    assert!(
        db.entry_for_path(Path::new("._alpha.wav"))
            .unwrap()
            .is_none()
    );
    assert_eq!(db.index_for_path(Path::new("alpha.wav")).unwrap(), Some(0));
    assert_eq!(
        db.index_for_path(Path::new("drums/kick.wav")).unwrap(),
        Some(1)
    );
}

#[test]
fn list_queries_skip_invalid_relative_paths() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();
    db.upsert_file(Path::new("valid.wav"), 10, 5).unwrap();
    db.set_missing(Path::new("valid.wav"), true).unwrap();
    db.connection
        .execute(
            "INSERT INTO wav_files (path, file_size, modified_ns, content_hash, tag, looped, locked, missing, extension)
             VALUES (?1, 1, 1, NULL, 0, 0, 0, 1, 'wav')",
            params!["../escape.wav"],
        )
        .unwrap();
    db.connection
        .execute(
            "INSERT INTO wav_files (path, file_size, modified_ns, content_hash, tag, looped, locked, missing, extension)
             VALUES (?1, 1, 1, NULL, 0, 0, 0, 1, 'wav')",
            params!["C:/absolute.wav"],
        )
        .unwrap();

    let listed = db.list_files().unwrap();
    let missing = db.list_missing_paths().unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].relative_path, PathBuf::from("valid.wav"));
    assert_eq!(missing, vec![PathBuf::from("valid.wav")]);
}

#[test]
fn bpm_queries_return_only_present_rows_and_preserve_null_values() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();
    db.connection
        .execute(
            "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, bpm)
             VALUES (?1, 'h1', 1, 1, ?2)",
            params!["source::one.wav", 124.0f64],
        )
        .unwrap();
    db.connection
        .execute(
            "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, bpm)
             VALUES (?1, 'h2', 1, 1, NULL)",
            params!["source::two.wav"],
        )
        .unwrap();

    assert_eq!(
        db.bpm_for_sample_id("source::one.wav").unwrap(),
        Some(124.0)
    );
    assert_eq!(db.bpm_for_sample_id("source::two.wav").unwrap(), None);
    assert_eq!(db.bpm_for_sample_id("source::missing.wav").unwrap(), None);

    let lookup = db
        .bpms_for_sample_ids(&[
            String::from("source::one.wav"),
            String::from("source::two.wav"),
            String::from("source::missing.wav"),
        ])
        .unwrap();

    assert_eq!(lookup.get("source::one.wav"), Some(&Some(124.0)));
    assert_eq!(lookup.get("source::two.wav"), Some(&None));
    assert!(!lookup.contains_key("source::missing.wav"));
    assert!(db.bpms_for_sample_ids(&[]).unwrap().is_empty());
}

#[test]
fn search_entry_metadata_matches_row_order_and_values() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();
    db.upsert_file(Path::new("drums/snare.wav"), 10, 5).unwrap();
    db.upsert_file(Path::new("drums/kick.wav"), 10, 6).unwrap();
    db.set_tag(Path::new("drums/kick.wav"), Rating::KEEP_1)
        .unwrap();
    db.set_locked(Path::new("drums/snare.wav"), true).unwrap();
    db.set_last_played_at(Path::new("drums/snare.wav"), 42)
        .unwrap();

    let rows = db.list_search_entry_rows().unwrap();
    let metadata = db.list_search_entry_metadata().unwrap();

    assert_eq!(metadata.len(), rows.len());
    assert_eq!(
        metadata,
        rows.into_iter().map(|row| row.metadata).collect::<Vec<_>>()
    );
}

#[test]
fn sound_type_round_trips_for_path_queries() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();
    db.upsert_file(Path::new("drums/kick.wav"), 10, 5).unwrap();
    db.set_sound_type(Path::new("drums/kick.wav"), Some(SampleSoundType::Kick))
        .unwrap();

    assert_eq!(
        db.sound_type_for_path(Path::new("drums/kick.wav")).unwrap(),
        Some(SampleSoundType::Kick)
    );
}

#[test]
fn collection_reads_use_canonical_membership_rows() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();
    db.upsert_file(Path::new("one.wav"), 10, 5).unwrap();
    db.upsert_file(Path::new("two.wav"), 10, 5).unwrap();
    db.connection
        .execute(
            "UPDATE wav_files SET collection = 1 WHERE path IN ('one.wav', 'two.wav')",
            [],
        )
        .unwrap();
    db.connection
        .execute(
            "INSERT INTO wav_file_collections (path, collection) VALUES ('one.wav', 2)",
            [],
        )
        .unwrap();

    assert_eq!(
        db.collections_for_path(Path::new("one.wav")).unwrap(),
        vec![SampleCollection::new(2).unwrap()]
    );
    assert!(
        db.collections_for_path(Path::new("two.wav"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn legacy_read_only_collection_query_falls_back_to_wav_files_column() {
    let dir = tempdir().unwrap();
    let connection = Connection::open(dir.path().join(DB_FILE_NAME)).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE wav_files (
                path TEXT PRIMARY KEY,
                file_size INTEGER NOT NULL,
                modified_ns INTEGER NOT NULL,
                collection INTEGER
            );
            INSERT INTO wav_files (path, file_size, modified_ns, collection)
            VALUES ('one.wav', 10, 5, 3);",
        )
        .unwrap();
    drop(connection);

    let db = SourceDatabase::open_for_ui_read(dir.path()).unwrap();
    let expected = SampleCollection::new(3).unwrap();
    assert_eq!(
        db.collections_for_path(Path::new("one.wav")).unwrap(),
        vec![expected]
    );
    assert_eq!(
        db.collection_for_path(Path::new("one.wav")).unwrap(),
        Some(expected)
    );
}

#[test]
fn subtree_file_reads_use_an_exact_binary_path_range() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_source_write(dir.path()).unwrap();
    for path in [
        "drum_kits%_!/inside.wav",
        "drum_kits%_!/nested/deep.wav",
        "drum_kitsX_YX!/outside.wav",
        "Drum_kits%_!/case-distinct.wav",
    ] {
        db.upsert_file(Path::new(path), 10, 5).unwrap();
    }

    let paths = db
        .list_files_under_path(Path::new("drum_kits%_!"))
        .unwrap()
        .into_iter()
        .map(|entry| entry.relative_path)
        .collect::<Vec<_>>();

    assert_eq!(
        paths,
        vec![
            PathBuf::from("drum_kits%_!/inside.wav"),
            PathBuf::from("drum_kits%_!/nested/deep.wav"),
        ]
    );

    let (lower_bound, upper_bound) = exact_subtree_path_bounds("drum_kits%_!");
    let explain_sql = format!(
        "EXPLAIN QUERY PLAN SELECT path FROM wav_files WHERE {EXACT_SUBTREE_PATH_PREDICATE}"
    );
    let mut statement = db.connection.prepare(&explain_sql).unwrap();
    let details = statement
        .query_map(
            rusqlite::params!["drum_kits%_!", lower_bound, upper_bound],
            |row| row.get::<_, String>(3),
        )
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        details
            .iter()
            .any(|detail| detail.contains("path>? AND path<?")),
        "expected the exact subtree predicate to use the path index, got {details:?}"
    );
}

#[test]
fn legacy_read_only_database_without_pending_renames_returns_empty_list() {
    let dir = tempdir().unwrap();
    let connection = Connection::open(dir.path().join(DB_FILE_NAME)).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE wav_files (
                path TEXT PRIMARY KEY,
                file_size INTEGER NOT NULL,
                modified_ns INTEGER NOT NULL
            );",
        )
        .unwrap();
    drop(connection);

    let db = SourceDatabase::open_for_ui_read(dir.path()).unwrap();
    assert_eq!(db.list_pending_renames().unwrap(), Vec::new());
}

#[test]
fn legacy_read_only_pending_renames_project_optional_defaults() {
    let dir = tempdir().unwrap();
    let connection = Connection::open(dir.path().join(DB_FILE_NAME)).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE pending_wav_renames (
                path TEXT PRIMARY KEY,
                file_size INTEGER NOT NULL,
                modified_ns INTEGER NOT NULL,
                content_hash TEXT,
                tag INTEGER NOT NULL,
                looped INTEGER NOT NULL,
                locked INTEGER NOT NULL,
                last_played_at INTEGER,
                collection INTEGER
            );
            INSERT INTO pending_wav_renames (
                path, file_size, modified_ns, content_hash, tag, looped, locked,
                last_played_at, collection
            ) VALUES (
                'legacy.wav', 10, 5, 'hash-a', 1, 1, 1, 42, 2
            );",
        )
        .unwrap();
    drop(connection);

    let db = SourceDatabase::open_for_ui_read(dir.path()).unwrap();
    let pending = db.list_pending_renames().unwrap();
    assert_eq!(pending.len(), 1);
    let entry = &pending[0];
    assert_eq!(entry.relative_path, Path::new("legacy.wav"));
    assert_eq!(entry.file_size, 10);
    assert_eq!(entry.modified_ns, 5);
    assert_eq!(entry.content_hash.as_deref(), Some("hash-a"));
    assert_eq!(entry.file_identity, None);
    assert_eq!(entry.staged_generation, 0);
    assert_eq!(entry.staged_at, None);
    assert_eq!(entry.metadata.tag, Rating::KEEP_1);
    assert!(entry.metadata.looped);
    assert!(entry.metadata.locked);
    assert_eq!(entry.metadata.last_played_at, Some(42));
    assert_eq!(entry.metadata.sound_type, None);
    assert_eq!(entry.metadata.last_curated_at, None);
    assert_eq!(entry.metadata.user_tag, None);
    assert!(entry.metadata.normal_tags.is_empty());
    assert_eq!(
        entry.metadata.collections,
        vec![SampleCollection::new(2).unwrap()]
    );
    assert!(!entry.metadata.tag_named);
    assert_eq!(
        db.pending_rename_diagnostics().unwrap(),
        super::super::PendingRenameDiagnostics {
            candidate_count: 1,
            authoritative_generation: 0,
            oldest_staged_generation: Some(0),
            oldest_staged_at: None,
            oldest_candidate_age_seconds: None,
        }
    );
}

#[test]
fn legacy_read_only_database_without_last_curated_at_preserves_saved_metadata() {
    let dir = tempdir().unwrap();
    let connection = Connection::open(dir.path().join(DB_FILE_NAME)).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE wav_files (
                path TEXT PRIMARY KEY,
                file_size INTEGER NOT NULL,
                modified_ns INTEGER NOT NULL,
                content_hash TEXT,
                tag INTEGER NOT NULL DEFAULT 0,
                looped INTEGER NOT NULL DEFAULT 0,
                sound_type TEXT,
                locked INTEGER NOT NULL DEFAULT 0,
                missing INTEGER NOT NULL DEFAULT 0,
                extension TEXT NOT NULL DEFAULT '',
                last_played_at INTEGER,
                user_tag TEXT,
                tag_named INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE source_tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                normalized_text TEXT NOT NULL UNIQUE,
                display_label TEXT NOT NULL
            );
            CREATE TABLE wav_file_tags (
                path TEXT NOT NULL,
                tag_id INTEGER NOT NULL,
                PRIMARY KEY (path, tag_id)
            ) WITHOUT ROWID;",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO wav_files (
                path, file_size, modified_ns, content_hash, tag, looped, sound_type,
                locked, missing, extension, last_played_at, user_tag, tag_named
             )
             VALUES (?1, 2048, 99, ?2, ?3, 1, ?4, 1, 0, 'wav', 123, ?5, 1)",
            params![
                "drums/kick.wav",
                "hash-kick",
                Rating::KEEP_1.as_i64(),
                SampleSoundType::Kick.token(),
                "808",
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO source_tags (normalized_text, display_label) VALUES (?1, ?2)",
            params!["warm", "Warm"],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO wav_file_tags (path, tag_id) VALUES (?1, ?2)",
            params!["drums/kick.wav", connection.last_insert_rowid()],
        )
        .unwrap();
    drop(connection);

    let db = SourceDatabase::open_for_ui_read(dir.path()).unwrap();
    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.relative_path, PathBuf::from("drums/kick.wav"));
    assert_eq!(row.content_hash.as_deref(), Some("hash-kick"));
    assert_eq!(row.tag, Rating::KEEP_1);
    assert!(row.looped);
    assert_eq!(row.sound_type, Some(SampleSoundType::Kick));
    assert!(row.locked);
    assert_eq!(row.last_played_at, Some(123));
    assert_eq!(row.last_curated_at, None);
    assert_eq!(row.user_tag.as_deref(), Some("808"));
    assert!(row.tag_named);
    assert_eq!(row.normal_tags, vec![String::from("Warm")]);

    let entry = db
        .entry_for_path(Path::new("drums/kick.wav"))
        .unwrap()
        .expect("legacy row should hydrate");
    assert_eq!(entry.tag, Rating::KEEP_1);

    let search_rows = db.list_search_entry_rows().unwrap();
    assert_eq!(search_rows.len(), 1);
    assert_eq!(search_rows[0].metadata.tag, Rating::KEEP_1);
    assert_eq!(search_rows[0].metadata.last_played_at, Some(123));
    assert_eq!(search_rows[0].metadata.last_curated_at, None);
    assert_eq!(
        search_rows[0].metadata.normal_tags,
        vec![String::from("Warm")]
    );
    assert!(search_rows[0].metadata.tag_named);

    let search_metadata = db.list_search_entry_metadata().unwrap();
    assert_eq!(search_metadata, vec![search_rows[0].metadata.clone()]);
    assert_eq!(
        db.last_curated_at_for_path(Path::new("drums/kick.wav"))
            .unwrap(),
        None
    );
}

#[test]
fn legacy_read_only_minimal_wav_files_schema_reads_with_defaults() {
    let dir = tempdir().unwrap();
    let connection = Connection::open(dir.path().join(DB_FILE_NAME)).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE wav_files (
                path TEXT PRIMARY KEY,
                file_size INTEGER NOT NULL,
                modified_ns INTEGER NOT NULL
            );
            INSERT INTO wav_files (path, file_size, modified_ns)
            VALUES
                ('nested/One.WAV', 2048, 99),
                ('nested/._sidecar.wav', 1, 1),
                ('notes.txt', 1, 1);",
        )
        .unwrap();
    drop(connection);

    let db = SourceDatabase::open_for_ui_read(dir.path()).unwrap();
    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.relative_path, PathBuf::from("nested/One.WAV"));
    assert_eq!(row.content_hash, None);
    assert_eq!(row.tag, Rating::NEUTRAL);
    assert!(!row.looped);
    assert_eq!(row.sound_type, None);
    assert!(!row.locked);
    assert!(!row.missing);
    assert_eq!(row.last_played_at, None);
    assert_eq!(row.last_curated_at, None);
    assert_eq!(row.user_tag, None);
    assert!(!row.tag_named);
    assert!(row.normal_tags.is_empty());
    let browser_snapshot = db.browser_metadata_snapshot().unwrap();
    assert_eq!(browser_snapshot.revision, 0);
    assert_eq!(browser_snapshot.files.len(), 1);
    assert_eq!(
        browser_snapshot.files[0].relative_path,
        PathBuf::from("nested/One.WAV")
    );
    assert_eq!(browser_snapshot.files[0].rating, Rating::NEUTRAL);
    assert_eq!(browser_snapshot.files[0].file_size, 2048);
    assert_eq!(browser_snapshot.files[0].modified_ns, 99);
    assert!(!browser_snapshot.files[0].missing);
    assert!(browser_snapshot.files[0].collections.is_empty());
    assert_eq!(db.count_files().unwrap(), 1);
    assert_eq!(db.count_present_files().unwrap(), 1);
    assert_eq!(db.list_files_page(10, 0).unwrap().len(), 1);
    assert!(
        db.entry_for_path(Path::new("nested/One.WAV"))
            .unwrap()
            .is_some()
    );
    assert!(
        db.list_paths_with_content_hash("missing")
            .unwrap()
            .is_empty()
    );

    let search_rows = db.list_search_entry_rows().unwrap();
    assert_eq!(search_rows.len(), 1);
    assert_eq!(search_rows[0].metadata.tag, Rating::NEUTRAL);
    assert_eq!(search_rows[0].metadata.last_played_at, None);
    assert_eq!(search_rows[0].metadata.last_curated_at, None);
    assert!(search_rows[0].metadata.normal_tags.is_empty());
    assert!(!search_rows[0].metadata.tag_named);
    assert_eq!(
        db.list_search_entry_rows_for_paths(&[PathBuf::from("nested/One.WAV")])
            .unwrap()
            .len(),
        1
    );
    assert_eq!(db.list_search_entry_metadata().unwrap().len(), 1);
    assert!(
        db.collections_for_path(Path::new("nested/One.WAV"))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.collection_for_path(Path::new("nested/One.WAV")).unwrap(),
        None
    );
}
