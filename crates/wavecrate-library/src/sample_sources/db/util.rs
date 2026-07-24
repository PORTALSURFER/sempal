use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

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

const INDEX_PATH_ENCODING_PLAIN: i64 = 0;
const INDEX_PATH_ENCODING_LOSSLESS: i64 = 1;
const INDEX_NON_UNICODE_COMPONENT_PREFIX: &str = "~wavecrate-nu~";
const INDEX_ESCAPED_COMPONENT_PREFIX: &str = "~wavecrate-escaped~";

pub(super) fn source_index_plain_path_needs_rekey(path: &str) -> bool {
    path.split('/').any(|component| {
        component.starts_with(INDEX_NON_UNICODE_COMPONENT_PREFIX)
            || component.starts_with(INDEX_ESCAPED_COMPONENT_PREFIX)
    })
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

/// Normalize an index-only path while preserving non-Unicode components.
///
/// Supported sample rows intentionally continue to use `normalize_relative_path`:
/// they require a normal UTF-8 database key. Index-only rows have a separate
/// encoding bit so their SQLite key can retain the exact raw bytes without
/// changing the representation of existing Unicode paths. Plain components
/// stay readable and therefore retain exact-subtree prefix ordering.
pub(super) fn normalize_source_index_path(path: &Path) -> Result<(String, i64), SourceDbError> {
    let cleaned = sanitize_relative_path(path)?;
    let has_reserved_component = cleaned.components().any(|component| {
        let Component::Normal(part) = component else {
            return false;
        };
        part.to_str().is_some_and(|value| {
            value.starts_with(INDEX_NON_UNICODE_COMPONENT_PREFIX)
                || value.starts_with(INDEX_ESCAPED_COMPONENT_PREFIX)
        })
    });
    if cleaned.to_str().is_some() && !has_reserved_component {
        return normalize_relative_path(&cleaned).map(|value| (value, INDEX_PATH_ENCODING_PLAIN));
    }

    let mut components = Vec::new();
    for component in cleaned.components() {
        let Component::Normal(part) = component else {
            return Err(SourceDbError::InvalidRelativePath(cleaned));
        };
        let encoded = match part.to_str() {
            Some(value)
                if !value.starts_with(INDEX_NON_UNICODE_COMPONENT_PREFIX)
                    && !value.starts_with(INDEX_ESCAPED_COMPONENT_PREFIX) =>
            {
                value.to_owned()
            }
            Some(value) => format!(
                "{INDEX_ESCAPED_COMPONENT_PREFIX}{}",
                encode_hex(value.as_bytes())
            ),
            #[cfg(unix)]
            None => format!(
                "{INDEX_NON_UNICODE_COMPONENT_PREFIX}{}",
                encode_hex(part.as_bytes())
            ),
            #[cfg(not(unix))]
            None => return Err(SourceDbError::NonUnicodeRelativePath(cleaned)),
        };
        components.push(encoded);
    }
    Ok((components.join("/"), INDEX_PATH_ENCODING_LOSSLESS))
}

/// Parse and validate a stored relative path from the database.
///
/// Returns a normalized `PathBuf` without `.` components.
pub(super) fn parse_relative_path_from_db(path: &str) -> Result<PathBuf, SourceDbError> {
    sanitize_relative_path(Path::new(path))
}

pub(super) fn parse_source_index_path_from_db(
    path: &str,
    encoding: i64,
) -> Result<PathBuf, SourceDbError> {
    match encoding {
        INDEX_PATH_ENCODING_PLAIN => parse_relative_path_from_db(path),
        INDEX_PATH_ENCODING_LOSSLESS => parse_lossless_index_path(path),
        _ => Err(SourceDbError::Unexpected),
    }
}

#[cfg(unix)]
fn parse_lossless_index_path(path: &str) -> Result<PathBuf, SourceDbError> {
    if path.is_empty() {
        return Err(SourceDbError::InvalidRelativePath(PathBuf::from(path)));
    }
    let mut decoded = PathBuf::new();
    for component in path.split('/') {
        if component.is_empty() {
            return Err(SourceDbError::InvalidRelativePath(PathBuf::from(path)));
        }
        let value = if let Some(hex) = component.strip_prefix(INDEX_NON_UNICODE_COMPONENT_PREFIX) {
            OsString::from_vec(
                decode_hex(hex)
                    .ok_or_else(|| SourceDbError::InvalidRelativePath(PathBuf::from(path)))?,
            )
        } else if let Some(hex) = component.strip_prefix(INDEX_ESCAPED_COMPONENT_PREFIX) {
            let bytes = decode_hex(hex)
                .ok_or_else(|| SourceDbError::InvalidRelativePath(PathBuf::from(path)))?;
            OsString::from_vec(bytes)
        } else {
            OsString::from(component)
        };
        decoded.push(value);
    }
    Ok(decoded)
}

#[cfg(not(unix))]
fn parse_lossless_index_path(path: &str) -> Result<PathBuf, SourceDbError> {
    let mut decoded = PathBuf::new();
    for component in path.split('/') {
        if component.is_empty() {
            return Err(SourceDbError::InvalidRelativePath(PathBuf::from(path)));
        }
        if let Some(hex) = component.strip_prefix(INDEX_ESCAPED_COMPONENT_PREFIX) {
            let bytes = decode_hex(hex)
                .ok_or_else(|| SourceDbError::InvalidRelativePath(PathBuf::from(path)))?;
            let value = String::from_utf8(bytes)
                .map_err(|_| SourceDbError::InvalidRelativePath(PathBuf::from(path)))?;
            decoded.push(value);
        } else {
            // A non-Unicode source key can only be authored on Unix. Preserve
            // that component's encoded projection on platforms that cannot
            // reconstruct the original raw name.
            decoded.push(component);
        }
    }
    Ok(decoded)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() || !value.len().is_multiple_of(2) {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Some((hex_digit(pair[0])? << 4) | hex_digit(pair[1])?))
        .collect()
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
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

    #[cfg(unix)]
    #[test]
    fn source_index_path_encoding_round_trips_raw_components_and_subtree_prefixes() {
        use std::os::unix::ffi::OsStringExt;

        let path = PathBuf::from_iter([
            std::ffi::OsString::from("folder"),
            std::ffi::OsString::from_vec(b"kick-\xFF.wav".to_vec()),
        ]);
        let (encoded, encoding) = normalize_source_index_path(&path).unwrap();
        assert_eq!(encoding, INDEX_PATH_ENCODING_LOSSLESS);
        assert_eq!(
            parse_source_index_path_from_db(&encoded, encoding).unwrap(),
            path
        );
        let (lower, upper) = exact_subtree_path_bounds(&encoded);
        assert!(lower.starts_with("folder/"));
        assert!(upper.starts_with("folder/"));
    }

    #[test]
    fn source_index_path_encoding_escapes_reserved_unicode_components() {
        let path = PathBuf::from_iter([
            std::ffi::OsString::from("folder"),
            std::ffi::OsString::from("~wavecrate-nu~ff.wav"),
        ]);
        let (encoded, encoding) = normalize_source_index_path(&path).unwrap();
        assert_eq!(encoding, INDEX_PATH_ENCODING_LOSSLESS);
        assert_eq!(
            parse_source_index_path_from_db(&encoded, encoding).unwrap(),
            path
        );
    }

    #[cfg(unix)]
    #[test]
    fn source_index_path_encoding_does_not_alias_reserved_unicode_names() {
        use std::os::unix::ffi::OsStringExt;

        let raw = PathBuf::from(OsString::from_vec(b"~wavecrate-nu~ff".to_vec()));
        let invalid = PathBuf::from(OsString::from_vec(b"~wavecrate-nu~\xFF".to_vec()));
        let (raw_key, raw_encoding) = normalize_source_index_path(&raw).unwrap();
        let (invalid_key, invalid_encoding) = normalize_source_index_path(&invalid).unwrap();
        assert_ne!(
            (raw_key.clone(), raw_encoding),
            (invalid_key, invalid_encoding)
        );
        assert_eq!(
            parse_source_index_path_from_db(&raw_key, raw_encoding).unwrap(),
            raw
        );
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
