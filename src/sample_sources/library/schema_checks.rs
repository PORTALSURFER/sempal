use rusqlite::OptionalExtension;
use std::collections::HashSet;

use super::{LibraryDatabase, LibraryError, map_sql_error};

impl LibraryDatabase {
    pub(super) fn table_exists(&self, table: &str) -> Result<bool, LibraryError> {
        let mut stmt = self
            .connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name=?1")
            .map_err(map_sql_error)?;
        let exists: Option<String> = stmt
            .query_row([table], |row| row.get(0))
            .optional()
            .map_err(map_sql_error)?;
        Ok(exists.is_some())
    }

    pub(super) fn table_columns(&self, table: &str) -> Result<HashSet<String>, LibraryError> {
        let mut stmt = self
            .connection
            .prepare(&format!("PRAGMA table_info({})", table))
            .map_err(map_sql_error)?;
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(map_sql_error)?
            .filter_map(Result::ok)
            .collect();
        Ok(columns)
    }

    pub(super) fn get_metadata(&self, key: &str) -> Result<Option<String>, LibraryError> {
        self.connection
            .query_row("SELECT value FROM metadata WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(map_sql_error)
    }

    pub(super) fn set_metadata(&self, key: &str, value: &str) -> Result<(), LibraryError> {
        self.connection
            .execute(
                "INSERT INTO metadata (key, value)
                 VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [key, value],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }
}
