//! Incremental schema migrations for older source database files.

use std::collections::HashSet;

use rusqlite::Connection;

use super::super::SourceDbError;
use super::super::util::map_sql_error;

mod analysis_jobs;
mod aspect_descriptors;
mod columns;
mod invalid_paths;
mod tag_catalog;

use self::analysis_jobs::{
    ensure_analysis_jobs_optional_columns, ensure_source_readiness_schema,
    retire_legacy_analysis_runtime_state,
};
use self::aspect_descriptors::ensure_aspect_descriptor_tables;
use self::columns::{
    ensure_feature_metric_columns, ensure_file_ops_journal_optional_columns,
    ensure_pending_rename_optional_columns, ensure_samples_optional_columns,
    ensure_wav_files_optional_columns,
};
pub(super) use self::invalid_paths::remove_invalid_relative_paths;
use self::tag_catalog::{backfill_tag_catalog, ensure_tag_catalog_schema};

/// Apply additive column migrations needed by older source databases.
pub(super) fn apply_optional_migrations(connection: &Connection) -> Result<(), SourceDbError> {
    apply_structural_migrations(connection)?;
    retire_legacy_analysis_runtime_state(connection)?;
    migrate_canonical_file_identities(connection)?;
    backfill_tag_catalog(connection)?;
    Ok(())
}

/// Apply low-cost additive repairs that must not be skipped for current stamps.
pub(super) fn apply_current_stamp_repairs(connection: &Connection) -> Result<(), SourceDbError> {
    apply_structural_migrations(connection)
}

fn apply_structural_migrations(connection: &Connection) -> Result<(), SourceDbError> {
    ensure_wav_files_optional_columns(connection)?;
    ensure_source_index_schema(connection)?;
    ensure_pending_rename_optional_columns(connection)?;
    ensure_file_ops_journal_optional_columns(connection)?;
    ensure_analysis_jobs_optional_columns(connection)?;
    ensure_source_readiness_schema(connection)?;
    ensure_samples_optional_columns(connection)?;
    ensure_feature_metric_columns(connection, "features")?;
    ensure_feature_metric_columns(connection, "analysis_cache_features")?;
    ensure_tag_catalog_schema(connection)?;
    ensure_collection_membership_schema(connection)?;
    ensure_pending_rename_destination_schema(connection)?;
    ensure_aspect_descriptor_tables(connection)?;
    Ok(())
}

fn ensure_source_index_schema(connection: &Connection) -> Result<(), SourceDbError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS source_index_entries (
                path TEXT PRIMARY KEY,
                path_encoding INTEGER NOT NULL DEFAULT 0,
                classification TEXT NOT NULL CHECK(classification IN (
                    'unsupported_audio',
                    'unsupported_non_audio',
                    'inaccessible',
                    'practically_unsupported_audio'
                )),
                file_size INTEGER,
                modified_ns INTEGER,
                file_identity TEXT,
                diagnostic TEXT,
                format_policy_version INTEGER NOT NULL
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS idx_source_index_entries_classification_path
                ON source_index_entries(classification, path);",
        )
        .map_err(map_sql_error)?;
    if !table_columns(connection, "source_index_entries")?.contains("path_encoding") {
        connection
            .execute(
                "ALTER TABLE source_index_entries
                 ADD COLUMN path_encoding INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(map_sql_error)?;
    }
    Ok(())
}

fn ensure_pending_rename_destination_schema(connection: &Connection) -> Result<(), SourceDbError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS pending_wav_rename_destinations (
                path TEXT PRIMARY KEY,
                scan_generation INTEGER NOT NULL,
                retained_hash TEXT
            );",
        )
        .map_err(map_sql_error)?;
    let columns = table_columns(connection, "pending_wav_rename_destinations")?;
    if !columns.contains("retained_hash") {
        connection
            .execute(
                "ALTER TABLE pending_wav_rename_destinations
                 ADD COLUMN retained_hash TEXT",
                [],
            )
            .map_err(map_sql_error)?;
    }
    Ok(())
}

fn ensure_collection_membership_schema(connection: &Connection) -> Result<(), SourceDbError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS wav_file_collections (
                path TEXT NOT NULL,
                collection INTEGER NOT NULL,
                PRIMARY KEY (path, collection),
                FOREIGN KEY(path) REFERENCES wav_files(path) ON DELETE CASCADE
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS idx_wav_file_collections_collection
                ON wav_file_collections(collection);
            INSERT OR IGNORE INTO wav_file_collections (path, collection)
            SELECT path, collection
            FROM wav_files
            WHERE collection IS NOT NULL;",
        )
        .map_err(map_sql_error)?;
    Ok(())
}

fn migrate_canonical_file_identities(connection: &Connection) -> Result<(), SourceDbError> {
    let mut migrated_identities = 0usize;
    for table in ["wav_files", "pending_wav_renames"] {
        if !table_columns(connection, table)?.contains("file_identity") {
            continue;
        }
        let sql = format!(
            "UPDATE {table}
             SET file_identity = CASE
                 WHEN file_identity LIKE 'unix-v2:%'
                     THEN 'unix:' || substr(file_identity, length('unix-v2:') + 1)
                 WHEN file_identity LIKE 'windows-v2:%'
                     THEN 'windows:' || substr(file_identity, length('windows-v2:') + 1)
                 WHEN (
                     file_identity LIKE 'unix:%'
                     OR file_identity LIKE 'windows:%'
                 ) AND (
                     length(file_identity) - length(replace(file_identity, ':', ''))
                 ) = 3
                     THEN file_identity
                 ELSE NULL
             END
             WHERE file_identity IS NOT NULL
               AND (
                   file_identity LIKE 'unix-v2:%'
                   OR file_identity LIKE 'windows-v2:%'
                   OR NOT (
                       (file_identity LIKE 'unix:%' OR file_identity LIKE 'windows:%')
                       AND (
                           length(file_identity) - length(replace(file_identity, ':', ''))
                       ) = 3
                   )
               )"
        );
        migrated_identities = migrated_identities
            .saturating_add(connection.execute(&sql, []).map_err(map_sql_error)?);
    }
    if migrated_identities == 0 {
        return Ok(());
    }
    // Readiness rows are derived from the manifest. Rebuilding them is both
    // simpler and safer than rewriting identity-bearing keys in place: mixed
    // experimental/canonical databases may already contain both spellings,
    // making an UPDATE collide with their uniqueness constraints. Existing
    // durable analysis artifacts are deliberately preserved and will be
    // rediscovered under the canonical identities.
    connection
        .execute_batch(
            "DELETE FROM analysis_jobs WHERE readiness_managed = 1;
             DELETE FROM source_readiness_targets;
             DELETE FROM source_readiness_artifacts;
             DELETE FROM source_readiness_sources;
             DELETE FROM metadata
             WHERE key = 'readiness_target_fingerprint_v1';",
        )
        .map_err(map_sql_error)?;
    Ok(())
}

pub(crate) fn table_columns(
    connection: &Connection,
    table_name: &str,
) -> Result<HashSet<String>, SourceDbError> {
    let pragma = format!("PRAGMA table_info({table_name})");
    let mut stmt = connection.prepare(&pragma).map_err(map_sql_error)?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(map_sql_error)?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(map_sql_error)?;
    Ok(columns)
}

#[cfg(test)]
mod tests;
