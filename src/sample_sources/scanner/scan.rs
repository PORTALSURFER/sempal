use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    thread,
};

use thiserror::Error;

use crate::sample_sources::db::WavEntry;
use crate::sample_sources::{SourceDatabase, SourceDbError};

use super::scan_db_sync::db_sync_phase;
use super::scan_diff::index_by_hash;
use super::scan_fs::ensure_root_dir;
use super::scan_walk::walk_phase;

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

/// Scan strategy used when walking a source root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    /// Update the database with new/modified files and mark missing entries.
    Quick,
    /// Force a full rescan, pruning missing rows to rebuild state from disk.
    Hard,
}

/// Errors that can occur while scanning a source folder.
#[derive(Debug, Error)]
pub enum ScanError {
    #[error("Source root is not a directory: {0}")]
    InvalidRoot(PathBuf),
    #[error("Scan canceled")]
    Canceled,
    #[error("Failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Database error: {0}")]
    Db(#[from] SourceDbError),
    #[error("Time conversion failed for {path}")]
    Time { path: PathBuf },
}

pub(super) struct ScanContext {
    pub(super) existing: HashMap<PathBuf, WavEntry>,
    pub(super) existing_by_hash: HashMap<String, Vec<PathBuf>>,
    pub(super) stats: ScanStats,
    pub(super) mode: ScanMode,
}

impl ScanContext {
    fn new(db: &SourceDatabase, mode: ScanMode) -> Result<Self, ScanError> {
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
    walk_phase(&root, cancel, on_progress.as_deref_mut(), &mut context, &mut batch)?;
    db_sync_phase(db, &mut batch, &mut context)?;
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

fn index_existing(db: &SourceDatabase) -> Result<HashMap<PathBuf, WavEntry>, ScanError> {
    let entries = db.list_files()?;
    Ok(entries
        .into_iter()
        .map(|entry| (entry.relative_path.clone(), entry))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_sources::SampleTag;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn scan_add_update_mark_missing() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("one.wav");
        std::fs::write(&file_path, b"one").unwrap();

        let db = SourceDatabase::open(dir.path()).unwrap();
        let first = scan_once(&db).unwrap();
        assert_eq!(first.added, 1);
        assert_eq!(first.content_changed, 1);
        assert_eq!(first.changed_samples.len(), 1);
        let initial = db.list_files().unwrap();
        assert_eq!(initial.len(), 1);
        assert_eq!(initial[0].tag, SampleTag::Neutral);

        std::fs::write(&file_path, b"longer-data").unwrap();
        let second = scan_once(&db).unwrap();
        assert_eq!(second.updated, 1);
        assert_eq!(second.content_changed, 1);
        assert_eq!(second.changed_samples.len(), 1);

        std::fs::remove_file(&file_path).unwrap();
        let third = scan_once(&db).unwrap();
        assert_eq!(third.missing, 1);
        let rows = db.list_files().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].missing);
        let fourth = scan_once(&db).unwrap();
        assert_eq!(fourth.missing, 0);

        std::fs::write(&file_path, b"one").unwrap();
        let fifth = scan_once(&db).unwrap();
        assert_eq!(fifth.added, 0);
        assert_eq!(fifth.updated, 1);
        assert_eq!(fifth.content_changed, 1);
        assert_eq!(fifth.changed_samples.len(), 1);
        let rows = db.list_files().unwrap();
        assert!(!rows[0].missing);
    }

    #[test]
    fn scan_skips_analysis_when_hash_unchanged() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("one.wav");
        std::fs::write(&file_path, b"one").unwrap();

        let db = SourceDatabase::open(dir.path()).unwrap();
        let first = scan_once(&db).unwrap();
        assert_eq!(first.content_changed, 1);

        std::thread::sleep(Duration::from_millis(2));
        std::fs::write(&file_path, b"one").unwrap();

        let second = scan_once(&db).unwrap();
        assert_eq!(second.updated, 1);
        assert_eq!(second.content_changed, 0);
        assert!(second.changed_samples.is_empty());
    }

