use std::path::{Path, PathBuf};

use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod read;
mod schema;
mod util;
mod write;

/// Hidden filename used for per-source databases.
pub const DB_FILE_NAME: &str = ".sempal_samples.db";
pub const META_LAST_SCAN_COMPLETED_AT: &str = "last_scan_completed_at";
pub const META_LAST_SIMILARITY_PREP_SCAN_AT: &str = "last_similarity_prep_scan_at";

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

/// Groups multiple database writes into one transaction using cached statements.
pub struct SourceWriteBatch<'conn> {
    tx: Transaction<'conn>,
}

impl SourceDatabase {
    /// Open (or create) the database that lives inside the source folder.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, SourceDbError> {
        let root = root.as_ref();
        if !root.is_dir() {
            return Err(SourceDbError::InvalidRoot(root.to_path_buf()));
        }

        let db_path = root.join(DB_FILE_NAME);
        util::create_parent_if_needed(&db_path)?;
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
            .map_err(util::map_sql_error)?;
        if let Err(err) = crate::sqlite_ext::try_load_optional_extension(&self.connection) {
            tracing::debug!("SQLite extension not loaded: {err}");
        }
        Ok(())
    }

    fn apply_schema(&self) -> Result<(), SourceDbError> {
        schema::apply_schema(&self.connection)
    }
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

        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");

        let synchronous: i64 = conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .unwrap();
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
