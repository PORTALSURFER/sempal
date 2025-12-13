use super::*;
use crate::sample_sources::collections::CollectionMember;
use std::collections::HashSet;

impl EguiController {
    /// Pick a global export root used when collections inherit automatic folders.
    pub fn pick_collection_export_root(&mut self) {
        let Some(path) = FileDialog::new().pick_folder() else {
            return;
        };
        let normalized = crate::sample_sources::config::normalize_path(path.as_path());
        match self.set_collection_export_root(Some(normalized.clone())) {
            Ok(()) => self.set_status(
                format!("Collection export root set to {}", normalized.display()),
                StatusTone::Info,
            ),
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }

    /// Clear the global collection export root without touching disk contents.
    pub fn clear_collection_export_root(&mut self) {
        match self.set_collection_export_root(None) {
            Ok(()) => self.set_status("Cleared collection export root", StatusTone::Info),
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }

    /// Open the global export root in the system file explorer.
    pub fn open_collection_export_root(&mut self) {
        let Some(path) = self.collection_export_root.clone() else {
            self.set_status("Set a collection export root first", StatusTone::Warning);
            return;
        };
        if let Err(err) = ensure_export_dir(&path) {
            self.set_status(err, StatusTone::Error);
            return;
        }
        if let Err(err) = open::that(&path) {
            self.set_status(
                format!("Could not open folder {}: {err}", path.display()),
                StatusTone::Error,
            );
        }
    }

    /// Open a folder picker and set the export path for the given collection, exporting existing members immediately.
    pub fn pick_collection_export_path(&mut self, collection_id: &CollectionId) {
        let Some(path) = FileDialog::new().pick_folder() else {
            return;
        };
        let normalized = crate::sample_sources::config::normalize_path(path.as_path());
        match self.set_collection_export_path(collection_id, Some(path)) {
            Ok(()) => {
                if let Err(err) = self.export_all_members(collection_id) {
                    self.set_status(err, StatusTone::Error);
                } else {
                    let display = self
                        .collections
                        .iter()
                        .find(|c| &c.id == collection_id)
                        .and_then(|c| {
                            export_dir_for(c, self.collection_export_root.as_deref()).ok()
                        })
                        .unwrap_or(normalized);
                    self.set_status(
                        format!("Exports enabled: {}", display.display()),
                        StatusTone::Info,
                    );
                }
            }
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }

    /// Remove the export path from a collection without touching existing files on disk.
    pub fn clear_collection_export_path(&mut self, collection_id: &CollectionId) {
        match self.set_collection_export_path(collection_id, None) {
            Ok(()) => self.set_status("Cleared export path", StatusTone::Info),
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }

    /// Reconcile a collection with the current contents of its export folder.
    pub fn refresh_collection_export(&mut self, collection_id: &CollectionId) {
        let Some(collection) = self.collections.iter().find(|c| &c.id == collection_id) else {
            self.set_status("Collection not found", StatusTone::Error);
            return;
        };
        if resolved_export_dir(collection, self.collection_export_root.as_deref()).is_none() {
            self.set_status("Set an export folder first", StatusTone::Warning);
            return;
        };
        let result = self.reconcile_collection_export(collection_id);
        match result {
            Ok((added, removed)) => {
                let summary = format!("Refresh export complete: +{added} new, -{removed} missing");
                self.set_status(summary, StatusTone::Info);
            }
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }

    /// Open the collection's export folder in the system file explorer.
    pub fn open_collection_export_folder(&mut self, collection_id: &CollectionId) {
        let Some(collection) = self.collections.iter().find(|c| &c.id == collection_id) else {
            self.set_status("Collection not found", StatusTone::Error);
            return;
        };
        let Ok(dir) = export_dir_for(collection, self.collection_export_root.as_deref()) else {
            self.set_status("Set an export folder first", StatusTone::Warning);
            return;
        };
        if let Err(err) = ensure_export_dir(&dir) {
            self.set_status(err, StatusTone::Error);
            return;
        }
        if let Err(err) = open::that(&dir) {
            self.set_status(
                format!("Could not open folder {}: {err}", dir.display()),
                StatusTone::Error,
            );
        }
    }

    pub(super) fn export_member_if_needed(
        &mut self,
        collection_id: &CollectionId,
        member: &CollectionMember,
    ) -> Result<(), String> {
        let Some(collection) = self.collections.iter().find(|c| &c.id == collection_id) else {
            return Err("Collection not found".into());
        };
        let collection_dir =
            match export_dir_for(collection, self.collection_export_root.as_deref()) {
                Ok(dir) => dir,
                Err(_) => return Ok(()),
            };
        let source = if let Some(root) = member.clip_root.as_ref() {
            SampleSource {
                id: member.source_id.clone(),
                root: root.clone(),
            }
        } else {
            self.sources
                .iter()
                .find(|s| s.id == member.source_id)
                .cloned()
                .ok_or_else(|| "Source not available for export".to_string())?
        };
        ensure_export_dir(&collection_dir)?;
        copy_member_to_export(&collection_dir, &source, member)
    }

    fn set_collection_export_path(
        &mut self,
        collection_id: &CollectionId,
        path: Option<PathBuf>,
    ) -> Result<(), String> {
        let Some(collection) = self.collections.iter_mut().find(|c| &c.id == collection_id) else {
            return Err("Collection not found".into());
        };
        if let Some(path) = path {
            let folder_label = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_string());
            let normalized = crate::sample_sources::config::normalize_path(path.as_path());
            collection.export_path = Some(normalized);
            if let Some(label) = folder_label {
                collection.name = label;
            }
        } else {
            collection.export_path = None;
        }
        self.persist_config("Failed to save collection")?;
        self.refresh_collections_ui();
        Ok(())
    }

    fn set_collection_export_root(&mut self, path: Option<PathBuf>) -> Result<(), String> {
        self.collection_export_root = path.clone();
        self.ui.collection_export_root = path.clone();
        if let Some(root) = path.as_deref() {
            let _ = self.sync_collections_from_export_root_path(root);
        }
        self.persist_config("Failed to save collection export root")
    }

    pub(crate) fn sync_collections_from_export_root_path(
        &mut self,
        root: &Path,
    ) -> Result<usize, String> {
        if !root.exists() {
            return Ok(0);
        }
        if !root.is_dir() {
            return Err(format!(
                "Collection export root is not a directory: {}",
                root.display()
            ));
        }
        let mut created = 0usize;
        let mut changed = false;
        let entries = std::fs::read_dir(root)
            .map_err(|err| format!("Failed to read export root {}: {err}", root.display()))?;
        for entry in entries {
            let entry =
                entry.map_err(|err| format!("Failed to read export root entry: {err}"))?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(folder_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if folder_name.starts_with('.') {
                continue;
            }
            let normalized = crate::sample_sources::config::normalize_path(path.as_path());
            if self
                .collections
                .iter()
                .any(|c| c.export_path.as_ref() == Some(&normalized))
            {
                continue;
            }
            if let Some(existing) = self
                .collections
                .iter_mut()
                .find(|c| c.export_path.is_none() && c.name == folder_name)
            {
                existing.export_path = Some(normalized);
                changed = true;
                continue;
            }
            let mut collection = Collection::new(folder_name);
            collection.export_path = Some(normalized);
            self.collections.push(collection);
            created += 1;
            changed = true;
        }
        if changed {
            let _ = self.persist_config("Failed to save collections after export root sync");
            self.refresh_collections_ui();
        }
        Ok(created)
    }

    fn export_all_members(&mut self, collection_id: &CollectionId) -> Result<(), String> {
        let Some(collection) = self.collections.iter().find(|c| &c.id == collection_id) else {
            return Err("Collection not found".into());
        };
        let members = collection.members.clone();
        for member in members {
            self.export_member_if_needed(collection_id, &member)?;
        }
        Ok(())
    }

    fn reconcile_collection_export(
        &mut self,
        collection_id: &CollectionId,
    ) -> Result<(usize, usize), String> {
        let Some(collection) = self.collections.iter().find(|c| &c.id == collection_id) else {
            return Err("Collection not found".into());
        };
        let collection_dir = export_dir_for(collection, self.collection_export_root.as_deref())?;
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
        let members = self.collection_members(collection_id);
        let member_paths: HashSet<PathBuf> = members
            .iter()
            .filter_map(|m| m.relative_path.file_name().map(PathBuf::from))
            .collect();
        let (seen, removed) = self.remove_missing_exports(collection_id, &members, &files);
        let added = self.add_new_exports(collection_id, &files, &member_paths, &seen)?;
        self.persist_config("Failed to save collection")?;
        self.refresh_collections_ui();
        Ok((added, removed))
    }

    fn add_member_from_refresh(
        &mut self,
        collection_id: &CollectionId,
        source: &SampleSource,
        relative_path: &Path,
    ) -> bool {
        let Some(collection) = self.collections.iter_mut().find(|c| &c.id == collection_id) else {
            return false;
        };
        collection.add_member(source.id.clone(), relative_path.to_path_buf())
    }

    fn remove_member_from_collection(
        &mut self,
        collection_id: &CollectionId,
        member: &CollectionMember,
    ) -> bool {
        let Some(collection) = self.collections.iter_mut().find(|c| &c.id == collection_id) else {
            return false;
        };
        let export_dir = resolved_export_dir(collection, self.collection_export_root.as_deref());
        let removed = collection.remove_member(&member.source_id, &member.relative_path);
        if removed {
            delete_exported_file(export_dir, member);
        }
        removed
    }

    fn resolve_source_for_relative_path(&self, relative_path: &Path) -> Option<SampleSource> {
        self.sources.iter().find_map(|source| {
            let candidate = source.root.join(relative_path);
            candidate.is_file().then(|| source.clone())
        })
    }

    pub(super) fn collection_members(&self, collection_id: &CollectionId) -> Vec<CollectionMember> {
        self.collections
            .iter()
            .find(|c| &c.id == collection_id)
            .map(|c| c.members.clone())
            .unwrap_or_default()
    }

    fn remove_missing_exports(
        &mut self,
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
            if self.remove_member_from_collection(collection_id, member) {
                removed += 1;
            }
        }
        (seen, removed)
    }

    fn add_new_exports(
        &mut self,
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
            if let Some(source) = self.resolve_source_for_relative_path(rel_path) {
                self.ensure_sample_db_entry(&source, rel_path)?;
                if self.add_member_from_refresh(collection_id, &source, rel_path) {
                    added += 1;
                }
            }
        }
        Ok(added)
    }
}

fn copy_member_to_export(
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

fn collect_exported_files(root: &Path) -> Result<Vec<PathBuf>, String> {
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
            } else if path.is_file()
                && let Some(name) = path.file_name()
                && seen.insert(name.to_os_string())
            {
                files.push(PathBuf::from(name));
            }
        }
    }
    Ok(files)
}

fn ensure_export_dir(path: &Path) -> Result<(), String> {
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

pub(super) fn resolved_export_dir(
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

pub(super) fn export_dir_for(
    collection: &Collection,
    global_root: Option<&Path>,
) -> Result<PathBuf, String> {
    resolved_export_dir(collection, global_root).ok_or_else(|| "Set an export folder first".into())
}

pub(super) fn collection_folder_name(collection: &Collection) -> String {
    collection.export_folder_name()
}

pub(super) fn delete_exported_file(export_dir: Option<PathBuf>, member: &CollectionMember) {
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

pub(super) fn collection_folder_name_from_str(name: &str) -> String {
    crate::sample_sources::collections::collection_folder_name_from_str(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_dirs::ConfigBaseGuard;
    use crate::sample_sources::Collection;
    use tempfile::tempdir;

    #[test]
    fn manual_export_path_updates_name_and_path() {
        let renderer = crate::waveform::WaveformRenderer::new(4, 4);
        let mut controller = EguiController::new(renderer, None);
        let collection = Collection::new("Original");
        let id = collection.id.clone();
        controller.collections.push(collection);
        let temp = tempdir().unwrap();
        let manual_dir = temp.path().join("Manual Name");
        controller
            .set_collection_export_path(&id, Some(manual_dir.clone()))
            .unwrap();
        let stored = controller
            .collections
            .iter()
            .find(|c| c.id == id)
            .expect("collection present");
        assert_eq!(stored.name, "Manual Name");
        assert_eq!(stored.export_path.as_ref(), Some(&manual_dir));
    }

    #[test]
    fn resolved_export_dir_prefers_manual_override() {
        let renderer = crate::waveform::WaveformRenderer::new(4, 4);
        let mut controller = EguiController::new(renderer, None);
        let mut collection = Collection::new("Manual");
        collection.export_path = Some(PathBuf::from("custom/manual"));
        controller.collections.push(collection);
        let dir = resolved_export_dir(&controller.collections[0], Some(Path::new("global/root")))
            .expect("dir");
        assert_eq!(dir, PathBuf::from("custom/manual"));
    }

    #[test]
    fn resolved_export_dir_uses_global_root_when_missing_override() {
        let renderer = crate::waveform::WaveformRenderer::new(4, 4);
        let mut controller = EguiController::new(renderer, None);
        controller.collection_export_root = Some(PathBuf::from("global"));
        let collection = Collection::new("Global Collection");
        controller.collections.push(collection);
        let dir = resolved_export_dir(
            &controller.collections[0],
            controller.collection_export_root.as_deref(),
        )
        .expect("dir");
        assert_eq!(dir, PathBuf::from("global").join("Global Collection"));
    }

    #[test]
    fn setting_export_root_syncs_direct_subfolders_to_collections() {
        let temp = tempdir().unwrap();
        let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
        let export_root = temp.path().join("export_root");
        std::fs::create_dir_all(export_root.join("A")).unwrap();
        std::fs::create_dir_all(export_root.join("B")).unwrap();
        std::fs::create_dir_all(export_root.join(".hidden")).unwrap();
        std::fs::write(export_root.join("not_a_dir.txt"), b"x").unwrap();

        let renderer = crate::waveform::WaveformRenderer::new(4, 4);
        let mut controller = EguiController::new(renderer, None);

        let normalized_root = crate::sample_sources::config::normalize_path(export_root.as_path());
        controller
            .set_collection_export_root(Some(normalized_root.clone()))
            .unwrap();

        assert_eq!(controller.collections.len(), 2);
        assert!(controller.collections.iter().any(|c| c.name == "A"));
        assert!(controller.collections.iter().any(|c| c.name == "B"));

        let expected_a =
            crate::sample_sources::config::normalize_path(export_root.join("A").as_path());
        let expected_b =
            crate::sample_sources::config::normalize_path(export_root.join("B").as_path());
        assert!(controller
            .collections
            .iter()
            .any(|c| c.name == "A" && c.export_path.as_ref() == Some(&expected_a)));
        assert!(controller
            .collections
            .iter()
            .any(|c| c.name == "B" && c.export_path.as_ref() == Some(&expected_b)));
    }

    #[test]
    fn sync_updates_existing_collection_export_path_by_name() {
        let temp = tempdir().unwrap();
        let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
        let export_root = temp.path().join("export_root");
        std::fs::create_dir_all(export_root.join("Existing")).unwrap();

        let renderer = crate::waveform::WaveformRenderer::new(4, 4);
        let mut controller = EguiController::new(renderer, None);
        controller.collections.push(Collection::new("Existing"));

        let created = controller
            .sync_collections_from_export_root_path(export_root.as_path())
            .unwrap();
        assert_eq!(created, 0);
        assert_eq!(controller.collections.len(), 1);

        let expected =
            crate::sample_sources::config::normalize_path(export_root.join("Existing").as_path());
        assert_eq!(controller.collections[0].export_path.as_ref(), Some(&expected));
    }
}
