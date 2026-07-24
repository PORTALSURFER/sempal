use std::path::PathBuf;

use rusqlite::{OptionalExtension, Params};

use super::super::super::util::{
    EXACT_SUBTREE_PATH_PREDICATE, exact_subtree_path_bounds, map_sql_error,
};
use super::super::super::{Rating, SourceDatabase, SourceDbError, SourceManifestEntry, WavEntry};
use super::super::decode::{
    decode_path_row, decode_wav_entry_row, wav_file_has_column, wav_file_select_columns,
    wav_file_supported_audio_filter,
};

fn collect_wav_entries(
    db: &SourceDatabase,
    sql: &str,
    params: impl Params,
    context: &str,
) -> Result<Vec<WavEntry>, SourceDbError> {
    let mut stmt = db.connection.prepare(sql).map_err(map_sql_error)?;
    let rows = stmt
        .query_map(params, |row| decode_wav_entry_row(row, context))
        .map_err(map_sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sql_error)?;
    Ok(rows.into_iter().flatten().collect())
}

fn collect_paths(
    db: &SourceDatabase,
    sql: &str,
    params: impl Params,
    context: &str,
) -> Result<Vec<PathBuf>, SourceDbError> {
    let mut stmt = db.connection.prepare(sql).map_err(map_sql_error)?;
    let rows = stmt
        .query_map(params, |row| decode_path_row(row, context))
        .map_err(map_sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sql_error)?;
    Ok(rows.into_iter().flatten().collect())
}

fn count_rows(db: &SourceDatabase, extra_predicate: &str) -> Result<usize, SourceDbError> {
    let filter = wav_file_supported_audio_filter(db)?;
    let sql = format!("SELECT COUNT(*) FROM wav_files WHERE {filter}{extra_predicate}");
    let count: i64 = db
        .connection
        .query_row(&sql, [], |row| row.get(0))
        .map_err(map_sql_error)?;
    Ok(count.max(0) as usize)
}

