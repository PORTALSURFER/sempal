use super::super::*;
use super::export_dir_for;
use super::fs_ops::collect_exported_files;
use crate::sample_sources::collections::CollectionMember;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(super) fn reconcile_collection_export(
    controller: &mut EguiController,
    collection_id: &CollectionId,
) -> Result<(usize, usize), String> {
    let Some(collection) = controller
        .library
        .collections
        .iter()
        .find(|c| &c.id == collection_id)
    else {
        return Err("Collection not found".into());
    };
    let collection_dir = export_dir_for(
        collection,
        controller.settings.collection_export_root.as_deref(),
    )?;
    if !collection_dir.exists() {
        return Err(format!(
            "Export folder missing: {}",
            collection_dir.display()
        ));
    }
    if !collection_dir.is_dir() {
        return Err(format!(
            "Export path is not a directory: {}",
            collection_dir.display()
        ));
    }
    let files = collect_exported_files(&collection_dir)?;
    let members = controller.collection_members(collection_id);
    let member_paths: HashSet<PathBuf> = members
        .iter()
        .filter_map(|m| m.relative_path.file_name().map(PathBuf::from))
        .collect();
    let (seen, removed) = remove_missing_exports(controller, collection_id, &members, &files);
    let added = add_new_exports(controller, collection_id, &files, &member_paths, &seen)?;
    controller.persist_config("Failed to save collection")?;
    controller.refresh_collections_ui();
    Ok((added, removed))
}

fn remove_missing_exports(
    controller: &mut EguiController,
    collection_id: &CollectionId,
    members: &[CollectionMember],
    files: &[PathBuf],
) -> (HashSet<PathBuf>, usize) {
    let mut seen = HashSet::new();
    let mut removed = 0;
    let file_set: HashSet<PathBuf> = files.iter().cloned().collect();
    for member in members {
        let name = match member.relative_path.file_name() {
            Some(name) => PathBuf::from(name),
            None => continue,
        };
        if file_set.contains(&name) {
            seen.insert(name);
            continue;
        }
        if controller.remove_member_from_collection(collection_id, member) {
            removed += 1;
        }
    }
    (seen, removed)
}

fn add_new_exports(
    controller: &mut EguiController,
    collection_id: &CollectionId,
    files: &[PathBuf],
    member_paths: &HashSet<PathBuf>,
    seen: &HashSet<PathBuf>,
) -> Result<usize, String> {
    let mut added = 0;
    for rel_path in files {
        if seen.contains(rel_path) || member_paths.contains(rel_path) {
            continue;
        }
        if let Some(source) = resolve_source_for_relative_path(controller, rel_path) {
            controller.ensure_sample_db_entry(&source, rel_path)?;
            if add_member_from_refresh(controller, collection_id, &source, rel_path) {
                added += 1;
            }
        }
    }
    Ok(added)
}

fn add_member_from_refresh(
    controller: &mut EguiController,
    collection_id: &CollectionId,
    source: &SampleSource,
    relative_path: &Path,
) -> bool {
    let Some(collection) = controller
        .library
        .collections
        .iter_mut()
        .find(|c| &c.id == collection_id)
    else {
        return false;
    };
    collection.add_member(source.id.clone(), relative_path.to_path_buf())
}

fn resolve_source_for_relative_path(
    controller: &EguiController,
    relative_path: &Path,
) -> Option<SampleSource> {
    controller.library.sources.iter().find_map(|source| {
        let candidate = source.root.join(relative_path);
        candidate.is_file().then(|| source.clone())
    })
}
