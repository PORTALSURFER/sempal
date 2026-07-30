use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use cap_fs_ext::{DirExt, ambient_authority};
use cap_std::fs::Dir;

use wavecrate_library::sample_sources::BrowserFileMetadata;

use super::{
    super::{
        FolderEntry,
        collections::MissingCollectionSnapshot,
        path_helpers::{folder_label, path_id},
        scan_types::{FolderTreeRefreshRequest, FolderTreeRefreshResult},
    },
    entry::{
        BrowserEntryKind, classify_path_without_following, read_sorted_entries,
        read_sorted_entries_nofollow,
        source_traversal_policy,
    },
    file_entry_metadata::{file_entry, file_entry_with_snapshot_metadata},
    metadata::{SourceMetadataMap, rated_file_entry, source_rating_map, source_rating_snapshot},
};

pub(in crate::native_app::sample_library::folder_browser) struct LoadedSourceSnapshot {
    pub(in crate::native_app::sample_library::folder_browser) folder: FolderEntry,
    pub(in crate::native_app::sample_library::folder_browser) missing_collection_snapshot:
        MissingCollectionSnapshot,
}

pub(in crate::native_app::sample_library::folder_browser) fn load_source_snapshot(
    root: PathBuf,
    database_root: PathBuf,
) -> LoadedSourceSnapshot {
    let ratings = source_rating_map(&root, &database_root)
        .map(|(ratings, _)| ratings)
        .unwrap_or_else(|error| {
            tracing::warn!(source = %root.display(), "{error}");
            SourceMetadataMap::new()
        });
    let policy = source_traversal_policy(&root, &database_root);
    let folder =
        load_folder(&root, &root, &ratings, policy).unwrap_or_else(|| placeholder_folder(&root));
    let missing_collection_snapshot =
        MissingCollectionSnapshot::from_source_metadata(&root, &folder, &ratings);
    LoadedSourceSnapshot {
        folder,
        missing_collection_snapshot,
    }
}

pub(in crate::native_app::sample_library::folder_browser) fn placeholder_folder(
    root: &Path,
) -> FolderEntry {
    FolderEntry {
        id: path_id(root),
        name: folder_label(root),
        children: Vec::new(),
        files: Vec::new(),
    }
}

pub(in crate::native_app) fn refresh_folder_tree_only(
    request: FolderTreeRefreshRequest,
) -> FolderTreeRefreshResult {
    let policy = source_traversal_policy(&request.root, &request.database_root);
    let mut folder_count = 0;
    let folder = load_folder_tree_only(&request.root, &request.root, policy, &mut folder_count)
        .unwrap_or_else(|| placeholder_folder(&request.root));
    let rating_hydration = match source_rating_snapshot(&request.root, &request.database_root) {
        Ok(snapshot) => super::super::scan_types::RatingHydrationStatus::Complete { snapshot },
        Err(error) => {
            tracing::warn!(source = %request.root.display(), %error, "Folder tree rating hydration failed");
            super::super::scan_types::RatingHydrationStatus::Failed {
                error: error.to_string(),
            }
        }
    };
    FolderTreeRefreshResult {
        source_id: request.source_id,
        label: request.label,
        folder,
        folder_count,
        source_root_available: classify_path_without_following(&request.root)
            == Some(BrowserEntryKind::Directory),
        rating_hydration,
    }
}

pub(in crate::native_app::sample_library::folder_browser) fn load_folder_at_path(
    path: &Path,
    source_root: &Path,
    source_database_root: &Path,
) -> Option<FolderEntry> {
    let ratings = source_rating_map(source_root, source_database_root)
        .map(|(ratings, _)| ratings)
        .unwrap_or_else(|error| {
            tracing::warn!(source = %source_root.display(), "{error}");
            SourceMetadataMap::new()
        });
    let policy = source_traversal_policy(source_root, source_database_root);
    load_folder(path, source_root, &ratings, policy)
}

