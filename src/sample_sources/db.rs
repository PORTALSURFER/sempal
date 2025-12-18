use std::path::{Path, PathBuf};

use rusqlite::{Connection, Transaction, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Hidden filename used for per-source databases.
pub const DB_FILE_NAME: &str = ".sempal_samples.db";

/// Tag applied to a wav file to mark keep/trash decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SampleTag {
    Neutral,
    Keep,
    Trash,
}

impl SampleTag {
    /// Convert the tag to a SQLite-friendly integer.
    pub fn as_i64(self) -> i64 {
        match self {
            SampleTag::Neutral => 0,
            SampleTag::Keep => 1,
            SampleTag::Trash => 2,
        }
    }

    /// Parse an integer column value into a tag.
    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => SampleTag::Keep,
            2 => SampleTag::Trash,
            _ => SampleTag::Neutral,
        }
    }
}

/// Details about a wav file stored in a source database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WavEntry {
    pub relative_path: PathBuf,
    pub file_size: u64,
    pub modified_ns: i64,
    pub content_hash: Option<String>,
    pub tag: SampleTag,
    pub missing: bool,
}

/// Errors returned when managing a source database.
#[derive(Debug, Error)]
pub enum SourceDbError {
    #[error("Source folder is not a directory: {0}")]
    InvalidRoot(PathBuf),
    #[error("Database query failed: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("Could not write to {path}: {source}")]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Path must be relative to the source root: {0}")]
    PathMustBeRelative(PathBuf),
    #[error("Database is busy, please retry")]
    Busy,
    #[error("SQLite returned an unexpected result")]
    Unexpected,
}

/// SQLite wrapper that stores wav metadata for a single source folder.
pub struct SourceDatabase {
    connection: Connection,
    root: PathBuf,
}

impl SourceDatabase {
    /// Open (or create) the database that lives inside the source folder.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, SourceDbError> {
        let root = root.as_ref();
        if !root.is_dir() {
            return Err(SourceDbError::InvalidRoot(root.to_path_buf()));
        }

        let db_path = root.join(DB_FILE_NAME);
        create_parent_if_needed(&db_path)?;
        let connection = Connection::open(&db_path)?;
        let db = Self {
            connection,
            root: root.to_path_buf(),
        };
        db.apply_pragmas()?;
        db.apply_schema()?;
        Ok(db)
    }

    /// Return the path to the root folder backing this database.
    pub fn root(&self) -> &Path {
        &self.root
    }

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
                    tag: SampleTag::from_i64(row.get(4)?),
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

    /// Start a write batch that wraps related mutations in a single transaction.
    pub fn write_batch(&self) -> Result<SourceWriteBatch<'_>, SourceDbError> {
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(map_sql_error)?;
        Ok(SourceWriteBatch { tx })
    }

    fn apply_pragmas(&self) -> Result<(), SourceDbError> {
        self.connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;
             PRAGMA temp_store=MEMORY;
             PRAGMA cache_size=-32000;
             PRAGMA mmap_size=134217728;",
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    fn apply_schema(&self) -> Result<(), SourceDbError> {
        self.connection
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
        ensure_optional_columns(&self.connection)?;
        self.connection
            .execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_wav_files_missing
                 ON wav_files(path) WHERE missing != 0;",
            )
            .map_err(map_sql_error)?;
        Ok(())
    }
}

/// Groups multiple database writes into one transaction using cached statements.
pub struct SourceWriteBatch<'conn> {
    tx: Transaction<'conn>,
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

/// Translate rusqlite errors into friendlier SourceDbError variants.
fn map_sql_error(err: rusqlite::Error) -> SourceDbError {
    match err {
        rusqlite::Error::SqliteFailure(sql_err, _)
            if sql_err.extended_code == rusqlite::ffi::SQLITE_BUSY =>
        {
            SourceDbError::Busy
        }
        rusqlite::Error::InvalidQuery
        | rusqlite::Error::InvalidParameterName(_)
        | rusqlite::Error::MultipleStatement => SourceDbError::Unexpected,
        other => SourceDbError::Sql(other),
    }
}

fn normalize_relative_path(path: &Path) -> Result<String, SourceDbError> {
    if path.is_absolute() {
        return Err(SourceDbError::PathMustBeRelative(path.to_path_buf()));
    }
    let cleaned = PathBuf::from_iter(path.components());
    Ok(cleaned.to_string_lossy().replace('\\', "/"))
}