    #[test]
    fn scan_ignores_non_wav_and_counts_nested() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.path().join("one.wav"), b"one").unwrap();
        std::fs::write(nested.join("two.wav"), b"two").unwrap();
        std::fs::write(dir.path().join("ignore.txt"), b"text").unwrap();

        let db = SourceDatabase::open(dir.path()).unwrap();
        let stats = scan_once(&db).unwrap();
        assert_eq!(stats.added, 2);
        assert_eq!(stats.total_files, 2);
        let rows = db.list_files().unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn scan_in_background_finishes() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("one.wav"), b"one").unwrap();
        let handle = scan_in_background(dir.path().to_path_buf());
        let stats = handle.join().unwrap().unwrap();
        assert_eq!(stats.added, 1);
    }

    #[test]
    fn scan_with_progress_respects_cancel_flag() {
        use std::sync::atomic::AtomicBool;

        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("one.wav"), b"one").unwrap();
        let db = SourceDatabase::open(dir.path()).unwrap();

        let cancel = AtomicBool::new(true);
        let mut progress_called = false;
        let result = scan_with_progress(&db, ScanMode::Quick, Some(&cancel), &mut |_, _| {
            progress_called = true;
        });
        assert!(matches!(result, Err(ScanError::Canceled)));
        assert!(!progress_called);
    }

    #[test]
    fn hard_rescan_prunes_missing_rows() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("one.wav");
        std::fs::write(&file_path, b"one").unwrap();
        let db = SourceDatabase::open(dir.path()).unwrap();
        scan_once(&db).unwrap();

        std::fs::remove_file(&file_path).unwrap();
        scan_once(&db).unwrap();
        let rows = db.list_files().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].missing);

        let stats = hard_rescan(&db).unwrap();
        assert_eq!(stats.missing, 1);
        let rows = db.list_files().unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn scan_detects_missing_paths_without_double_counting() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("one.wav");
        std::fs::write(&file_path, b"one").unwrap();

        let db = SourceDatabase::open(dir.path()).unwrap();
        scan_once(&db).unwrap();

        std::fs::remove_file(&file_path).unwrap();
        let first = scan_once(&db).unwrap();
        assert_eq!(first.missing, 1);

        let second = scan_once(&db).unwrap();
        assert_eq!(second.missing, 0);
    }

    #[test]
    fn scan_detects_changed_content_hash() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("one.wav");
        std::fs::write(&file_path, b"one").unwrap();

        let db = SourceDatabase::open(dir.path()).unwrap();
        scan_once(&db).unwrap();

        std::fs::write(&file_path, b"two").unwrap();
        let stats = scan_once(&db).unwrap();
        assert_eq!(stats.content_changed, 1);
        assert_eq!(stats.changed_samples.len(), 1);
    }

    #[test]
    fn scan_detects_rename_and_preserves_tag() {
        let dir = tempdir().unwrap();
        let first_path = dir.path().join("one.wav");
        let second_path = dir.path().join("two.wav");
        std::fs::write(&first_path, b"one").unwrap();

        let db = SourceDatabase::open(dir.path()).unwrap();
        scan_once(&db).unwrap();
        db.set_tag(Path::new("one.wav"), SampleTag::Keep).unwrap();

        std::fs::rename(&first_path, &second_path).unwrap();
        let stats = scan_once(&db).unwrap();

        assert_eq!(stats.missing, 0);
        assert_eq!(stats.added, 0);
        assert_eq!(stats.content_changed, 0);
        assert_eq!(stats.updated, 1);

        let rows = db.list_files().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].relative_path, PathBuf::from("two.wav"));
        assert_eq!(rows[0].tag, SampleTag::Keep);
        assert!(!rows[0].missing);
    }

    #[test]
    fn hard_rescan_prunes_missing_files_with_tags() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("one.wav");
        std::fs::write(&file_path, b"one").unwrap();

        let db = SourceDatabase::open(dir.path()).unwrap();
        scan_once(&db).unwrap();
        db.set_tag(Path::new("one.wav"), SampleTag::Keep).unwrap();

        std::fs::remove_file(&file_path).unwrap();
        scan_once(&db).unwrap();

        let stats = hard_rescan(&db).unwrap();
        assert_eq!(stats.missing, 1);
        let rows = db.list_files().unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn hard_rescan_prunes_missing_without_touching_existing() {
        let dir = tempdir().unwrap();
        let keep_path = dir.path().join("keep.wav");
        let remove_path = dir.path().join("remove.wav");
        std::fs::write(&keep_path, b"keep").unwrap();
        std::fs::write(&remove_path, b"remove").unwrap();

        let db = SourceDatabase::open(dir.path()).unwrap();
        scan_once(&db).unwrap();

        std::fs::remove_file(&remove_path).unwrap();
        let stats = hard_rescan(&db).unwrap();
        assert_eq!(stats.missing, 1);

        let rows = db.list_files().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].relative_path, PathBuf::from("keep.wav"));
    }

    #[cfg(unix)]
    #[test]
    fn scan_tolerates_vanishing_nested_directories() {
        let dir = tempdir().unwrap();
        let one = dir.path().join("one.wav");
        std::fs::write(&one, b"one").unwrap();

        let vanishing = dir.path().join("vanishing");
        std::fs::create_dir_all(&vanishing).unwrap();
        std::fs::write(vanishing.join("two.wav"), b"two").unwrap();

        let vanishing_for_thread = vanishing.clone();
        let killer = std::thread::spawn(move || {
            for _ in 0..200 {
                let _ = std::fs::remove_dir_all(&vanishing_for_thread);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });

        let db = SourceDatabase::open(dir.path()).unwrap();
        let stats = scan_once(&db).unwrap();
        assert!(stats.total_files >= 1);

        let rows = db.list_files().unwrap();
        assert!(
            rows.iter()
                .any(|row| row.relative_path == PathBuf::from("one.wav"))
        );

        let _ = killer.join();
    }

    #[cfg(unix)]
    #[test]
    fn scan_skips_symlink_directories() {
        use std::os::unix::fs as unix_fs;

        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("two.wav"), b"two").unwrap();
        std::fs::write(dir.path().join("one.wav"), b"one").unwrap();

        let link = dir.path().join("nested_link");
        unix_fs::symlink(&nested, &link).unwrap();

        let db = SourceDatabase::open(dir.path()).unwrap();
        let stats = scan_once(&db).unwrap();
        assert_eq!(stats.total_files, 2);
        assert_eq!(stats.added, 2);
    }

    #[cfg(unix)]
    #[test]
    fn scan_skips_symlink_files() {
        use std::os::unix::fs as unix_fs;

        let dir = tempdir().unwrap();
        let target = dir.path().join("one.wav");
        std::fs::write(&target, b"one").unwrap();
        let link = dir.path().join("one_link.wav");
        unix_fs::symlink(&target, &link).unwrap();

        let db = SourceDatabase::open(dir.path()).unwrap();
        let stats = scan_once(&db).unwrap();
        assert_eq!(stats.total_files, 1);
        assert_eq!(stats.added, 1);
    }
}
