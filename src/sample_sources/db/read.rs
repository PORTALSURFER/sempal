use std::path::{Path, PathBuf};

use super::util::map_sql_error;
use super::{SourceDatabase, SourceDbError, WavEntry};
use rusqlite::OptionalExtension;

impl SourceDatabase {
    /// Fetch all tracked wav files for this source.
    pub fn list_files(&self) -> Result<Vec<WavEntry>, SourceDbError> {
        let mut stmt = self.connection.prepare(
            "SELECT path, file_size, modified_ns, content_hash, tag, missing FROM wav_files ORDER BY path ASC",
        ).map_err(map_sql_error)?;
        let rows = stmt
            .query_map([], |row| {
                let path: String = row.get(0)?;
                Ok(WavEntry {
                    relative_path: PathBuf::from(path),
                    file_size: row.get::<_, i64>(1)? as u64,
                    modified_ns: row.get(2)?,
                    content_hash: row.get::<_, Option<String>>(3)?,
                    tag: super::SampleTag::from_i64(row.get(4)?),
                    missing: row.get::<_, i64>(5)? != 0,
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
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM wav_files", [], |row| row.get(0))
            .map_err(map_sql_error)?;
        Ok(count.max(0) as usize)
    }

    /// Fetch a page of tracked wav files ordered by path.
    pub fn list_files_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<WavEntry>, SourceDbError> {
        let mut stmt = self
            .connection
            .prepare(
                "SELECT path, file_size, modified_ns, content_hash, tag, missing
                 FROM wav_files
                 ORDER BY path ASC
                 LIMIT ?1 OFFSET ?2",
            )
            .map_err(map_sql_error)?;
        let rows = stmt
            .query_map(
                rusqlite::params![limit as i64, offset as i64],
                |row| {
                    let path: String = row.get(0)?;
                    Ok(WavEntry {
                        relative_path: PathBuf::from(path),
                        file_size: row.get::<_, i64>(1)? as u64,
                        modified_ns: row.get(2)?,
                        content_hash: row.get::<_, Option<String>>(3)?,
                        tag: super::SampleTag::from_i64(row.get(4)?),
                        missing: row.get::<_, i64>(5)? != 0,
                    })
                },
            )
            .map_err(map_sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_error)?;
        Ok(rows)
    }

    /// Find the sorted index for a tracked wav path.
    pub fn index_for_path(&self, path: &Path) -> Result<Option<usize>, SourceDbError> {
        let path_str = path.to_string_lossy();
        let (offset, exists): (i64, i64) = self
            .connection
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM wav_files WHERE path < ?1) AS offset,
                    EXISTS(SELECT 1 FROM wav_files WHERE path = ?1) AS exists",
                rusqlite::params![path_str.as_ref()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(map_sql_error)?;
        if exists == 0 {
            return Ok(None);
        }
        Ok(Some(offset.max(0) as usize))
    }

    /// Fetch the tag for a specific wav path.
    pub fn tag_for_path(&self, path: &Path) -> Result<Option<super::SampleTag>, SourceDbError> {
        let path_str = path.to_string_lossy();
        let value: Option<i64> = self
            .connection
            .query_row(
                "SELECT tag FROM wav_files WHERE path = ?1",
                rusqlite::params![path_str.as_ref()],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sql_error)?;
        Ok(value.map(super::SampleTag::from_i64))
    }
}
