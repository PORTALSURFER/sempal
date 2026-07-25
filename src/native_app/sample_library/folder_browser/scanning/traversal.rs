use std::path::{Path, PathBuf};

use super::{
    super::{
        FolderEntry,
        collections::MissingCollectionSnapshot,
        path_helpers::{folder_label, path_id},
        scan_types::{FolderTreeRefreshRequest, FolderTreeRefreshResult},
    },
    entry::{
        BrowserEntryKind, classify_path_without_following, read_sorted_entries,
        source_traversal_policy,
    },
    metadata::{SourceMetadataMap, rated_file_entry, source_rating_map},
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
    FolderTreeRefreshResult {
        source_id: request.source_id,
        label: request.label,
        folder,
        folder_count,
        source_root_available: classify_path_without_following(&request.root)
            == Some(BrowserEntryKind::Directory),
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

pub(super) fn load_folder(
    path: &Path,
    source_root: &Path,
    ratings: &SourceMetadataMap,
    policy: wavecrate_library::sample_sources::SourceTraversalPolicy,
) -> Option<FolderEntry> {
    let entries = read_sorted_entries(path, source_root, policy)?;
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

fn load_folder_tree_only(
    path: &Path,
    source_root: &Path,
    policy: wavecrate_library::sample_sources::SourceTraversalPolicy,
    folder_count: &mut usize,
) -> Option<FolderEntry> {
    let entries = read_sorted_entries(path, source_root, policy)?;
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
