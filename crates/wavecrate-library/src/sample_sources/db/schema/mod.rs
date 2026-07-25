//! SQLite schema creation and migration helpers for source databases.

use rusqlite::Connection;

use super::SourceDbError;
use super::util::map_sql_error;

mod ddl;
mod migrations;

pub(crate) use migrations::table_columns;

/// SQLite `user_version` value for the current source-db schema shape.
///
/// A matching stamp means the file has already passed the full schema-assurance
/// path once. Current-stamped opens still run low-cost additive table/column
/// repairs, but they skip index rebuilds and deferred cleanup work.
pub(super) const SOURCE_DB_SCHEMA_VERSION: i64 = 11;

/// Apply the full source-database schema, including deferred cleanup work.
pub(super) fn apply_schema(connection: &Connection) -> Result<SchemaApplyOutcome, SourceDbError> {
    apply_schema_with_mode(connection, SchemaApplyMode::Full)
}

/// Apply the source-database schema using only startup-friendly migration work.
pub(super) fn apply_schema_fast(
    connection: &Connection,
) -> Result<SchemaApplyOutcome, SourceDbError> {
    apply_schema_with_mode(connection, SchemaApplyMode::Fast)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SchemaApplyMode {
    Fast,
    Full,
}

/// Outcome describing whether schema assurance ran for this open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SchemaApplyOutcome {
    /// The connection performed schema assurance because the database was new or stale.
    Assured,
    /// The database already carried the current schema stamp, so only lightweight repairs ran.
    Current,
}

fn apply_schema_with_mode(
    connection: &Connection,
    mode: SchemaApplyMode,
) -> Result<SchemaApplyOutcome, SourceDbError> {
    let outcome = if schema_is_current(connection)? {
        ddl::apply_base_schema(connection)?;
        migrations::apply_current_stamp_repairs(connection)?;
        SchemaApplyOutcome::Current
    } else {
        assure_schema(connection)?;
        SchemaApplyOutcome::Assured
    };
    if mode == SchemaApplyMode::Full {
        migrations::remove_invalid_relative_paths(connection)?;
    }
    Ok(outcome)
}

fn assure_schema(connection: &Connection) -> Result<(), SourceDbError> {
    ddl::apply_base_schema(connection)?;
    migrations::apply_optional_migrations(connection)?;
    ddl::apply_indices(connection)?;
    stamp_schema_version(connection)?;
    Ok(())
}

fn schema_is_current(connection: &Connection) -> Result<bool, SourceDbError> {
    Ok(read_schema_version(connection)? == SOURCE_DB_SCHEMA_VERSION)
}

fn read_schema_version(connection: &Connection) -> Result<i64, SourceDbError> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(map_sql_error)
}