fn create_parent_if_needed(path: &Path) -> Result<(), SourceDbError> {
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(|source| SourceDbError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::OptionalExtension;
    use tempfile::tempdir;

    #[test]
    fn tags_default_and_persist() {
        let dir = tempdir().unwrap();
        let db = SourceDatabase::open(dir.path()).unwrap();
        db.upsert_file(Path::new("one.wav"), 10, 5).unwrap();

        let first = db.list_files().unwrap();
        assert_eq!(first[0].tag, SampleTag::Neutral);
        assert!(!first[0].missing);

        db.set_tag(Path::new("one.wav"), SampleTag::Keep).unwrap();
        let second = db.list_files().unwrap();
        assert_eq!(second[0].tag, SampleTag::Keep);
        assert!(!second[0].missing);

        db.upsert_file(Path::new("one.wav"), 12, 6).unwrap();
        let third = db.list_files().unwrap();
        assert_eq!(third[0].tag, SampleTag::Keep);
        assert!(!third[0].missing);

        let reopened = SourceDatabase::open(dir.path()).unwrap();
        let fourth = reopened.list_files().unwrap();
        assert_eq!(fourth[0].tag, SampleTag::Keep);
        assert!(!fourth[0].missing);
    }

    #[test]
    fn batch_tag_updates_coalesce_to_latest_value() {
        let dir = tempdir().unwrap();
        let db = SourceDatabase::open(dir.path()).unwrap();
        db.upsert_file(Path::new("one.wav"), 10, 5).unwrap();

        db.set_tags_batch(&[
            (PathBuf::from("one.wav"), SampleTag::Keep),
            (PathBuf::from("one.wav"), SampleTag::Trash),
        ])
        .unwrap();

        let rows = db.list_files().unwrap();
        assert_eq!(rows[0].tag, SampleTag::Trash);
    }

    #[test]
    fn absolute_paths_are_rejected() {
        let dir = tempdir().unwrap();
        let db = SourceDatabase::open(dir.path()).unwrap();
        let absolute = std::env::current_dir().unwrap().join("absolute.wav");
        let err = db.upsert_file(&absolute, 1, 1).unwrap_err();
        assert!(matches!(err, SourceDbError::PathMustBeRelative(_)));
    }

    #[test]
    fn missing_columns_are_added_on_open() {
        let dir = tempdir().unwrap();
        let db_file = dir.path().join(DB_FILE_NAME);
        {
            let conn = Connection::open(&db_file).unwrap();
            conn.execute(
                "CREATE TABLE wav_files (
                    path TEXT PRIMARY KEY,
                    file_size INTEGER NOT NULL,
                    modified_ns INTEGER NOT NULL
                )",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO wav_files (path, file_size, modified_ns) VALUES ('one.wav', 10, 5)",
                [],
            )
            .unwrap();
        }
        let db = SourceDatabase::open(dir.path()).unwrap();
        let rows = db.list_files().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tag, SampleTag::Neutral);
        assert!(!rows[0].missing);
    }

    #[test]
    fn missing_flag_round_trips() {
        let dir = tempdir().unwrap();
        let db = SourceDatabase::open(dir.path()).unwrap();
        db.upsert_file(Path::new("one.wav"), 10, 5).unwrap();
        db.set_missing(Path::new("one.wav"), true).unwrap();
        let rows = db.list_files().unwrap();
        assert!(rows[0].missing);
        db.set_missing(Path::new("one.wav"), false).unwrap();
        let rows = db.list_files().unwrap();
        assert!(!rows[0].missing);
    }

    #[test]
    fn applies_workload_pragmas_and_indices() {
        let dir = tempdir().unwrap();
        let _db = SourceDatabase::open(dir.path()).unwrap();
        let conn = Connection::open(dir.path().join(DB_FILE_NAME)).unwrap();

        let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap();
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");

        let synchronous: i64 = conn.query_row("PRAGMA synchronous", [], |row| row.get(0)).unwrap();
        assert_eq!(synchronous, 2, "expected PRAGMA synchronous=NORMAL (2)");

        let busy_timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(busy_timeout, 5000);

        let idx: Option<String> = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='index' AND name='idx_wav_files_missing'",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert_eq!(idx.as_deref(), Some("idx_wav_files_missing"));
    }
}
