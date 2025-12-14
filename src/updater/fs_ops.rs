use std::{
    fs,
    path::{Path, PathBuf},
};

use super::UpdateError;

pub(super) fn ensure_empty_dir(path: &Path) -> Result<(), UpdateError> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    fs::create_dir_all(path)?;
    Ok(())
}

pub(super) fn list_root_entries(path: &Path) -> Result<Vec<PathBuf>, UpdateError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        entries.push(entry.path());
    }
    Ok(entries)
}

pub(super) fn copy_file_atomic(src: &Path, dest: &Path) -> Result<(), UpdateError> {
    let new_path = with_suffix(dest, "new");
    let old_path = with_suffix(dest, "old");
    if old_path.exists() {
        let _ = fs::remove_file(&old_path);
    }
    if new_path.exists() {
        let _ = fs::remove_file(&new_path);
    }
    fs::copy(src, &new_path)?;
    if dest.exists() {
        fs::rename(dest, &old_path)?;
    }
    fs::rename(&new_path, dest)?;
    Ok(())
}

pub(super) fn replace_dir(src: &Path, dest: &Path) -> Result<(), UpdateError> {
    let new_path = with_suffix(dest, "new");
    let old_path = with_suffix(dest, "old");
    if old_path.exists() {
        let _ = fs::remove_dir_all(&old_path);
    }
    if new_path.exists() {
        let _ = fs::remove_dir_all(&new_path);
    }
    copy_dir_all(src, &new_path)?;
    if dest.exists() {
        fs::rename(dest, &old_path)?;
    }
    fs::rename(&new_path, dest)?;
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();
    name.push('.');
    name.push_str(suffix);
    path.with_file_name(name)
}

fn copy_dir_all(src: &Path, dest: &Path) -> Result<(), UpdateError> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&src_path, &dest_path)?;
        } else if ty.is_file() {
            fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

