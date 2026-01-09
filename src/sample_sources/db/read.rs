use std::path::{Path, PathBuf};

use super::util::map_sql_error;
use super::{SourceDatabase, SourceDbError, WavEntry};
use rusqlite::OptionalExtension;

impl SourceDatabase {
    /// Fetch all tracked wav files for this source.
    pub fn list_files(&self) -> Result<Vec<WavEntry>, SourceDbError> {
        let filter = crate::sample_sources::supported_audio_where_clause();
        let sql = format!(
            "SELECT path, file_size, modified_ns, content_hash, tag, looped, missing, last_played_at
             FROM wav_files
             WHERE {filter}
             ORDER BY path ASC"
        );
        let mut stmt = self.connection.prepare(&sql).map_err(map_sql_error)?;
        let rows = stmt
            .query_map([], |row| {
                let path: String = row.get(0)?;
                Ok(WavEntry {
                    relative_path: PathBuf::from(path),
                    file_size: row.get::<_, i64>(1)? as u64,
                    modified_ns: row.get(2)?,
                    content_hash: row.get::<_, Option<String>>(3)?,
                    tag: super::Rating::from_i64(row.get(4)?),
                    looped: row.get::<_, i64>(5)? != 0,
                    missing: row.get::<_, i64>(6)? != 0,
                    last_played_at: row.get(7)?,
                })
            })
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        Ok(rows)
    }

    /// Fetch tracked wav files filtered by tag.
    pub fn list_files_by_tag(&self, tag: super::Rating) -> Result<Vec<WavEntry>, SourceDbError> {
        let filter = crate::sample_sources::supported_audio_where_clause();
        let sql = format!(
            "SELECT path, file_size, modified_ns, content_hash, tag, looped, missing, last_played_at
             FROM wav_files
             WHERE {filter} AND tag = ?1
             ORDER BY path ASC"
        );
        let mut stmt = self.connection.prepare(&sql).map_err(map_sql_error)?;
        let rows = stmt
            .query_map([tag.as_i64()], |row| {
                let path: String = row.get(0)?;
                Ok(WavEntry {
                    relative_path: PathBuf::from(path),
                    file_size: row.get::<_, i64>(1)? as u64,
                    modified_ns: row.get(2)?,
                    content_hash: row.get::<_, Option<String>>(3)?,
                    tag: super::Rating::from_i64(row.get(4)?),
                    looped: row.get::<_, i64>(5)? != 0,
                    missing: row.get::<_, i64>(6)? != 0,
                    last_played_at: row.get(7)?,
                })
            })
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        Ok(rows)
    }

    /// Fetch relative paths that are currently marked missing.
    pub fn list_missing_paths(&self) -> Result<Vec<PathBuf>, SourceDbError> {
        let mut stmt = self
            .connection
            .prepare("SELECT path FROM wav_files WHERE missing != 0")
            .map_err(map_sql_error)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        Ok(rows.into_iter().map(PathBuf::from).collect())
    }

    /// Count all tracked wav files for this source.
    pub fn count_files(&self) -> Result<usize, SourceDbError> {
        let filter = crate::sample_sources::supported_audio_where_clause();
        let sql = format!("SELECT COUNT(*) FROM wav_files WHERE {filter}");
        let count: i64 = self
            .connection
            .query_row(&sql, [], |row| row.get(0))
            .map_err(map_sql_error)?;
        Ok(count.max(0) as usize)
    }

    /// Count all tracked wav files that are not marked missing.
    pub fn count_present_files(&self) -> Result<usize, SourceDbError> {
        let filter = crate::sample_sources::supported_audio_where_clause();
        let sql = format!("SELECT COUNT(*) FROM wav_files WHERE {filter} AND missing = 0");
        let count: i64 = self
            .connection
            .query_row(&sql, [], |row| row.get(0))
            .map_err(map_sql_error)?;
        Ok(count.max(0) as usize)
    }

