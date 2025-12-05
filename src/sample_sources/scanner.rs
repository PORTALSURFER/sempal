use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use thiserror::Error;

use super::db::SourceWriteBatch;
use super::db::WavEntry;
use super::{SourceDatabase, SourceDbError};

/// Summary of a scan run.
#[derive(Debug, Default, Clone)]
pub struct ScanStats {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub total_files: usize,
}

/// Errors that can occur while scanning a source folder.
#[derive(Debug, Error)]
pub enum ScanError {
    #[error("Source root is not a directory: {0}")]
    InvalidRoot(PathBuf),
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

/// Recursively scan the source root, syncing .wav files into the database.
pub fn scan_once(db: &SourceDatabase) -> Result<ScanStats, ScanError> {
    let root = ensure_root_dir(db)?;
    let mut stats = ScanStats::default();
    let mut existing = index_existing(db)?;
    let mut batch = db.write_batch()?;
    visit_dir(&root, &mut |path| {
        sync_file(&mut batch, &root, path, &mut existing, &mut stats)
    })?;
    remove_missing(&mut batch, existing, &mut stats)?;
    batch.commit()?;
    Ok(stats)
}

/// Spawn a background thread that opens the source database and performs one scan.
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
    visitor: &mut impl FnMut(&Path) -> Result<(), ScanError>,
) -> Result<(), ScanError> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).map_err(|source| ScanError::Io {
            path: dir.clone(),
            source,
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if is_wav(&path) {
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
    apply_diff(batch, facts, existing, stats)?;
    stats.total_files += 1;
    Ok(())
}

fn remove_missing(
    batch: &mut SourceWriteBatch<'_>,
    existing: HashMap<PathBuf, WavEntry>,
    stats: &mut ScanStats,
) -> Result<(), ScanError> {
    for leftover in existing.values() {
        batch.remove_file(&leftover.relative_path)?;
        stats.removed += 1;
    }
    Ok(())
}

fn strip_relative(root: &Path, path: &Path) -> Result<PathBuf, ScanError> {
    path.strip_prefix(root)
        .map(PathBuf::from)
        .map_err(|_| ScanError::InvalidRoot(path.to_path_buf()))
}

#[derive(Debug)]
struct FileFacts {
    relative: PathBuf,
    size: u64,
    modified_ns: i64,
}

fn read_facts(root: &Path, path: &Path) -> Result<FileFacts, ScanError> {
    let relative = strip_relative(root, path)?;
    let meta = path.metadata().map_err(|source| ScanError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let modified_ns = to_nanos(
        &meta.modified().map_err(|source| ScanError::Io {
            path: path.to_path_buf(),
            source,
        })?,
        path,
    )?;
    Ok(FileFacts {
        relative,
        size: meta.len(),
        modified_ns,
    })
}

fn apply_diff(
    batch: &mut SourceWriteBatch<'_>,
    facts: FileFacts,
    existing: &mut HashMap<PathBuf, WavEntry>,
    stats: &mut ScanStats,
) -> Result<(), ScanError> {
    let path = facts.relative.clone();
    match existing.remove(&path) {
        Some(entry) if entry.file_size == facts.size && entry.modified_ns == facts.modified_ns => {}
        Some(_) => {
            batch.upsert_file(&path, facts.size, facts.modified_ns)?;
            stats.updated += 1;
        }
        None => {
            batch.upsert_file(&path, facts.size, facts.modified_ns)?;
            stats.added += 1;
        }
    }
    Ok(())
}

fn to_nanos(time: &SystemTime, path: &Path) -> Result<i64, ScanError> {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ScanError::Time {
            path: path.to_path_buf(),
        })?;
    Ok(duration.as_nanos().min(i64::MAX as u128) as i64)
}

fn is_wav(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_sources::SampleTag;
    use tempfile::tempdir;

    #[test]
    fn scan_add_update_remove() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("one.wav");
        std::fs::write(&file_path, b"one").unwrap();

        let db = SourceDatabase::open(dir.path()).unwrap();
        let first = scan_once(&db).unwrap();
        assert_eq!(first.added, 1);
        let initial = db.list_files().unwrap();
        assert_eq!(initial.len(), 1);
        assert_eq!(initial[0].tag, SampleTag::Neutral);

        std::fs::write(&file_path, b"longer-data").unwrap();
        let second = scan_once(&db).unwrap();
        assert_eq!(second.updated, 1);

        std::fs::remove_file(&file_path).unwrap();
        let third = scan_once(&db).unwrap();
        assert_eq!(third.removed, 1);
        assert_eq!(db.list_files().unwrap().len(), 0);
    }
}
