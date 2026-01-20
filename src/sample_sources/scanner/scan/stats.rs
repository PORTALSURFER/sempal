use std::path::PathBuf;

/// Summary of a scan run.
#[derive(Debug, Default, Clone)]
pub struct ScanStats {
    /// Number of newly discovered files.
    pub added: usize,
    /// Number of files updated in-place.
    pub updated: usize,
    /// Number of files now missing from disk.
    pub missing: usize,
    /// Total number of files scanned.
    pub total_files: usize,
    /// Number of files with changed content hashes.
    pub content_changed: usize,
    /// Number of files whose content hashes were computed during the scan.
    pub hashes_computed: usize,
    /// Number of files whose content hashes were deferred during the scan.
    pub hashes_pending: usize,
    /// Number of missing rows reconciled to renamed files.
    pub renames_reconciled: usize,
    /// Detailed list of changed samples.
    pub changed_samples: Vec<ChangedSample>,
}

/// Metadata describing a sample whose content changed.
#[derive(Debug, Clone)]
pub struct ChangedSample {
    /// Path relative to the source root.
    pub relative_path: PathBuf,
    /// File size in bytes.
    pub file_size: u64,
    /// Last modified timestamp in epoch nanoseconds.
    pub modified_ns: i64,
    /// Updated content hash.
    pub content_hash: String,
}
