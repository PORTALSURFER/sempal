use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::thread;

use crate::sample_sources::SourceDatabase;

use super::{ScanContext, ScanError, ScanStats};
use super::super::scan_db_sync::db_sync_phase;
use super::super::scan_fs::ensure_root_dir;
use super::super::scan_walk::walk_phase;

/// Scan strategy used when walking a source root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    /// Update the database with new/modified files and mark missing entries.
    Quick,
    /// Force a full rescan, pruning missing rows to rebuild state from disk.
    Hard,
}

/// Recursively scan the source root, syncing supported audio files into the database.
/// Returns counts of added/updated/removed rows.
pub fn scan_once(db: &SourceDatabase) -> Result<ScanStats, ScanError> {
    scan(db, ScanMode::Quick, None, None)
}

/// Rescan the entire source, pruning rows for files that no longer exist.
pub fn hard_rescan(db: &SourceDatabase) -> Result<ScanStats, ScanError> {
    scan(db, ScanMode::Hard, None, None)
}

pub fn scan_with_progress(
    db: &SourceDatabase,
    mode: ScanMode,
    cancel: Option<&AtomicBool>,
    on_progress: &mut impl FnMut(usize, &Path),
) -> Result<ScanStats, ScanError> {
    scan(db, mode, cancel, Some(on_progress))
}

fn scan(
    db: &SourceDatabase,
    mode: ScanMode,
    cancel: Option<&AtomicBool>,
    mut on_progress: Option<&mut dyn FnMut(usize, &Path)>,
) -> Result<ScanStats, ScanError> {
    let root = ensure_root_dir(db)?;
    let mut context = ScanContext::new(db, mode)?;
    let mut batch = db.write_batch()?;
    walk_phase(&root, cancel, &mut on_progress, &mut context, &mut batch)?;
    db_sync_phase(db, batch, &mut context)?;
    Ok(context.stats)
}

/// Spawn a background thread that opens the source database and performs one scan.
/// Useful for fire-and-forget refreshes without blocking the UI thread.
#[allow(dead_code)]
pub fn scan_in_background(root: PathBuf) -> thread::JoinHandle<Result<ScanStats, ScanError>> {
    thread::spawn(move || {
        let db = SourceDatabase::open(root)?;
        scan_once(&db)
    })
}
