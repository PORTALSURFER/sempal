use std::collections::HashMap;
use std::path::PathBuf;

use crate::sample_sources::SourceDatabase;
use crate::sample_sources::db::WavEntry;

use super::super::scan_diff::index_by_hash;
use super::{ScanError, ScanMode, ScanStats};

pub(crate) struct ScanContext {
    pub(crate) existing: HashMap<PathBuf, WavEntry>,
    pub(crate) existing_by_hash: HashMap<String, Vec<PathBuf>>,
    pub(crate) stats: ScanStats,
    pub(crate) mode: ScanMode,
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
