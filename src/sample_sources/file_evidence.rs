//! Immutable filesystem evidence captured at a source-owned I/O boundary.

use std::path::Path;

use wavecrate_library::timestamps::system_time_to_unix_nanos;

/// Files at or below this size are hashed when capturing mutation evidence.
///
/// This is the existing committed-mutation watcher policy. Keeping the threshold here lets
/// source workers and watcher reconciliation make the same bounded decision without depending on
/// native-app types.
pub const MAX_SOURCE_FILE_EVIDENCE_HASH_BYTES: u64 = 8 * 1024 * 1024;

/// Immutable observation of a path at a filesystem boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceFileEvidence {
    /// The path does not exist.
    Missing,
    /// A bounded-size regular file and its content digest.
    ContentHash([u8; 32]),
    /// Filesystem metadata used when hashing is intentionally bounded out.
    Metadata {
        /// File length in bytes.
        len: u64,
        /// Modification time in Unix nanoseconds, when available.
        modified_ns: Option<i64>,
        /// Whether the path identifies a directory.
        is_dir: bool,
    },
    /// The path could not be observed reliably.
    Unverifiable,
}

/// Capture immutable evidence for one path without opening a source database.
pub fn capture_source_file_evidence(path: &Path) -> SourceFileEvidence {
    match std::fs::metadata(path) {
        Ok(metadata)
            if metadata.is_file() && metadata.len() <= MAX_SOURCE_FILE_EVIDENCE_HASH_BYTES =>
        {
            match std::fs::read(path) {
                Ok(bytes) => SourceFileEvidence::ContentHash(*blake3::hash(&bytes).as_bytes()),
                Err(_) => SourceFileEvidence::Unverifiable,
            }
        }
        Ok(metadata) => SourceFileEvidence::Metadata {
            len: metadata.len(),
            modified_ns: metadata.modified().ok().map(system_time_to_unix_nanos),
            is_dir: metadata.is_dir(),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => SourceFileEvidence::Missing,
        Err(_) => SourceFileEvidence::Unverifiable,
    }
}
