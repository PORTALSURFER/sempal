use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use tracing::warn;

use crate::sample_sources::{is_supported_audio, SourceDatabase};

use super::scan::ScanError;

#[derive(Debug)]
pub(super) struct FileFacts {
    pub(super) relative: PathBuf,
    pub(super) size: u64,
    pub(super) modified_ns: i64,
}

pub(super) fn ensure_root_dir(db: &SourceDatabase) -> Result<PathBuf, ScanError> {
    let root = db.root().to_path_buf();
    if root.is_dir() {
        Ok(root)
    } else {
        Err(ScanError::InvalidRoot(root))
    }
}

pub(super) fn visit_dir(
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

pub(super) fn read_facts(root: &Path, path: &Path) -> Result<FileFacts, ScanError> {
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

pub(super) fn compute_content_hash(path: &Path) -> Result<String, ScanError> {
    let mut file = fs::File::open(path).map_err(|source| ScanError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|source| ScanError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn strip_relative(root: &Path, path: &Path) -> Result<PathBuf, ScanError> {
    if let Ok(relative) = path.strip_prefix(root) {
        return Ok(PathBuf::from(relative));
    }
    if let (Ok(canon_root), Ok(canon_path)) = (root.canonicalize(), path.canonicalize()) {
        if let Ok(relative) = canon_path.strip_prefix(&canon_root) {
            return Ok(PathBuf::from(relative));
        }
    }
    Err(ScanError::InvalidRoot(path.to_path_buf()))
}

fn to_nanos(time: &SystemTime, path: &Path) -> Result<i64, ScanError> {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ScanError::Time {
            path: path.to_path_buf(),
        })?;
    Ok(duration.as_nanos().min(i64::MAX as u128) as i64)
}
