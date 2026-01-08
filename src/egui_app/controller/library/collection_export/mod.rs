mod export;
mod fs_ops;
mod paths;
mod reconcile;
mod settings;

use super::*;
use crate::sample_sources::collections::CollectionMember;
use std::path::{Path, PathBuf};

pub(in crate::egui_app::controller) fn resolved_export_dir(
    collection: &Collection,
    global_root: Option<&Path>,
) -> Option<PathBuf> {
    paths::resolved_export_dir(collection, global_root)
}

pub(in crate::egui_app::controller) fn export_dir_for(
    collection: &Collection,
    global_root: Option<&Path>,
) -> Result<PathBuf, String> {
    paths::export_dir_for(collection, global_root)
}

#[cfg(test)]
pub(in crate::egui_app::controller) fn collection_folder_name(collection: &Collection) -> String {
    paths::collection_folder_name(collection)
}

pub(in crate::egui_app::controller) fn delete_exported_file(
    export_dir: Option<PathBuf>,
    member: &CollectionMember,
) {
    paths::delete_exported_file(export_dir, member);
}

pub(in crate::egui_app::controller) fn collection_folder_name_from_str(name: &str) -> String {
    paths::collection_folder_name_from_str(name)
}

#[cfg(test)]
mod tests;
