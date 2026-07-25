use std::path::Path;

use super::{
    super::{
        path_helpers::path_id,
        scan_types::{
            FolderVerifyOutcome, FolderVerifyRequest, FolderVerifyResult, FolderVerifySnapshot,
        },
    },
    entry::{
        BrowserEntryKind, classify_path_without_following, read_sorted_entries,
        source_traversal_policy,
    },
    file_entry_metadata::file_entry,
};

pub(in crate::native_app) fn verify_direct_folder(
    request: FolderVerifyRequest,
) -> FolderVerifyResult {
    let policy = source_traversal_policy(&request.source_root, &request.database_root);
    let outcome =
        match read_direct_folder_snapshot(&request.folder_path, &request.source_root, policy) {
            DirectFolderSnapshot::Missing => FolderVerifyOutcome::Missing,
            DirectFolderSnapshot::Unavailable => FolderVerifyOutcome::Unchanged,
            DirectFolderSnapshot::Available(snapshot) => {
                if direct_folder_changed(&request, &snapshot) {
                    FolderVerifyOutcome::Changed(snapshot)
                } else {
                    FolderVerifyOutcome::Unchanged
                }
            }
        };
    FolderVerifyResult {
        source_id: request.source_id,
        folder_path: request.folder_path,
        outcome,
    }
}

enum DirectFolderSnapshot {
    Available(FolderVerifySnapshot),
    Missing,
    Unavailable,
}

fn read_direct_folder_snapshot(
    path: &Path,
    source_root: &Path,
    policy: wavecrate_library::sample_sources::SourceTraversalPolicy,
) -> DirectFolderSnapshot {
    let relative_path = path.strip_prefix(source_root).unwrap_or(path);
    if wavecrate_library::sample_sources::classify_source_entry_with_policy(
        relative_path,
        wavecrate_library::sample_sources::SourceEntryFileType::Directory,
        policy,
    )
    .visible_kind()
        != Some(BrowserEntryKind::Directory)
        || classify_path_without_following(path) != Some(BrowserEntryKind::Directory)
    {
        return DirectFolderSnapshot::Missing;
    }
    let Some(entries) = read_sorted_entries(path, source_root, policy) else {
        return DirectFolderSnapshot::Unavailable;
    };
    let child_paths = entries
        .iter()
        .filter(|entry| entry.kind == BrowserEntryKind::Directory)
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    let files = entries
        .iter()
        .filter(|entry| entry.kind == BrowserEntryKind::File)
        .map(|entry| file_entry(&entry.path))
        .collect::<Vec<_>>();
    DirectFolderSnapshot::Available(FolderVerifySnapshot { child_paths, files })
}

fn direct_folder_changed(request: &FolderVerifyRequest, snapshot: &FolderVerifySnapshot) -> bool {
    let child_ids = snapshot
        .child_paths
        .iter()
        .map(|path| path_id(path))
        .collect::<Vec<_>>();
    if child_ids != request.cached_child_ids {
        return true;
    }
    let file_signatures = snapshot
        .files
        .iter()
        .map(|file| (file.id.clone(), file.size_bytes))
        .collect::<Vec<_>>();
    file_signatures != request.cached_file_signatures
}
