use super::super::{
    FileEntry, FolderEntry,
    scan_types::{FolderScanDiscovery, FolderScanItem},
};

pub(in crate::native_app::sample_library::folder_browser) fn merge_scan_discovery(
    root: &mut FolderEntry,
    event: &FolderScanDiscovery,
) -> bool {
    let Some(parent) = root.find_mut(&event.parent_id) else {
        return false;
    };
    match &event.item {
        FolderScanItem::ResetFolder => {
            let changed = !parent.children.is_empty() || !parent.files.is_empty();
            parent.children.clear();
            parent.files.clear();
            changed
        }
        FolderScanItem::Folder(folder) => {
            insert_discovered_folder(&mut parent.children, folder.clone())
        }
        FolderScanItem::File(file) => upsert_file(&mut parent.files, file.clone()),
    }
}

fn insert_discovered_folder(folders: &mut Vec<FolderEntry>, folder: FolderEntry) -> bool {
    match folders.binary_search_by(|candidate| {
        candidate
            .name
            .to_ascii_lowercase()
            .cmp(&folder.name.to_ascii_lowercase())
    }) {
        Ok(_) => false,
        Err(index) => {
            folders.insert(index, folder);
            true
        }
    }
}

pub(in crate::native_app::sample_library::folder_browser) fn upsert_folder(
    folders: &mut Vec<FolderEntry>,
    folder: FolderEntry,
) -> bool {
    match folders.binary_search_by(|candidate| {
        candidate
            .name
            .to_ascii_lowercase()
            .cmp(&folder.name.to_ascii_lowercase())
    }) {
        Ok(index) if folders[index] == folder => false,
        Ok(index) => {
            folders[index] = folder;
            true
        }
        Err(index) => {
            folders.insert(index, folder);
            true
        }
    }
}

pub(in crate::native_app::sample_library::folder_browser) fn upsert_file(
    files: &mut Vec<FileEntry>,
    file: FileEntry,
) -> bool {
    match files.binary_search_by(|candidate| {
        candidate
            .name
            .to_ascii_lowercase()
            .cmp(&file.name.to_ascii_lowercase())
    }) {
        Ok(index) if files[index] == file => false,
        Ok(index) => {
            files[index] = file;
            true
        }
        Err(index) => {
            files.insert(index, file);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use wavecrate::sample_sources::Rating;

    #[test]
    fn folder_snapshot_start_prunes_stale_cached_children() {
        let mut root = FolderEntry {
            id: String::from("root"),
            name: String::from("root"),
            children: vec![FolderEntry {
                id: String::from("root/stale"),
                name: String::from("stale"),
                children: Vec::new(),
                files: Vec::new(),
            }],
            files: vec![FileEntry::missing_collection_member(
                Path::new("root/stale.wav"),
                Rating::NEUTRAL,
                false,
                Vec::new(),
                None,
                None,
            )],
        };
        let event = FolderScanDiscovery {
            task_id: 1,
            source_id: String::from("source"),
            committed_revision: None,
            parent_id: String::from("root"),
            item: FolderScanItem::ResetFolder,
        };

        assert!(merge_scan_discovery(&mut root, &event));
        assert!(root.children.is_empty());
        assert!(root.files.is_empty());
    }
}
