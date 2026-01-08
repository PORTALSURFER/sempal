use super::super::*;
use crate::sample_sources::collections::CollectionMember;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(crate) fn copy_member_to_export(
    export_root: &Path,
    source: &SampleSource,
    member: &CollectionMember,
) -> Result<(), String> {
    let source_path = source.root.join(&member.relative_path);
    if !source_path.is_file() {
        return Err(format!(
            "File missing for export: {}",
            source_path.display()
        ));
    }
    let file_name = member
        .relative_path
        .file_name()
        .ok_or_else(|| "Invalid filename for export".to_string())?;
    let dest = export_root.join(file_name);
    if dest == source_path {
        return Ok(());
    }
    std::fs::create_dir_all(export_root).map_err(|err| {
        format!(
            "Failed to create export folder {}: {err}",
            export_root.display()
        )
    })?;
    std::fs::copy(&source_path, &dest)
        .map_err(|err| format!("Failed to export {}: {err}", dest.display()))?;
    Ok(())
}

pub(crate) fn collect_exported_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|err| format!("Unable to read export folder {}: {err}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|err| format!("Unable to read export entry: {err}"))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if !path.is_file() {
                continue;
            }
            let Some(rel_path) = path.strip_prefix(root).ok().map(PathBuf::from) else {
                continue;
            };
            let Some(name) = rel_path.file_name() else {
                continue;
            };
            if seen.insert(name.to_os_string()) {
                files.push(rel_path);
            }
        }
    }
    Ok(files)
}

pub(crate) fn ensure_export_dir(path: &Path) -> Result<(), String> {
    if path.exists() && !path.is_dir() {
        return Err(format!(
            "Export path is not a directory: {}",
            path.display()
        ));
    }
    if !path.exists() {
        std::fs::create_dir_all(path)
            .map_err(|err| format!("Unable to create export folder {}: {err}", path.display()))?;
    }
    Ok(())
}
