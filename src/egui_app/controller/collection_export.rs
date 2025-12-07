use super::*;
use crate::sample_sources::collections::CollectionMember;
use std::collections::HashSet;

impl EguiController {
    /// Open a folder picker and set the export path for the given collection, exporting existing members immediately.
    pub fn pick_collection_export_path(&mut self, collection_id: &CollectionId) {
        let Some(path) = FileDialog::new().pick_folder() else {
            return;
        };
        let normalized = crate::sample_sources::config::normalize_path(path.as_path());
        match self.set_collection_export_path(collection_id, Some(normalized.clone())) {
            Ok(()) => {
                if let Err(err) = self.export_all_members(collection_id) {
                    self.set_status(err, StatusTone::Error);
                } else {
                    self.set_status(
                        format!("Exports enabled: {}", normalized.display()),
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
        let Some(export_root) = self
            .collections
            .iter()
            .find(|c| &c.id == collection_id)
            .and_then(|c| c.export_path.clone())
        else {
            self.set_status("Set an export folder first", StatusTone::Warning);
            return;
        };
        let result = self.reconcile_collection_export(collection_id, &export_root);
        match result {
            Ok((added, removed)) => {
                let summary = format!(
                    "Refresh export complete: +{added} new, -{removed} missing"
                );
                self.set_status(summary, StatusTone::Info);
            }
            Err(err) => self.set_status(err, StatusTone::Error),
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
        let Some(export_root) = collection.export_path.as_ref() else {
            return Ok(());
        };
        let source = self
            .sources
            .iter()
            .find(|s| s.id == member.source_id)
            .cloned()
            .ok_or_else(|| "Source not available for export".to_string())?;
        if export_root.exists() && !export_root.is_dir() {
            return Err(format!("Export path is not a directory: {}", export_root.display()));
        }
        if !export_root.exists() {
            std::fs::create_dir_all(export_root).map_err(|err| {
                format!("Unable to create export folder {}: {err}", export_root.display())
            })?;
        }
        copy_member_to_export(export_root, &source, member)
    }

    fn set_collection_export_path(
        &mut self,
        collection_id: &CollectionId,
        path: Option<PathBuf>,
    ) -> Result<(), String> {
        let Some(collection) = self.collections.iter_mut().find(|c| &c.id == collection_id) else {
            return Err("Collection not found".into());
        };
        collection.export_path = path;
        self.persist_config("Failed to save collection")?;
        self.refresh_collections_ui();
        Ok(())
    }

    fn export_all_members(&mut self, collection_id: &CollectionId) -> Result<(), String> {
        let Some(collection) = self.collections.iter().find(|c| &c.id == collection_id) else {
            return Err("Collection not found".into());
        };
        if collection.export_path.is_none() {
            return Ok(());
        }
        let members = collection.members.clone();
        for member in members {
            self.export_member_if_needed(collection_id, &member)?;
        }
        Ok(())
    }

    fn reconcile_collection_export(
        &mut self,
        collection_id: &CollectionId,
        export_root: &Path,
    ) -> Result<(usize, usize), String> {
        if !export_root.exists() {
            return Err(format!(
                "Export folder missing: {}",
                export_root.display()
            ));
        }
        if !export_root.is_dir() {
            return Err(format!(
                "Export path is not a directory: {}",
                export_root.display()
            ));
        }
        let files = collect_exported_files(export_root)?;
        let members = self.collection_members(collection_id);
        let member_paths: HashSet<PathBuf> = members
            .iter()
            .map(|m| m.relative_path.clone())
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
        let added = collection.add_member(source.id.clone(), relative_path.to_path_buf());
        added
    }

    fn remove_member_from_collection(
        &mut self,
        collection_id: &CollectionId,
        member: &CollectionMember,
    ) -> bool {
        let Some(collection) = self.collections.iter_mut().find(|c| &c.id == collection_id) else {
            return false;
        };
        let export_root = collection.export_path.clone();
        let removed = collection.remove_member(&member.source_id, &member.relative_path);
        if removed {
            delete_exported_file(export_root, member);
        }
        removed
    }

    fn resolve_source_for_relative_path(&self, relative_path: &Path) -> Option<SampleSource> {
        self.sources.iter().find_map(|source| {
            let candidate = source.root.join(relative_path);
            candidate.is_file().then(|| source.clone())
        })
    }

    fn collection_members(&self, collection_id: &CollectionId) -> Vec<CollectionMember> {
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
            if file_set.contains(&member.relative_path) {
                seen.insert(member.relative_path.clone());
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
    let dest = export_root.join(&member.relative_path);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!("Failed to create export folder {}: {err}", parent.display())
        })?;
    }
    std::fs::copy(&source_path, &dest)
        .map_err(|err| format!("Failed to export {}: {err}", dest.display()))?;
    Ok(())
}

pub(super) fn delete_exported_file(export_root: Option<PathBuf>, member: &CollectionMember) {
    let Some(root) = export_root else {
        return;
    };
    let target = root.join(&member.relative_path);
    let _ = std::fs::remove_file(target);
}

fn collect_exported_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|err| format!("Unable to read export folder {}: {err}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|err| format!("Unable to read export entry: {err}"))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .map_err(|err| format!("Path error in export folder: {err}"))?
                    .to_path_buf();
                files.push(rel);
            }
        }
    }
    Ok(files)
}
