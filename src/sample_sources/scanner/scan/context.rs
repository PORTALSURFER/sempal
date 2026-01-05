use std::collections::HashMap;
use std::path::PathBuf;

use crate::sample_sources::db::WavEntry;
use crate::sample_sources::SourceDatabase;

use super::{ScanError, ScanMode, ScanStats};
use super::super::scan_diff::index_by_hash;

pub(super) struct ScanContext {
    pub(super) existing: HashMap<PathBuf, WavEntry>,
    pub(super) existing_by_hash: HashMap<String, Vec<PathBuf>>,
    pub(super) stats: ScanStats,
    pub(super) mode: ScanMode,
}

impl ScanContext {
    pub(super) fn new(db: &SourceDatabase, mode: ScanMode) -> Result<Self, ScanError> {
        let existing = index_existing(db)?;
        let existing_by_hash = index_by_hash(&existing);
        Ok(Self {
            existing,
            existing_by_hash,
            stats: ScanStats::default(),
            mode,
        })
    }
}

fn index_existing(db: &SourceDatabase) -> Result<HashMap<PathBuf, WavEntry>, ScanError> {
    let entries = db.list_files()?;
    Ok(entries
        .into_iter()
        .map(|entry| (entry.relative_path.clone(), entry))
        .collect())
}
