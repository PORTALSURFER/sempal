//! Utility helpers for retained-restore merge operations.

use crate::sample_sources::normalize_relative_path;
use std::fs;
use std::path::{Path, PathBuf};
use wavecrate_library::timestamps::system_time_to_unix_nanos;

pub(super) fn files_match(left: &Path, right: &Path) -> Result<bool, String> {
    let left_meta =
        fs::metadata(left).map_err(|err| format!("Failed to read staged metadata: {err}"))?;
    let right_meta =
        fs::metadata(right).map_err(|err| format!("Failed to read target metadata: {err}"))?;
    if left_meta.len() != right_meta.len() {
        return Ok(false);
    }
    let left_hash = blake3::hash(
        &fs::read(left)
            .map_err(|err| format!("Failed to read staged file for comparison: {err}"))?,
    );
    let right_hash = blake3::hash(
        &fs::read(right)
            .map_err(|err| format!("Failed to read target file for comparison: {err}"))?,
    );
    Ok(left_hash == right_hash)
}

pub(super) fn modified_nanos(path: &Path) -> Result<i64, String> {
    let modified = fs::metadata(path)
        .and_then(|meta| meta.modified())
        .map_err(|err| format!("Failed to read modified time for {}: {err}", path.display()))?;
    Ok(system_time_to_unix_nanos(modified))
}

pub(super) fn source_relative(source_root: &Path, absolute: &Path) -> Result<PathBuf, String> {
    let relative = absolute
        .strip_prefix(source_root)
        .map_err(|_| format!("Restored path escaped source root: {}", absolute.display()))?;
    normalize_relative_path(relative)
        .map(PathBuf::from)
        .map_err(|err| {
            format!(
                "Invalid restored relative path {}: {err}",
                relative.display()
            )
        })
}

pub(super) fn timestamped_conflict_path(path: &Path, label: &str, stamp: &str) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("entry");
    let (base, extension) = split_name(name, path.is_file());
    for idx in 0..=1000 {
        let suffix = if idx == 0 {
            format!("{label}-{stamp}")
        } else {
            format!("{label}-{stamp}-{idx}")
        };
        let candidate_name = match extension.as_deref() {
            Some(ext) => format!("{base}.{suffix}.{ext}"),
            None => format!("{base}.{suffix}"),
        };
        let candidate = parent.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{name}.{label}-{stamp}-overflow"))
}

pub(super) fn read_dir_paths(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|err| format!("Failed to read restored directory {}: {err}", dir.display()))?
        .map(|entry| entry.map(|value| value.path()))
        .collect::<Result<_, _>>()
        .map_err(|err| {
            format!(
                "Failed to enumerate restored directory {}: {err}",
                dir.display()
            )
        })?;
    paths.sort();
    Ok(paths)
}

pub(super) fn remove_dir_if_empty(dir: &Path) -> Result<(), String> {
    let mut entries = fs::read_dir(dir).map_err(|err| {
        format!(
            "Failed to inspect retained staging directory {}: {err}",
            dir.display()
        )
    })?;
    if entries.next().is_none() {
        fs::remove_dir(dir).map_err(|err| {
            format!(
                "Failed to clear retained staging directory {}: {err}",
                dir.display()
            )
        })?;
    }
    Ok(())
}

fn split_name(name: &str, is_file: bool) -> (String, Option<String>) {
    if !is_file {
        return (name.to_string(), None);
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(name);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_string);
    (stem.to_string(), extension)
}
