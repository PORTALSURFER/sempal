//! Filesystem helpers for harvest and protected-source workflows.

use std::{
    fs,
    path::{Path, PathBuf},
};

use wavecrate_library::timestamps::system_time_to_unix_nanos;

/// Return whether a path currently exists.
pub fn path_exists(path: &Path) -> bool {
    path.exists()
}

/// Return whether a path currently points at a file.
pub fn path_is_file(path: &Path) -> bool {
    path.is_file()
}

/// Return whether a sample source root is currently available on disk.
pub fn source_root_available(path: &Path) -> bool {
    path.is_dir()
}

/// Create a directory tree and format failures with user-facing context.
pub fn ensure_dir(path: &Path, context: &str) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|err| format!("{context} {}: {err}", path.display()))
}

/// Copy a file and format failures with source and destination context.
pub fn copy_file(source: &Path, destination: &Path, context: &str) -> Result<(), String> {
    fs::copy(source, destination).map(|_| ()).map_err(|err| {
        format!(
            "{context} {} to {}: {err}",
            source.display(),
            destination.display()
        )
    })
}

/// Read file size and modified timestamp metadata for harvest identity matching.
pub fn file_identity_metadata(path: &Path) -> (Option<u64>, Option<i64>) {
    let Ok(metadata) = fs::metadata(path) else {
        return (None, None);
    };
    let modified_ns = metadata.modified().ok().map(system_time_to_unix_nanos);
    (Some(metadata.len()), modified_ns)
}

/// Find the next available WAV destination path using a suffix convention.
pub fn next_available_wav_copy_path(
    source_path: &Path,
    target_folder: &Path,
    base_suffix: &str,
    error_message: &'static str,
) -> Result<PathBuf, String> {
    let stem = source_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| String::from("Source sample has no file name"))?;
    for index in 0..10_000 {
        let suffix = if index == 0 {
            base_suffix.to_string()
        } else {
            format!("{base_suffix}_{index}")
        };
        let candidate = target_folder.join(format!("{stem}{suffix}.wav"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(String::from(error_message))
}
