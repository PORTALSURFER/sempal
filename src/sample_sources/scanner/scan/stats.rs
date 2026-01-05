use std::path::PathBuf;

/// Summary of a scan run.
#[derive(Debug, Default, Clone)]
pub struct ScanStats {
    pub added: usize,
    pub updated: usize,
    pub missing: usize,
    pub total_files: usize,
    pub content_changed: usize,
    pub changed_samples: Vec<ChangedSample>,
}

#[derive(Debug, Clone)]
pub struct ChangedSample {
    pub relative_path: PathBuf,
    pub file_size: u64,
    pub modified_ns: i64,
    pub content_hash: String,
}
