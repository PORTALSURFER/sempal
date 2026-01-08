use super::super::*;
use crate::sample_sources::collections::CollectionMember;
use std::path::{Path, PathBuf};

pub(crate) fn resolved_export_dir(
    collection: &Collection,
    global_root: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = collection.export_path.clone() {
        Some(path)
    } else {
        global_root.map(|root| {
            crate::sample_sources::config::normalize_path(
                root.join(collection_folder_name(collection)).as_path(),
            )
        })
    }
}

pub(crate) fn export_dir_for(
    collection: &Collection,
    global_root: Option<&Path>,
) -> Result<PathBuf, String> {
    resolved_export_dir(collection, global_root).ok_or_else(|| "Set an export folder first".into())
}

pub(crate) fn collection_folder_name(collection: &Collection) -> String {
    collection.export_folder_name()
}

pub(crate) fn delete_exported_file(
    export_dir: Option<PathBuf>,
    member: &CollectionMember,
) {
    let Some(dir) = export_dir else {
        return;
    };
    let file_name = match member.relative_path.file_name() {
        Some(name) => name,
        None => return,
    };
    let target = dir.join(file_name);
    let _ = std::fs::remove_file(target);
}

pub(crate) fn collection_folder_name_from_str(name: &str) -> String {
    crate::sample_sources::collections::collection_folder_name_from_str(name)
}
