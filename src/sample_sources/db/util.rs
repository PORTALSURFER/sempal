use std::path::{Component, Path, PathBuf};

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
///
/// Rejects absolute paths, parent traversal, root prefixes, and empty paths.
pub fn normalize_relative_path(path: &Path) -> Result<String, SourceDbError> {
    let cleaned = sanitize_relative_path(path)?;
    Ok(cleaned.to_string_lossy().replace('\\', "/"))
}

/// Parse and validate a stored relative path from the database.
///
/// Returns a normalized `PathBuf` without `.` components.
pub(super) fn parse_relative_path_from_db(path: &str) -> Result<PathBuf, SourceDbError> {
    sanitize_relative_path(Path::new(path))
}

/// Validate a relative path and normalize away `.` components.
fn sanitize_relative_path(path: &Path) -> Result<PathBuf, SourceDbError> {
    if path.is_absolute() {
        return Err(SourceDbError::PathMustBeRelative(path.to_path_buf()));
    }
    let mut cleaned = PathBuf::new();
    let mut saw_component = false;
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => {
                cleaned.push(part);
                saw_component = true;
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(SourceDbError::InvalidRelativePath(path.to_path_buf()));
            }
        }
    }
    if !saw_component {
        return Err(SourceDbError::InvalidRelativePath(path.to_path_buf()));
    }
    Ok(cleaned)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_relative_path_rejects_parent_dir() {
        let err = normalize_relative_path(Path::new("../escape.wav")).unwrap_err();
        assert!(matches!(err, SourceDbError::InvalidRelativePath(_)));
    }

    #[test]
    fn normalize_relative_path_rejects_empty_or_curdir_only() {
        let err = normalize_relative_path(Path::new(".")).unwrap_err();
        assert!(matches!(err, SourceDbError::InvalidRelativePath(_)));
        let err = normalize_relative_path(Path::new("")).unwrap_err();
        assert!(matches!(err, SourceDbError::InvalidRelativePath(_)));
    }

    #[test]
    fn normalize_relative_path_skips_curdir_components() {
        let normalized = normalize_relative_path(Path::new("folder/./file.wav")).unwrap();
        assert_eq!(normalized, "folder/file.wav");
    }
}
