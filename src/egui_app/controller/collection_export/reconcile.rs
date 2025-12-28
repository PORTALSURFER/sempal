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
    let file_names: HashSet<PathBuf> = files
        .iter()
        .filter_map(|path| path.file_name().map(PathBuf::from))
        .collect();
    let members = controller.collection_members(collection_id);
    let member_paths: HashSet<PathBuf> = members
        .iter()
        .filter_map(|m| m.relative_path.file_name().map(PathBuf::from))
        .collect();
    let (seen, removed) = remove_missing_exports(controller, collection_id, &members, &file_names);
    let added =
        add_new_exports(controller, collection_id, &collection_dir, &files, &member_paths, &seen)?;
    controller.persist_config("Failed to save collection")?;
    controller.refresh_collections_ui();
    Ok((added, removed))
}

fn remove_missing_exports(
    controller: &mut EguiController,
    collection_id: &CollectionId,
    members: &[CollectionMember],
    file_names: &HashSet<PathBuf>,
) -> (HashSet<PathBuf>, usize) {
    let mut seen = HashSet::new();
    let mut removed = 0;
    for member in members {
        let name = match member.relative_path.file_name() {
            Some(name) => PathBuf::from(name),
            None => continue,
        };
        if file_names.contains(&name) {
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
    collection_dir: &Path,
    files: &[PathBuf],
    member_paths: &HashSet<PathBuf>,
    seen: &HashSet<PathBuf>,
) -> Result<usize, String> {
    let mut added = 0;
    let clip_source_id = SourceId::from_string(format!("collection-{}", collection_id.as_str()));
    let clip_source = SampleSource {
        id: clip_source_id.clone(),
        root: collection_dir.to_path_buf(),
    };
    for rel_path in files {
        let Some(file_name) = rel_path.file_name().map(PathBuf::from) else {
            continue;
        };
        if seen.contains(&file_name) || member_paths.contains(&file_name) {
            continue;
        }
        if collection_contains_member(controller, collection_id, &clip_source_id, rel_path) {
            continue;
        }
        controller.ensure_sample_db_entry(&clip_source, rel_path)?;
        if add_clip_member_from_export(
            controller,
            collection_id,
            &clip_source_id,
            rel_path,
            collection_dir,
        ) {
            added += 1;
        }
    }
    Ok(added)
}

fn add_clip_member_from_export(
    controller: &mut EguiController,
    collection_id: &CollectionId,
    source_id: &SourceId,
    relative_path: &Path,
    clip_root: &Path,
) -> bool {
    let Some(collection) = controller
        .library
        .collections
        .iter_mut()
        .find(|c| &c.id == collection_id)
    else {
        return false;
    };
    if collection.contains(source_id, &relative_path.to_path_buf()) {
        return false;
    }
    collection.members.push(CollectionMember {
        source_id: source_id.clone(),
        relative_path: relative_path.to_path_buf(),
        clip_root: Some(clip_root.to_path_buf()),
    });
    true
}

fn collection_contains_member(
    controller: &EguiController,
    collection_id: &CollectionId,
    source_id: &SourceId,
    relative_path: &Path,
) -> bool {
    controller
        .library
        .collections
        .iter()
        .find(|c| &c.id == collection_id)
        .is_some_and(|collection| collection.contains(source_id, &relative_path.to_path_buf()))
}
