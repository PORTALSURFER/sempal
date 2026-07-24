use std::path::{Component, Path, PathBuf};

use super::SourceDbError;

/// Exact subtree predicate for normalized, persisted source-relative paths.
///
/// The slash in the lower bound is immediately followed by `0` in the upper
/// bound. SQLite's default BINARY collation therefore keeps every descendant
/// of `path/` in the half-open range while excluding sibling names such as
/// `path-other`. The equality arm includes a file or directory row targeted
/// directly. This avoids LIKE's wildcard and ASCII case-folding semantics and
/// remains compatible with the path primary-key indexes.
pub(super) const EXACT_SUBTREE_PATH_PREDICATE: &str =
    "(path = ?1 COLLATE BINARY OR (path >= ?2 COLLATE BINARY AND path < ?3 COLLATE BINARY))";

pub(super) fn exact_subtree_path_bounds(path: &str) -> (String, String) {
    (format!("{path}/"), format!("{path}0"))
}

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
    let value = cleaned
        .to_str()
        .ok_or_else(|| SourceDbError::NonUnicodeRelativePath(cleaned.clone()))?;
    Ok(value.replace('\\', "/"))
}

/// Parse and validate a stored relative path from the database.
///
/// Returns a normalized `PathBuf` without `.` components.
pub(super) fn parse_relative_path_from_db(path: &str) -> Result<PathBuf, SourceDbError> {
    sanitize_relative_path(Path::new(path))
}

/// Validate a relative path and normalize away `.` components.
fn sanitize_relative_path(path: &Path) -> Result<PathBuf, SourceDbError> {
    if has_windows_absolute_prefix(path) {
        return Err(SourceDbError::InvalidRelativePath(path.to_path_buf()));
    }
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

fn has_windows_absolute_prefix(path: &Path) -> bool {
    let Some(value) = path.to_str() else {
        return false;
    };
    let bytes = value.as_bytes();
    value.starts_with('\\')
        || (bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic())
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
    fn normalize_relative_path_rejects_rooted_path() {
        let err = normalize_relative_path(Path::new("/escape.wav")).unwrap_err();
        #[cfg(windows)]
        assert!(matches!(err, SourceDbError::InvalidRelativePath(_)));
        #[cfg(not(windows))]
        assert!(matches!(err, SourceDbError::PathMustBeRelative(_)));
    }

    #[test]
    fn normalize_relative_path_rejects_windows_drive_prefix() {
        let err = normalize_relative_path(Path::new(r"C:\escape.wav")).unwrap_err();
        assert!(matches!(err, SourceDbError::InvalidRelativePath(_)));
        let err = normalize_relative_path(Path::new("C:/escape.wav")).unwrap_err();
        assert!(matches!(err, SourceDbError::InvalidRelativePath(_)));
    }

    #[test]
    fn normalize_relative_path_rejects_windows_rooted_path_without_prefix() {
        let err = normalize_relative_path(Path::new(r"\escape.wav")).unwrap_err();
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

    #[cfg(unix)]
    #[test]
    fn normalize_relative_path_rejects_non_unicode_names_without_aliasing_them() {
        use std::os::unix::ffi::OsStringExt;

        let path = PathBuf::from(std::ffi::OsString::from_vec(b"kick-\xFF.wav".to_vec()));
        let err = normalize_relative_path(&path).unwrap_err();
        assert!(matches!(err, SourceDbError::NonUnicodeRelativePath(_)));
    }

    #[test]
    fn normalize_relative_path_preserves_case_and_normalization_distinctions() {
        assert_ne!(
            normalize_relative_path(Path::new("Kick.wav")).unwrap(),
            normalize_relative_path(Path::new("kick.wav")).unwrap()
        );
        assert_ne!(
            normalize_relative_path(Path::new("café.wav")).unwrap(),
            normalize_relative_path(Path::new("café.wav")).unwrap()
        );
    }
}
