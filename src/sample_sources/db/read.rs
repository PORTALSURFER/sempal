use std::path::PathBuf;

use super::{SourceDatabase, SourceDbError, WavEntry};
use super::util::map_sql_error;

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
}