fn stamp_schema_version(connection: &Connection) -> Result<(), SourceDbError> {
    connection
        .pragma_update(None, "user_version", SOURCE_DB_SCHEMA_VERSION)
        .map_err(map_sql_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::OptionalExtension;

    fn index_exists(connection: &Connection, index_name: &str) -> bool {
        connection
            .query_row(
                "SELECT name FROM sqlite_master WHERE type = 'index' AND name = ?1",
                [index_name],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .unwrap()
            .is_some()
    }

    #[test]
    fn stamped_databases_skip_repeat_schema_assurance() {
        let connection = Connection::open_in_memory().unwrap();

        let first = apply_schema_fast(&connection).unwrap();
        assert_eq!(first, SchemaApplyOutcome::Assured);
        assert_eq!(
            read_schema_version(&connection).unwrap(),
            SOURCE_DB_SCHEMA_VERSION
        );
        connection
            .execute("DROP INDEX idx_wav_files_missing", [])
            .unwrap();
        assert!(!index_exists(&connection, "idx_wav_files_missing"));

        let second = apply_schema_fast(&connection).unwrap();
        assert_eq!(second, SchemaApplyOutcome::Current);
        assert!(!index_exists(&connection, "idx_wav_files_missing"));
    }

    #[test]
    fn stale_schema_stamp_reapplies_schema_assurance() {
        let connection = Connection::open_in_memory().unwrap();
        apply_schema_fast(&connection).unwrap();
        connection
            .execute("DROP INDEX idx_wav_files_missing", [])
            .unwrap();
        connection.pragma_update(None, "user_version", 0).unwrap();

        let outcome = apply_schema_fast(&connection).unwrap();
        assert_eq!(outcome, SchemaApplyOutcome::Assured);
        assert!(index_exists(&connection, "idx_wav_files_missing"));
        assert_eq!(
            read_schema_version(&connection).unwrap(),
            SOURCE_DB_SCHEMA_VERSION
        );
    }

    #[test]
    fn v9_upgrade_retires_only_known_non_readiness_jobs() {
        let connection = Connection::open_in_memory().unwrap();
        apply_schema_fast(&connection).unwrap();
        connection.pragma_update(None, "user_version", 8).unwrap();
        connection
            .execute_batch(
                "INSERT INTO analysis_jobs (
                    sample_id, source_id, relative_path, job_type, status, attempts, created_at,
                    readiness_managed
                 ) VALUES
                    ('legacy::a.wav', 'legacy', 'a.wav', 'wav_metadata_v1',
                     'running', 0, 1, 0),
                    ('ready::a.wav', 'ready', 'a.wav', 'readiness_analysis_features_v1',
                     'running', 0, 1, 1),
                    ('unknown::a.wav', 'unknown', 'a.wav', 'third_party_analysis_v1',
                     'pending', 0, 1, 0);",
            )
            .unwrap();

        assert_eq!(
            apply_schema_fast(&connection).unwrap(),
            SchemaApplyOutcome::Assured
        );

        let retained = connection
            .prepare(
                "SELECT job_type, readiness_managed
                 FROM analysis_jobs
                 ORDER BY job_type",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            retained,
            vec![
                ("readiness_analysis_features_v1".to_string(), 1),
                ("third_party_analysis_v1".to_string(), 0),
            ]
        );
        assert_eq!(
            read_schema_version(&connection).unwrap(),
            SOURCE_DB_SCHEMA_VERSION
        );
    }

    #[test]
    fn v9_upgrade_waits_for_a_concurrent_legacy_writer_and_retires_its_commit() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("source.sqlite");
        let bootstrap = Connection::open(&database_path).unwrap();
        apply_schema_fast(&bootstrap).unwrap();
        bootstrap.pragma_update(None, "user_version", 8).unwrap();
        drop(bootstrap);

        let writer = Connection::open(&database_path).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        writer
            .execute(
                "INSERT INTO analysis_jobs (
                    sample_id, source_id, relative_path, job_type, status, attempts, created_at,
                    readiness_managed
                 ) VALUES (
                    'legacy::late.wav', 'legacy', 'late.wav', 'embedding_backfill_v1',
                    'running', 0, 1, 0
                 )",
                [],
            )
            .unwrap();

        let migration_path = database_path.clone();
        let migration = std::thread::spawn(move || {
            let connection = Connection::open(migration_path).unwrap();
            connection
                .busy_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            apply_schema_fast(&connection).unwrap();
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        writer.execute_batch("COMMIT").unwrap();
        migration.join().unwrap();

        let verification = Connection::open(database_path).unwrap();
        assert_eq!(
            verification
                .query_row(
                    "SELECT COUNT(*) FROM analysis_jobs
                     WHERE readiness_managed = 0
                       AND job_type = 'embedding_backfill_v1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(
            read_schema_version(&verification).unwrap(),
            SOURCE_DB_SCHEMA_VERSION
        );
    }

    #[test]
    fn current_stamp_repairs_missing_curation_columns() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE wav_files (
                    path TEXT PRIMARY KEY,
                    file_size INTEGER NOT NULL,
                    modified_ns INTEGER NOT NULL,
                    tag INTEGER NOT NULL DEFAULT 0,
                    looped INTEGER NOT NULL DEFAULT 0,
                    locked INTEGER NOT NULL DEFAULT 0,
                    missing INTEGER NOT NULL DEFAULT 0,
                    extension TEXT NOT NULL DEFAULT ''
                );
                CREATE TABLE file_ops_journal (
                    id TEXT PRIMARY KEY,
                    op_type TEXT NOT NULL,
                    stage TEXT NOT NULL,
                    target_relative TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                );",
            )
            .unwrap();
        connection
            .pragma_update(None, "user_version", SOURCE_DB_SCHEMA_VERSION)
            .unwrap();

        let outcome = apply_schema_fast(&connection).unwrap();

        assert_eq!(outcome, SchemaApplyOutcome::Current);
        let wav_columns = table_columns(&connection, "wav_files").unwrap();
        assert!(wav_columns.contains("last_curated_at"));
        assert!(wav_columns.contains("collection"));
        let journal_columns = table_columns(&connection, "file_ops_journal").unwrap();
        assert!(journal_columns.contains("last_curated_at"));
    }
}
