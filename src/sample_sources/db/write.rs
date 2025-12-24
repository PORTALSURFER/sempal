use std::path::{Path, PathBuf};

use rusqlite::params;

use super::util::{map_sql_error, normalize_relative_path};
use super::{SampleTag, SourceDatabase, SourceDbError, SourceWriteBatch};

impl SourceDatabase {
    /// Upsert a wav file row using the path relative to the source root.
    #[allow(dead_code)]
    pub fn upsert_file(
        &self,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(relative_path)?;
        let mut stmt = self
            .connection
            .prepare_cached(
                "INSERT INTO wav_files (path, file_size, modified_ns, tag, missing)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(path) DO UPDATE SET file_size = excluded.file_size,
                                                modified_ns = excluded.modified_ns,
                                                missing = excluded.missing",
            )
            .map_err(map_sql_error)?;
        stmt.execute(params![
            path,
            file_size as i64,
            modified_ns,
            SampleTag::Neutral.as_i64(),
            0i64
        ])
        .map_err(map_sql_error)?;
        Ok(())
    }

    /// Persist a keep/trash tag for a single wav file by relative path.
    #[allow(dead_code)]
    pub fn set_tag(&self, relative_path: &Path, tag: SampleTag) -> Result<(), SourceDbError> {
        self.set_tags_batch(&[(relative_path.to_path_buf(), tag)])
    }

    /// Persist multiple tag changes in one transaction, coalescing SQLite work.
    pub fn set_tags_batch(&self, updates: &[(PathBuf, SampleTag)]) -> Result<(), SourceDbError> {
        if updates.is_empty() {
            return Ok(());
        }
        let mut batch = self.write_batch()?;
        for (path, tag) in updates {
            batch.set_tag(path, *tag)?;
        }
        batch.commit()
    }

    /// Update the missing flag for a wav file by relative path.
    pub fn set_missing(&self, relative_path: &Path, missing: bool) -> Result<(), SourceDbError> {
        let mut batch = self.write_batch()?;
        batch.set_missing(relative_path, missing)?;
        batch.commit()
    }

    /// Remove a wav file row by relative path.
    pub fn remove_file(&self, relative_path: &Path) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(relative_path)?;
        self.connection
            .execute("DELETE FROM wav_files WHERE path = ?1", params![path])?;
        Ok(())
    }

    /// Start a write batch that wraps related mutations in a single transaction.
    pub fn write_batch(&self) -> Result<SourceWriteBatch<'_>, SourceDbError> {
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(map_sql_error)?;
        Ok(SourceWriteBatch { tx })
    }
}

impl<'conn> SourceWriteBatch<'conn> {
    /// Insert or update a wav row, resetting the tag to neutral on first insert.
    pub fn upsert_file(
        &mut self,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(relative_path)?;
        self.tx
            .prepare_cached(
                "INSERT INTO wav_files (path, file_size, modified_ns, tag, missing)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(path) DO UPDATE SET file_size = excluded.file_size,
                                                modified_ns = excluded.modified_ns,
                                                missing = excluded.missing",
            )
            .map_err(map_sql_error)?
            .execute(params![
                path,
                file_size as i64,
                modified_ns,
                SampleTag::Neutral.as_i64(),
                0i64
            ])
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub fn upsert_file_with_hash(
        &mut self,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
        content_hash: &str,
    ) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(relative_path)?;
        self.tx
            .prepare_cached(
                "INSERT INTO wav_files (path, file_size, modified_ns, content_hash, tag, missing)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(path) DO UPDATE SET file_size = excluded.file_size,
                                                modified_ns = excluded.modified_ns,
                                                content_hash = excluded.content_hash,
                                                missing = excluded.missing",
            )
            .map_err(map_sql_error)?
            .execute(params![
                path,
                file_size as i64,
                modified_ns,
                content_hash,
                SampleTag::Neutral.as_i64(),
                0i64
            ])
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub fn upsert_file_with_hash_and_tag(
        &mut self,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
        content_hash: &str,
        tag: SampleTag,
        missing: bool,
    ) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(relative_path)?;
        let flag = if missing { 1i64 } else { 0i64 };
        self.tx
            .prepare_cached(
                "INSERT INTO wav_files (path, file_size, modified_ns, content_hash, tag, missing)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(path) DO UPDATE SET file_size = excluded.file_size,
                                                modified_ns = excluded.modified_ns,
                                                content_hash = excluded.content_hash,
                                                tag = excluded.tag,
                                                missing = excluded.missing",
            )
            .map_err(map_sql_error)?
            .execute(params![
                path,
                file_size as i64,
                modified_ns,
                content_hash,
                tag.as_i64(),
                flag
            ])
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Update the tag for a wav row within the batch.
    pub fn set_tag(&mut self, relative_path: &Path, tag: SampleTag) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(relative_path)?;
        self.tx
            .prepare_cached("UPDATE wav_files SET tag = ?1 WHERE path = ?2")
            .map_err(map_sql_error)?
            .execute(params![tag.as_i64(), path])
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Update the missing flag for a wav row within the batch.
    pub fn set_missing(
        &mut self,
        relative_path: &Path,
        missing: bool,
    ) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(relative_path)?;
        let flag = if missing { 1i64 } else { 0i64 };
        self.tx
            .prepare_cached("UPDATE wav_files SET missing = ?1 WHERE path = ?2")
            .map_err(map_sql_error)?
            .execute(params![flag, path])
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Remove a wav row within the batch.
    pub fn remove_file(&mut self, relative_path: &Path) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(relative_path)?;
        self.tx
            .prepare_cached("DELETE FROM wav_files WHERE path = ?1")
            .map_err(map_sql_error)?
            .execute(params![path])
            .map_err(map_sql_error)?;
        Ok(())
    }

    /// Commit all batched operations atomically.
    pub fn commit(self) -> Result<(), SourceDbError> {
        self.tx.commit().map_err(map_sql_error)?;
        Ok(())
    }
}