    /// Fetch a page of tracked wav files ordered by path.
    pub fn list_files_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<WavEntry>, SourceDbError> {
        let filter = crate::sample_sources::supported_audio_where_clause();
        let sql = format!(
            "SELECT path, file_size, modified_ns, content_hash, tag, looped, missing, last_played_at
             FROM wav_files
             WHERE {filter}
             ORDER BY path ASC
             LIMIT ?1 OFFSET ?2"
        );
        let mut stmt = self.connection.prepare(&sql).map_err(map_sql_error)?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64, offset as i64], |row| {
                let path: String = row.get(0)?;
                Ok(WavEntry {
                    relative_path: PathBuf::from(path),
                    file_size: row.get::<_, i64>(1)? as u64,
                    modified_ns: row.get(2)?,
                    content_hash: row.get::<_, Option<String>>(3)?,
                    tag: super::Rating::from_i64(row.get(4)?),
                    looped: row.get::<_, i64>(5)? != 0,
                    missing: row.get::<_, i64>(6)? != 0,
                    last_played_at: row.get(7)?,
                })
            })
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        Ok(rows)
    }

    /// Find the sorted index for a tracked wav path.
    pub fn index_for_path(&self, path: &Path) -> Result<Option<usize>, SourceDbError> {
        if !crate::sample_sources::is_supported_audio(path) {
            return Ok(None);
        }
        let path_str = super::normalize_relative_path(path)?;
        let (offset, exists): (i64, i64) = self
            .connection
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM wav_files WHERE path < ?1) AS offset,
                    EXISTS(SELECT 1 FROM wav_files WHERE path = ?1) AS path_exists",
                rusqlite::params![path_str.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(map_sql_error)?;
        if exists == 0 {
            return Ok(None);
        }
        Ok(Some(offset.max(0) as usize))
    }

    /// Fetch the tag for a specific wav path.
    pub fn tag_for_path(&self, path: &Path) -> Result<Option<super::Rating>, SourceDbError> {
        if !crate::sample_sources::is_supported_audio(path) {
            return Ok(None);
        }
        let path_str = super::normalize_relative_path(path)?;
        let value: Option<i64> = self
            .connection
            .query_row(
                "SELECT tag FROM wav_files WHERE path = ?1",
                rusqlite::params![path_str.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sql_error)?;
        Ok(value.map(super::Rating::from_i64))
    }

    /// Fetch the loop marker state for a specific wav path.
    pub fn looped_for_path(&self, path: &Path) -> Result<Option<bool>, SourceDbError> {
        if !crate::sample_sources::is_supported_audio(path) {
            return Ok(None);
        }
        let path_str = super::normalize_relative_path(path)?;
        let value: Option<i64> = self
            .connection
            .query_row(
                "SELECT looped FROM wav_files WHERE path = ?1",
                rusqlite::params![path_str.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sql_error)?;
        Ok(value.map(|flag| flag != 0))
    }

    /// Fetch the last played timestamp for a specific wav path.
    pub fn last_played_at_for_path(&self, path: &Path) -> Result<Option<i64>, SourceDbError> {
        if !crate::sample_sources::is_supported_audio(path) {
            return Ok(None);
        }
        let path_str = super::normalize_relative_path(path)?;
        let value: Option<i64> = self
            .connection
            .query_row(
                "SELECT last_played_at FROM wav_files WHERE path = ?1",
                rusqlite::params![path_str.as_str()],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(map_sql_error)?
            .flatten();
        Ok(value)
    }

    pub fn get_metadata(&self, key: &str) -> Result<Option<String>, SourceDbError> {
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                rusqlite::params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sql_error)?;
        Ok(value)
    }

    pub fn get_revision(&self) -> Result<u64, SourceDbError> {
        let rev_str = self.get_metadata("revision")?;
        match rev_str {
            Some(s) => s.parse::<u64>().map_err(|_| SourceDbError::Unexpected),
            None => Ok(0),
        }
    }
}
