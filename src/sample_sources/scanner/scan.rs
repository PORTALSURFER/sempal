use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

use thiserror::Error;
use tracing::warn;

use crate::sample_sources::db::{SourceWriteBatch, WavEntry};
use crate::sample_sources::{SourceDatabase, SourceDbError};

use super::metadata::{compute_content_hash, is_supported_audio, read_facts, FileFacts};

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
    let mut stats = ScanStats::default();
    let mut existing = index_existing(db)?;
    let mut batch = db.write_batch()?;
    visit_dir(&root, cancel, &mut |path| {
        if let Some(cancel) = cancel
            && cancel.load(Ordering::Relaxed)
        {
            return Err(ScanError::Canceled);
        }
        sync_file(&mut batch, &root, path, &mut existing, &mut stats)?;
        if let Some(on_progress) = on_progress.as_mut() {
            on_progress(stats.total_files, path);
        }
        Ok(())
    })?;
    mark_missing(&mut batch, existing, &mut stats, mode)?;
    batch.commit()?;
    Ok(stats)
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

fn visit_dir(
    root: &Path,
    cancel: Option<&AtomicBool>,
    visitor: &mut impl FnMut(&Path) -> Result<(), ScanError>,
) -> Result<(), ScanError> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Some(cancel) = cancel
            && cancel.load(Ordering::Relaxed)
        {
            return Err(ScanError::Canceled);
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(source) if dir != root => {
                warn!(
                    dir = %dir.display(),
                    error = %source,
                    "Failed to read directory during scan"
                );
                continue;
            }
            Err(source) => {
                return Err(ScanError::Io {
                    path: dir.clone(),
                    source,
                });
            }
        };
        for entry_result in entries {
            let entry = match entry_result {
                Ok(entry) => entry,
                Err(err) => {
                    warn!(
                        dir = %dir.display(),
                        error = %err,
                        "Failed to read directory entry during scan"
                    );
                    continue;
                }
            };

            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(err) => {
                    warn!(
                        path = %path.display(),
                        error = %err,
                        "Failed to read file type during scan"
                    );
                    continue;
                }
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if file_type.is_file() && is_supported_audio(&path) {
                visitor(&path)?;
            }
        }
    }
    Ok(())
}

fn ensure_root_dir(db: &SourceDatabase) -> Result<PathBuf, ScanError> {
    let root = db.root().to_path_buf();
    if root.is_dir() {
        Ok(root)
    } else {
        Err(ScanError::InvalidRoot(root))
    }
}

fn sync_file(
    batch: &mut SourceWriteBatch<'_>,
    root: &Path,
    path: &Path,
    existing: &mut HashMap<PathBuf, WavEntry>,
    stats: &mut ScanStats,
) -> Result<(), ScanError> {
    let facts = read_facts(root, path)?;
    apply_diff(batch, facts, existing, stats, root)?;
    stats.total_files += 1;
    Ok(())
}

fn mark_missing(
    batch: &mut SourceWriteBatch<'_>,
    existing: HashMap<PathBuf, WavEntry>,
    stats: &mut ScanStats,
    mode: ScanMode,
) -> Result<(), ScanError> {
    for leftover in existing.values() {
        match mode {
            ScanMode::Quick => {
                if leftover.missing {
                    continue;
                }
                batch.set_missing(&leftover.relative_path, true)?;
                stats.missing += 1;
            }
            ScanMode::Hard => {
                batch.remove_file(&leftover.relative_path)?;
                stats.missing += 1;
            }
        }
    }
    Ok(())
}

fn apply_diff(
    batch: &mut SourceWriteBatch<'_>,
    facts: FileFacts,
    existing: &mut HashMap<PathBuf, WavEntry>,
    stats: &mut ScanStats,
    root: &Path,
) -> Result<(), ScanError> {
    let path = facts.relative.clone();
    match existing.remove(&path) {
        Some(entry) if entry.file_size == facts.size && entry.modified_ns == facts.modified_ns => {
            if entry.missing {
                batch.set_missing(&path, false)?;
            }
        }
        Some(entry) => {
            let absolute = root.join(&path);
            let hash = compute_content_hash(&absolute)?;
            let previous_hash = entry.content_hash.as_deref();
            batch.upsert_file_with_hash(&path, facts.size, facts.modified_ns, &hash)?;
            if previous_hash != Some(hash.as_str()) {
                stats.content_changed += 1;
                stats.changed_samples.push(ChangedSample {
                    relative_path: path.clone(),
                    file_size: facts.size,
                    modified_ns: facts.modified_ns,
                    content_hash: hash,
                });
            }
            stats.updated += 1;
        }
        None => {
            let absolute = root.join(&path);
            let hash = compute_content_hash(&absolute)?;
            batch.upsert_file_with_hash(&path, facts.size, facts.modified_ns, &hash)?;
            stats.added += 1;
            stats.content_changed += 1;
            stats.changed_samples.push(ChangedSample {
                relative_path: path.clone(),
                file_size: facts.size,
                modified_ns: facts.modified_ns,
                content_hash: hash,
            });
        }
    }
    Ok(())
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
}
