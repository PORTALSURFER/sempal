use rusqlite::Connection;

use super::super::super::SourceDbError;
use super::super::super::util::map_sql_error;
use super::table_columns;

pub(super) fn ensure_wav_files_optional_columns(
    connection: &Connection,
) -> Result<(), SourceDbError> {
    let columns = table_columns(connection, "wav_files")?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("wav_files", "tag", "INTEGER NOT NULL DEFAULT 0"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("wav_files", "missing", "INTEGER NOT NULL DEFAULT 0"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("wav_files", "looped", "INTEGER NOT NULL DEFAULT 0"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("wav_files", "locked", "INTEGER NOT NULL DEFAULT 0"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("wav_files", "sound_type", "TEXT"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("wav_files", "user_tag", "TEXT"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("wav_files", "tag_named", "INTEGER NOT NULL DEFAULT 0"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("wav_files", "content_hash", "TEXT"),
    )?;
    ensure_wav_file_extension_column(connection, &columns)?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("wav_files", "last_played_at", "INTEGER"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("wav_files", "last_curated_at", "INTEGER"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("wav_files", "collection", "INTEGER"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("wav_files", "file_identity", "TEXT"),
    )?;
    Ok(())
}

pub(super) fn ensure_pending_rename_optional_columns(
    connection: &Connection,
) -> Result<(), SourceDbError> {
    let columns = table_columns(connection, "pending_wav_renames")?;
    if columns.is_empty() {
        return Ok(());
    }
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("pending_wav_renames", "sound_type", "TEXT"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("pending_wav_renames", "user_tag", "TEXT"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("pending_wav_renames", "last_curated_at", "INTEGER"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("pending_wav_renames", "normal_tags", "TEXT"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("pending_wav_renames", "collection", "INTEGER"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("pending_wav_renames", "collections", "TEXT"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new(
            "pending_wav_renames",
            "tag_named",
            "INTEGER NOT NULL DEFAULT 0",
        ),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("pending_wav_renames", "file_identity", "TEXT"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new(
            "pending_wav_renames",
            "staged_generation",
            "INTEGER NOT NULL DEFAULT 0",
        ),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("pending_wav_renames", "staged_at", "INTEGER"),
    )?;
    connection
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_pending_wav_renames_generation
                 ON pending_wav_renames (staged_generation);
             CREATE INDEX IF NOT EXISTS idx_pending_wav_renames_identity_facts
                 ON pending_wav_renames (file_identity, file_size, modified_ns);",
        )
        .map_err(map_sql_error)?;
    Ok(())
}

pub(super) fn ensure_file_ops_journal_optional_columns(
    connection: &Connection,
) -> Result<(), SourceDbError> {
    let columns = table_columns(connection, "file_ops_journal")?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("file_ops_journal", "locked", "INTEGER"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("file_ops_journal", "last_curated_at", "INTEGER"),
    )?;
    Ok(())
}

pub(super) fn ensure_samples_optional_columns(
    connection: &Connection,
) -> Result<(), SourceDbError> {
    let columns = table_columns(connection, "samples")?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("samples", "duration_seconds", "REAL"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("samples", "sr_used", "INTEGER"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("samples", "analysis_version", "TEXT"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("samples", "bpm", "REAL"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new("samples", "long_sample_mark", "INTEGER"),
    )?;
    Ok(())
}

pub(super) fn ensure_feature_metric_columns(
    connection: &Connection,
    table_name: &str,
) -> Result<(), SourceDbError> {
    let columns = table_columns(connection, table_name)?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new(table_name, "light_dsp_blob", "BLOB"),
    )?;
    add_column_if_missing(
        connection,
        &columns,
        OptionalColumn::new(table_name, "rms", "REAL"),
    )?;
    Ok(())
}

fn ensure_wav_file_extension_column(
    connection: &Connection,
    columns: &std::collections::HashSet<String>,
) -> Result<(), SourceDbError> {
    if columns.contains("extension") {
        return Ok(());
    }
    connection
        .execute(
            "ALTER TABLE wav_files ADD COLUMN extension TEXT NOT NULL DEFAULT ''",
            [],
        )
        .map_err(map_sql_error)?;
    connection
        .execute(
            "UPDATE wav_files
             SET extension = lower(substr(path, instr(path, '.') + 1))
             WHERE extension = '' AND instr(path, '.') > 0",
            [],
        )
        .map_err(map_sql_error)?;
    Ok(())
}

fn add_column_if_missing(
    connection: &Connection,
    columns: &std::collections::HashSet<String>,
    column: OptionalColumn<'_>,
) -> Result<(), SourceDbError> {
    if columns.contains(column.name) {
        return Ok(());
    }
    connection
        .execute(
            &format!(
                "ALTER TABLE {} ADD COLUMN {} {}",
                column.table, column.name, column.definition
            ),
            [],
        )
        .map_err(map_sql_error)?;
    Ok(())
}

#[derive(Clone, Copy)]
struct OptionalColumn<'a> {
    table: &'a str,
    name: &'a str,
    definition: &'a str,
}

impl<'a> OptionalColumn<'a> {
    fn new(table: &'a str, name: &'a str, definition: &'a str) -> Self {
        Self {
            table,
            name,
            definition,
        }
    }
}
