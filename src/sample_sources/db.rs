use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Hidden filename used for per-source databases.
pub const DB_FILE_NAME: &str = ".sempal_samples.db";

/// Details about a wav file stored in a source database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WavEntry {
    pub relative_path: PathBuf,
    pub file_size: u64,
    pub modified_ns: i64,
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
    pub fn upsert_file(
        &self,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<(), SourceDbError> {
        let path = normalize_relative_path(relative_path)?;
        self.connection.execute(
            "INSERT INTO wav_files (path, file_size, modified_ns)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET file_size = excluded.file_size,
                                            modified_ns = excluded.modified_ns",
            params![path, file_size as i64, modified_ns],
        )?;
        Ok(())
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
        let mut stmt = self
            .connection
            .prepare("SELECT path, file_size, modified_ns FROM wav_files ORDER BY path ASC")?;
        let rows = stmt
            .query_map([], |row| {
                let path: String = row.get(0)?;
                Ok(WavEntry {
                    relative_path: PathBuf::from(path),
                    file_size: row.get::<_, i64>(1)? as u64,
                    modified_ns: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn apply_pragmas(&self) -> Result<(), SourceDbError> {
        self.connection
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(())
    }

    fn apply_schema(&self) -> Result<(), SourceDbError> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
             CREATE TABLE IF NOT EXISTS wav_files (
                path TEXT PRIMARY KEY,
                file_size INTEGER NOT NULL,
                modified_ns INTEGER NOT NULL
            );",
        )?;
        Ok(())
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
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|source| SourceDbError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    Ok(())
}
