use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(crate) fn normalize_folder_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Folder name cannot be empty".into());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("Folder name is invalid".into());
    }
    if trimmed.contains(['/', '\\']) {
        return Err("Folder name cannot contain path separators".into());
    }
    Ok(trimmed.to_string())
}

pub(crate) fn folder_with_name(target: &Path, name: &str) -> PathBuf {
    target.parent().map_or_else(
        || PathBuf::from(name),
        |parent| {
            if parent.as_os_str().is_empty() {
                PathBuf::from(name)
            } else {
                parent.join(name)
            }
        },
    )
}

pub(crate) fn rename_target(target: &Path, new_name: &str) -> Result<PathBuf, String> {
    let name = normalize_folder_name(new_name)?;
    Ok(folder_with_name(target, &name))
}


pub(crate) fn normalize_folder_hotkey(hotkey: Option<u8>) -> Result<Option<u8>, String> {
    match hotkey {
        None => Ok(None),
        Some(slot) if slot <= 9 => Ok(Some(slot)),
        Some(_) => Err("Folder hotkey must be between 0 and 9".into()),
    }
}

pub(crate) fn remap_path_set(set: &mut BTreeSet<PathBuf>, old: &Path, new: &Path) {
    let descendants: Vec<PathBuf> = set
        .iter()
        .filter(|path| path.starts_with(old))
        .cloned()
        .collect();
    if descendants.is_empty() {
        return;
    }
    set.retain(|path| !path.starts_with(old));
    for path in descendants {
        let suffix = path.strip_prefix(old).unwrap_or_else(|_| Path::new(""));
        set.insert(new.join(suffix));
    }
}

pub(crate) fn remap_path_map(map: &mut BTreeMap<u8, PathBuf>, old: &Path, new: &Path) {
    let updates: Vec<(u8, PathBuf)> = map
        .iter()
        .filter(|(_, path)| path.starts_with(old))
        .map(|(slot, path)| {
            let suffix = path.strip_prefix(old).unwrap_or_else(|_| Path::new(""));
            (*slot, new.join(suffix))
        })
        .collect();
    for (slot, path) in updates {
        map.insert(slot, path);
    }
}

pub(crate) fn remap_path_option(value: Option<PathBuf>, old: &Path, new: &Path) -> Option<PathBuf> {
    let value = value?;
    if !value.starts_with(old) {
        return Some(value);
    }
    let suffix = value.strip_prefix(old).unwrap_or_else(|_| Path::new(""));
    Some(new.join(suffix))
}
