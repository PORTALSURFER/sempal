use std::path::{Path, PathBuf};

use super::SourceDbError;

/// Translate rusqlite errors into friendlier SourceDbError variants.
pub(super) fn map_sql_error(err: rusqlite::Error) -> SourceDbError {
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

/// Normalize a relative path for stable database storage.
pub fn normalize_relative_path(path: &Path) -> Result<String, SourceDbError> {
    if path.is_absolute() {
        return Err(SourceDbError::PathMustBeRelative(path.to_path_buf()));
    }
    let cleaned = PathBuf::from_iter(path.components());
    Ok(cleaned.to_string_lossy().replace('\\', "/"))
}

pub(super) fn create_parent_if_needed(path: &Path) -> Result<(), SourceDbError> {
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
