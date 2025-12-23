use rusqlite::Connection;

use super::SourceDbError;
use super::util::map_sql_error;

pub(super) fn apply_schema(connection: &Connection) -> Result<(), SourceDbError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
             CREATE TABLE IF NOT EXISTS wav_files (
                path TEXT PRIMARY KEY,
                file_size INTEGER NOT NULL,
                modified_ns INTEGER NOT NULL,
                tag INTEGER NOT NULL DEFAULT 0,
                missing INTEGER NOT NULL DEFAULT 0
            );",
        )
        .map_err(map_sql_error)?;
    ensure_optional_columns(connection)?;
    connection
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_wav_files_missing
                 ON wav_files(path) WHERE missing != 0;",
        )
        .map_err(map_sql_error)?;
    Ok(())
}

fn ensure_optional_columns(connection: &Connection) -> Result<(), SourceDbError> {
    let mut stmt = connection
        .prepare("PRAGMA table_info(wav_files)")
        .map_err(map_sql_error)?;
    let columns: std::collections::HashSet<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(map_sql_error)?
        .filter_map(Result::ok)
        .collect();
    if !columns.contains("tag") {
        connection
            .execute(
                "ALTER TABLE wav_files ADD COLUMN tag INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(map_sql_error)?;
    }
    if !columns.contains("missing") {
        connection
            .execute(
                "ALTER TABLE wav_files ADD COLUMN missing INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(map_sql_error)?;
    }
    if !columns.contains("content_hash") {
        connection
            .execute("ALTER TABLE wav_files ADD COLUMN content_hash TEXT", [])
            .map_err(map_sql_error)?;
    }
    Ok(())
}
