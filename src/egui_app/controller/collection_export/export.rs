use super::fs_ops::{copy_member_to_export, ensure_export_dir};
use super::export_dir_for;
use super::super::*;
use crate::sample_sources::collections::CollectionMember;

impl EguiController {
    pub(in crate::egui_app::controller) fn export_member_if_needed(
        &mut self,
        collection_id: &CollectionId,
        member: &CollectionMember,
    ) -> Result<(), String> {
        let Some(collection) = self
            .library
            .collections
            .iter()
            .find(|c| &c.id == collection_id)
        else {
            return Err("Collection not found".into());
        };
        let collection_dir =
            match export_dir_for(collection, self.settings.collection_export_root.as_deref()) {
                Ok(dir) => dir,
                Err(_) => return Ok(()),
            };
        let source = if let Some(root) = member.clip_root.as_ref() {
            SampleSource {
                id: member.source_id.clone(),
                root: root.clone(),
            }
        } else {
            self.library
                .sources
                .iter()
                .find(|s| s.id == member.source_id)
                .cloned()
                .ok_or_else(|| "Source not available for export".to_string())?
        };
        ensure_export_dir(&collection_dir)?;
        copy_member_to_export(&collection_dir, &source, member)
    }

    pub(in crate::egui_app::controller) fn export_all_members(
        &mut self,
        collection_id: &CollectionId,
    ) -> Result<(), String> {
        let Some(collection) = self
            .library
            .collections
            .iter()
            .find(|c| &c.id == collection_id)
        else {
            return Err("Collection not found".into());
        };
        let members = collection.members.clone();
        for member in members {
            self.export_member_if_needed(collection_id, &member)?;
        }
        Ok(())
    }

    pub(in crate::egui_app::controller) fn sync_collections_from_export_root_path(
        &mut self,
        root: &std::path::Path,
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
                .library
                .collections
                .iter()
                .any(|c| c.export_path.as_ref() == Some(&normalized))
            {
                continue;
            }
            if let Some(existing) = self
                .library
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
            self.library.collections.push(collection);
            created += 1;
            changed = true;
        }
        if changed {
            let _ = self.persist_config("Failed to save collections after export root sync");
            self.refresh_collections_ui();
        }
        Ok(created)
    }

    pub(in crate::egui_app::controller) fn remove_member_from_collection(
        &mut self,
        collection_id: &CollectionId,
        member: &CollectionMember,
    ) -> bool {
        let Some(collection) = self
            .library
            .collections
            .iter_mut()
            .find(|c| &c.id == collection_id)
        else {
            return false;
        };
        let export_dir = super::resolved_export_dir(
            collection,
            self.settings.collection_export_root.as_deref(),
        );
        let removed = collection.remove_member(&member.source_id, &member.relative_path);
        if removed {
            super::delete_exported_file(export_dir, member);
        }
        removed
    }

    pub(in crate::egui_app::controller) fn collection_members(
        &self,
        collection_id: &CollectionId,
    ) -> Vec<CollectionMember> {
        self.library
            .collections
            .iter()
            .find(|c| &c.id == collection_id)
            .map(|c| c.members.clone())
            .unwrap_or_default()
    }
}