/// Build a no-follow folder subtree from a worker-owned committed metadata
/// snapshot. The UI completion path receives the resulting value and never
/// reopens the filesystem or source database.
pub(in crate::native_app) fn load_folder_at_path_with_browser_metadata(
    path: &Path,
    source_root: &Path,
    metadata: &HashMap<PathBuf, BrowserFileMetadata>,
    policy: wavecrate::sample_sources::SourceTraversalPolicy,
    cancel: &AtomicBool,
) -> Option<FolderEntry> {
    let mut directory = Dir::open_ambient_dir(source_root, ambient_authority()).ok()?;
    for component in path.strip_prefix(source_root).ok()?.components() {
        let std::path::Component::Normal(component) = component else {
            return None;
        };
        directory = directory.open_dir_nofollow(component).ok()?;
    }
    load_folder_with_browser_metadata(
        &directory,
        path,
        source_root,
        metadata,
        policy,
        cancel,
    )
}

pub(super) fn load_folder(
    path: &Path,
    source_root: &Path,
    ratings: &SourceMetadataMap,
    policy: wavecrate_library::sample_sources::SourceTraversalPolicy,
) -> Option<FolderEntry> {
    let entries = read_sorted_entries(path, source_root, policy, None)?;
    let children = entries
        .iter()
        .filter(|entry| entry.kind == BrowserEntryKind::Directory)
        .filter_map(|entry| load_folder(&entry.path, source_root, ratings, policy))
        .collect::<Vec<_>>();
    let files = entries
        .iter()
        .filter(|entry| entry.kind == BrowserEntryKind::File)
        .map(|entry| rated_file_entry(&entry.path, source_root, ratings))
        .collect::<Vec<_>>();
    Some(FolderEntry {
        id: path_id(path),
        name: folder_label(path),
        children,
        files,
    })
}

fn load_folder_with_browser_metadata(
    directory: &Dir,
    path: &Path,
    source_root: &Path,
    metadata: &HashMap<PathBuf, BrowserFileMetadata>,
    policy: wavecrate::sample_sources::SourceTraversalPolicy,
    cancel: &AtomicBool,
) -> Option<FolderEntry> {
    if cancel.load(Ordering::Acquire) {
        return None;
    }
    let entries = read_sorted_entries_nofollow(directory, path, source_root, policy, cancel)?;
    let mut children = Vec::new();
    for entry in entries
        .iter()
        .filter(|entry| entry.kind == BrowserEntryKind::Directory)
    {
        if cancel.load(Ordering::Acquire) {
            return None;
        }
        let Some(name) = entry.path.file_name() else {
            return None;
        };
        let Ok(child_directory) = directory.open_dir_nofollow(name) else {
            continue;
        };
        let Some(child) = load_folder_with_browser_metadata(
            &child_directory,
            &entry.path,
            source_root,
            metadata,
            policy,
            cancel,
        ) else {
            if cancel.load(Ordering::Acquire) {
                return None;
            }
            continue;
        };
        children.push(child);
    }
    let mut files = Vec::new();
    for entry in entries
        .iter()
        .filter(|entry| entry.kind == BrowserEntryKind::File)
    {
        if cancel.load(Ordering::Acquire) {
            return None;
        }
        let file = entry
            .path
            .strip_prefix(source_root)
            .ok()
            .and_then(|relative| metadata.get(relative))
            .map(|metadata| {
                file_entry_with_snapshot_metadata(
                    &entry.path,
                    metadata.file_size,
                    metadata.rating,
                    metadata.locked,
                    metadata.collections.clone(),
                    metadata.last_played_at,
                    metadata.last_curated_at,
                )
            })
            .unwrap_or_else(|| file_entry(&entry.path));
        files.push(file);
    }
    if cancel.load(Ordering::Acquire) {
        return None;
    }
    Some(FolderEntry {
        id: path_id(path),
        name: folder_label(path),
        children,
        files,
    })
}

fn load_folder_tree_only(
    path: &Path,
    source_root: &Path,
    policy: wavecrate_library::sample_sources::SourceTraversalPolicy,
    folder_count: &mut usize,
) -> Option<FolderEntry> {
    let entries = read_sorted_entries(path, source_root, policy, None)?;
    *folder_count += 1;
    let children = entries
        .iter()
        .filter(|entry| entry.kind == BrowserEntryKind::Directory)
        .filter_map(|entry| load_folder_tree_only(&entry.path, source_root, policy, folder_count))
        .collect::<Vec<_>>();
    Some(FolderEntry {
        id: path_id(path),
        name: folder_label(path),
        children,
        files: Vec::new(),
    })
}
