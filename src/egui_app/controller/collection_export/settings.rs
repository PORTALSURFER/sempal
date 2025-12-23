use super::super::*;
use super::fs_ops::ensure_export_dir;
use super::reconcile::reconcile_collection_export;
use super::{export_dir_for, resolved_export_dir};
use std::path::PathBuf;

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
        let Some(path) = self.settings.collection_export_root.clone() else {
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
                        .library
                        .collections
                        .iter()
                        .find(|c| &c.id == collection_id)
                        .and_then(|c| {
                            export_dir_for(c, self.settings.collection_export_root.as_deref()).ok()
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
    pub fn sync_collection_export(&mut self, collection_id: &CollectionId) {
        let Some(collection) = self
            .library
            .collections
            .iter()
            .find(|c| &c.id == collection_id)
        else {
            self.set_status("Collection not found", StatusTone::Error);
            return;
        };
        if resolved_export_dir(collection, self.settings.collection_export_root.as_deref())
            .is_none()
        {
            self.set_status("Set an export folder first", StatusTone::Warning);
            return;
        };
        let result = reconcile_collection_export(self, collection_id);
        match result {
            Ok((added, removed)) => {
                let summary = format!("Sync export complete: +{added} new, -{removed} missing");
                self.set_status(summary, StatusTone::Info);
            }
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }

    /// Open the collection's export folder in the system file explorer.
    pub fn open_collection_export_folder(&mut self, collection_id: &CollectionId) {
        let Some(collection) = self
            .library
            .collections
            .iter()
            .find(|c| &c.id == collection_id)
        else {
            self.set_status("Collection not found", StatusTone::Error);
            return;
        };
        let Ok(dir) = export_dir_for(collection, self.settings.collection_export_root.as_deref())
        else {
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

    pub(in crate::egui_app::controller::collection_export) fn set_collection_export_path(
        &mut self,
        collection_id: &CollectionId,
        path: Option<PathBuf>,
    ) -> Result<(), String> {
        let Some(collection) = self
            .library
            .collections
            .iter_mut()
            .find(|c| &c.id == collection_id)
        else {
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

    pub(in crate::egui_app::controller::collection_export) fn set_collection_export_root(
        &mut self,
        path: Option<PathBuf>,
    ) -> Result<(), String> {
        self.settings.collection_export_root = path.clone();
        self.ui.collection_export_root = path.clone();
        if let Some(root) = path.as_deref() {
            let _ = self.sync_collections_from_export_root_path(root);
        }
        self.persist_config("Failed to save collection export root")
    }
}