impl SourceDatabase {
    /// Fetch the committed live source manifest and revision from one SQLite read snapshot.
    pub fn manifest_snapshot_with_revision(
        &self,
    ) -> Result<(u64, Vec<SourceManifestEntry>), SourceDbError> {
        let filter = wav_file_supported_audio_filter(self)?;
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(map_sql_error)?;
        let revision = transaction
            .query_row(
                "SELECT value FROM metadata WHERE key = 'revision'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_sql_error)?
            .map(|raw| raw.parse::<u64>().map_err(|_| SourceDbError::Unexpected))
            .transpose()?
            .unwrap_or_default();
        let sql = format!(
            "SELECT path, file_identity, content_hash, file_size, modified_ns
             FROM wav_files
             WHERE {filter} AND missing = 0
             ORDER BY path ASC"
        );
        let entries = {
            let mut statement = transaction.prepare(&sql).map_err(map_sql_error)?;
            let rows = statement
                .query_map([], |row| {
                    let Some(relative_path) = decode_path_row(
                        row,
                        "Skipping source manifest row with invalid relative path",
                    )?
                    else {
                        return Ok(None);
                    };
                    Ok(Some(SourceManifestEntry {
                        relative_path,
                        file_identity: row.get(1)?,
                        content_hash: row.get(2)?,
                        file_size: row.get::<_, i64>(3)?.max(0) as u64,
                        modified_ns: row.get(4)?,
                    }))
                })
                .map_err(map_sql_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sql_error)?;
            rows.into_iter().flatten().collect()
        };
        transaction.rollback().map_err(map_sql_error)?;
        Ok((revision, entries))
    }

    /// Fetch the committed live source manifest in deterministic path order.
    pub fn list_manifest_entries(&self) -> Result<Vec<SourceManifestEntry>, SourceDbError> {
        let filter = wav_file_supported_audio_filter(self)?;
        let sql = format!(
            "SELECT path, file_identity, content_hash, file_size, modified_ns
             FROM wav_files
             WHERE {filter} AND missing = 0
             ORDER BY path ASC"
        );
        let mut statement = self.connection.prepare(&sql).map_err(map_sql_error)?;
        let rows = statement
            .query_map([], |row| {
                let Some(relative_path) = decode_path_row(
                    row,
                    "Skipping source manifest row with invalid relative path",
                )?
                else {
                    return Ok(None);
                };
                Ok(Some(SourceManifestEntry {
                    relative_path,
                    file_identity: row.get(1)?,
                    content_hash: row.get(2)?,
                    file_size: row.get::<_, i64>(3)?.max(0) as u64,
                    modified_ns: row.get(4)?,
                }))
            })
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        Ok(rows.into_iter().flatten().collect())
    }

    /// Fetch all tracked wav files for this source.
    pub fn list_files(&self) -> Result<Vec<WavEntry>, SourceDbError> {
        let filter = wav_file_supported_audio_filter(self)?;
        let columns = wav_file_select_columns(self)?;
        let sql = format!(
            "SELECT {columns}
             FROM wav_files
             WHERE {filter}
             ORDER BY path ASC"
        );
        collect_wav_entries(
            self,
            &sql,
            [],
            "Skipping wav row with invalid relative path",
        )
    }

    /// Fetch a bounded path-ordered batch that still needs a deep content hash.
    pub fn list_pending_hash_files(&self, limit: usize) -> Result<Vec<WavEntry>, SourceDbError> {
        let filter = wav_file_supported_audio_filter(self)?;
        let columns = wav_file_select_columns(self)?;
        let sql = format!(
            "SELECT {columns}
             FROM wav_files
             WHERE {filter}
               AND missing = 0
               AND content_hash IS NULL
             ORDER BY path ASC
             LIMIT ?1"
        );
        collect_wav_entries(
            self,
            &sql,
            [i64::try_from(limit).unwrap_or(i64::MAX)],
            "Skipping pending hash row with invalid relative path",
        )
    }

    /// Fetch tracked wav files whose paths are at or below the provided relative path.
    pub fn list_files_under_path(
        &self,
        path: &std::path::Path,
    ) -> Result<Vec<WavEntry>, SourceDbError> {
        let path_str = super::super::super::normalize_relative_path(path)?;
        let (lower_bound, upper_bound) = exact_subtree_path_bounds(&path_str);
        let filter = wav_file_supported_audio_filter(self)?;
        let columns = wav_file_select_columns(self)?;
        let sql = format!(
            "SELECT {columns}
             FROM wav_files
             WHERE {filter}
               AND {EXACT_SUBTREE_PATH_PREDICATE}
             ORDER BY path ASC"
        );
        collect_wav_entries(
            self,
            &sql,
            rusqlite::params![path_str, lower_bound, upper_bound],
            "Skipping wav row with invalid relative path during prefix lookup",
        )
    }

    /// Fetch one tracked wav entry by relative path.
    pub fn entry_for_path(
        &self,
        path: &std::path::Path,
    ) -> Result<Option<WavEntry>, SourceDbError> {
        if !crate::sample_sources::is_supported_audio(path) {
            return Ok(None);
        }
        let path_str = super::super::super::normalize_relative_path(path)?;
        let filter = wav_file_supported_audio_filter(self)?;
        let columns = wav_file_select_columns(self)?;
        let sql = format!(
            "SELECT {columns}
             FROM wav_files
             WHERE {filter} AND path = ?1"
        );
        let mut rows = collect_wav_entries(
            self,
            &sql,
            rusqlite::params![path_str],
            "Skipping wav row with invalid relative path during single-path lookup",
        )?;
        Ok(rows.pop())
    }

    /// Fetch tracked wav files filtered by tag.
    pub fn list_files_by_tag(&self, tag: Rating) -> Result<Vec<WavEntry>, SourceDbError> {
        if !wav_file_has_column(self, "tag")? {
            return if tag == Rating::NEUTRAL {
                self.list_files()
            } else {
                Ok(Vec::new())
            };
        }
        let filter = wav_file_supported_audio_filter(self)?;
        let columns = wav_file_select_columns(self)?;
        let sql = format!(
            "SELECT {columns}
             FROM wav_files
             WHERE {filter} AND tag = ?1
             ORDER BY path ASC"
        );
        collect_wav_entries(
            self,
            &sql,
            [tag.as_i64()],
            "Skipping tagged wav row with invalid relative path",
        )
    }

    /// Fetch relative paths that are currently marked missing.
    pub fn list_missing_paths(&self) -> Result<Vec<PathBuf>, SourceDbError> {
        if !wav_file_has_column(self, "missing")? {
            return Ok(Vec::new());
        }
        collect_paths(
            self,
            "SELECT path FROM wav_files WHERE missing != 0",
            [],
            "Skipping missing wav row with invalid relative path",
        )
    }

    /// Fetch tracked paths that currently have the provided content hash.
    ///
    /// This is used by scan rename reconciliation to resolve candidates without
    /// building a full in-memory hash index for all rows.
    pub fn list_paths_with_content_hash(&self, hash: &str) -> Result<Vec<PathBuf>, SourceDbError> {
        if !wav_file_has_column(self, "content_hash")? {
            return Ok(Vec::new());
        }
        let filter = wav_file_supported_audio_filter(self)?;
        let sql = format!(
            "SELECT path
             FROM wav_files
             WHERE {filter}
               AND content_hash = ?1"
        );
        collect_paths(
            self,
            &sql,
            rusqlite::params![hash],
            "Skipping wav row with invalid relative path during hash lookup",
        )
    }

    /// Fetch tracked paths that currently match file-size and modified timestamp.
    ///
    /// Quick scans use this to reconcile rename candidates for files whose full
    /// content hash is deferred to a later deep-hash pass.
    pub fn list_paths_with_file_facts(
        &self,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<Vec<PathBuf>, SourceDbError> {
        let filter = wav_file_supported_audio_filter(self)?;
        let sql = format!(
            "SELECT path
             FROM wav_files
             WHERE {filter}
               AND file_size = ?1
               AND modified_ns = ?2"
        );
        collect_paths(
            self,
            &sql,
            rusqlite::params![file_size as i64, modified_ns],
            "Skipping wav row with invalid relative path during facts lookup",
        )
    }

    /// Count all tracked wav files for this source.
    pub fn count_files(&self) -> Result<usize, SourceDbError> {
        count_rows(self, "")
    }

    /// Count all tracked wav files that are not marked missing.
    pub fn count_present_files(&self) -> Result<usize, SourceDbError> {
        let extra_predicate = if wav_file_has_column(self, "missing")? {
            " AND missing = 0"
        } else {
            ""
        };
        count_rows(self, extra_predicate)
    }

    /// Fetch a page of tracked wav files ordered by path.
    pub fn list_files_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<WavEntry>, SourceDbError> {
        let filter = wav_file_supported_audio_filter(self)?;
        let columns = wav_file_select_columns(self)?;
        let sql = format!(
            "SELECT {columns}
             FROM wav_files
             WHERE {filter}
             ORDER BY path ASC
             LIMIT ?1 OFFSET ?2"
        );
        collect_wav_entries(
            self,
            &sql,
            rusqlite::params![limit as i64, offset as i64],
            "Skipping wav row page with invalid relative path",
        )
    }
}
