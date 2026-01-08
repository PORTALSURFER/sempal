use super::super::*;
use super::CollectionsController;
use super::fs::{
    CollectionMoveRequest, move_sample_file, run_collection_move_task, unique_destination_name,
};
use super::move_plan::MovePlan;
use crate::sample_sources::SourceDatabase;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

impl CollectionsController<'_> {
    pub(crate) fn primary_visible_row_for_browser_selection(
        &mut self,
    ) -> Option<usize> {
        let selected_index = self.selected_row_index()?;
        let path = self
            .wav_entry(selected_index)
            .map(|entry| entry.relative_path.clone())?;
        self.visible_row_for_path(&path)
    }

    pub(crate) fn move_browser_rows_to_collection(
        &mut self,
        collection_id: &CollectionId,
        plan: MovePlan,
    ) {
        if !self.settings.feature_flags.collections_enabled {
            self.set_status("Collections are disabled", StatusTone::Warning);
            return;
        }
        if self.runtime.jobs.collection_move_in_progress() {
            self.set_status("Collection move already in progress", StatusTone::Warning);
            return;
        }
        let clip_root = match self.resolve_collection_clip_root(collection_id) {
            Ok(root) => root,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        let Some(collection_index) = self
            .library
            .collections
            .iter()
            .position(|collection| &collection.id == collection_id)
        else {
            self.set_status("Collection not found", StatusTone::Error);
            return;
        };
        let collection_name = self.library.collections[collection_index].name.clone();
        let mut last_error = plan.last_error;
        let total_contexts = plan.contexts.len();
        let move_requests: Vec<CollectionMoveRequest> = plan
            .contexts
            .into_iter()
            .map(|ctx| CollectionMoveRequest {
                source_id: ctx.source.id,
                source_root: ctx.source.root,
                relative_path: ctx.entry.relative_path,
            })
            .collect();
        if move_requests.is_empty() {
            if let Some(err) = last_error.take() {
                self.set_status(err, StatusTone::Error);
            }
            return;
        }
        let collection_id = collection_id.clone();
        let clip_root = clip_root.clone();
        let (tx, rx) = mpsc::channel();
        self.runtime.jobs.start_collection_move(rx);
        std::thread::spawn(move || {
            let result = run_collection_move_task(collection_id, clip_root, move_requests);
            let _ = tx.send(result);
        });
        if let Some(err) = last_error.take() {
            self.set_status(err, StatusTone::Warning);
        } else {
            self.set_status(
                format!("Moving {total_contexts} sample(s) to '{collection_name}'..."),
                StatusTone::Busy,
            );
        }
    }

    pub(crate) fn move_sample_to_collection(
        &mut self,
        collection_id: &CollectionId,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<String, String> {
        if !self.settings.feature_flags.collections_enabled {
            return Err("Collections are disabled".into());
        }
        let clip_root = self.resolve_collection_clip_root(collection_id)?;
        let collection_name = self
            .library
            .collections
            .iter()
            .find(|collection| &collection.id == collection_id)
            .map(|collection| collection.name.clone())
            .ok_or_else(|| "Collection not found".to_string())?;
        let absolute = source.root.join(relative_path);
        if !absolute.is_file() {
            return Err(format!("File missing: {}", relative_path.display()));
        }
        let clip_relative = unique_destination_name(&clip_root, relative_path)?;
        let clip_absolute = clip_root.join(&clip_relative);
        move_sample_file(&absolute, &clip_absolute)?;
        if let Err(err) =
            self.add_clip_to_collection(collection_id, clip_root.clone(), clip_relative.clone())
        {
            let _ = fs::rename(&clip_absolute, &absolute);
            return Err(err);
        }
        self.remove_source_sample(source, relative_path)?;
        self.rebuild_browser_lists();
        Ok(collection_name)
    }

    pub(crate) fn apply_collection_move_result(
        &mut self,
        result: crate::egui_app::controller::jobs::CollectionMoveResult,
    ) {
        let Some(collection_index) = self
            .library
            .collections
            .iter()
            .position(|collection| collection.id == result.collection_id)
        else {
            self.set_status("Collection not found", StatusTone::Error);
            return;
        };
        let collection_name = self.library.collections[collection_index].name.clone();
        let total_moved = result.moved.len();
        let mut moved = 0usize;
        let mut last_error = result.errors.last().cloned();
        let mut added_members = Vec::new();
        let mut affected_sources = std::collections::HashMap::new();
        let mut moved_by_source: std::collections::HashMap<SourceId, Vec<PathBuf>> =
            std::collections::HashMap::new();
        let mut collections_changed = false;
        let clip_source_id =
            SourceId::from_string(format!("collection-{}", result.collection_id.as_str()));
        let mut collection_removals = Vec::new();
        for entry in &result.moved {
            let Some(source) = self
                .library
                .sources
                .iter()
                .find(|source| source.id == entry.source_id)
                .cloned()
            else {
                last_error = Some("Source not available for move".to_string());
                continue;
            };
            let new_member = CollectionMember {
                source_id: clip_source_id.clone(),
                relative_path: entry.clip_relative.clone(),
                clip_root: Some(entry.clip_root.clone()),
            };
            let already_present = self.library.collections[collection_index]
                .contains(&new_member.source_id, &new_member.relative_path);
            if !already_present {
                self.library.collections[collection_index]
                    .members
                    .push(new_member.clone());
                added_members.push(new_member);
            }
            collection_removals.push((entry.source_id.clone(), entry.relative_path.clone()));
            moved_by_source
                .entry(entry.source_id.clone())
                .or_default()
                .push(entry.relative_path.clone());
            self.clear_loaded_sample_if(&source, &entry.relative_path);
            affected_sources.entry(source.id.clone()).or_insert(source);
            moved += 1;
        }
        for (source_id, relative_path) in collection_removals {
            if self.remove_sample_from_collections(&source_id, &relative_path) {
                collections_changed = true;
            }
        }
        if collections_changed {
            let _ = self.persist_config("Failed to save collection after move");
        }
        if !added_members.is_empty() {
            if let Err(err) = self.persist_config("Failed to save collection") {
                self.set_status(err, StatusTone::Error);
                return;
            }
            self.refresh_collections_ui();
            for member in &added_members {
                if let Err(err) = self.export_member_if_needed(&result.collection_id, member) {
                    self.set_status(err, StatusTone::Warning);
                }
            }
        }
        if !moved_by_source.is_empty() {
            let mut cleanup_error = None;
            for (source_id, relative_paths) in moved_by_source {
                let Some(source) = affected_sources.get(&source_id) else {
                    continue;
                };
                let db = match self.database_for(source) {
                    Ok(db) => db,
                    Err(err) => match SourceDatabase::open(&source.root) {
                        Ok(db) => std::rc::Rc::new(db),
                        Err(open_err) => {
                            cleanup_error =
                                Some(format!("Failed to open source database: {open_err}"));
                            let _ = err;
                            continue;
                        }
                    },
                };
                for relative_path in relative_paths {
                    if source.root.join(&relative_path).is_file() {
                        continue;
                    }
                    if let Err(err) = db.remove_file(&relative_path) {
                        cleanup_error = Some(format!("Failed to remove moved entry: {err}"));
                    }
                }
            }
            if last_error.is_none() {
                last_error = cleanup_error;
            }
        }
        for source in affected_sources.values() {
            self.invalidate_wav_entries_for_source_preserve_folders(source);
        }
        if moved > 0 {
            let failed = result
                .errors
                .len()
                .saturating_add(total_moved.saturating_sub(moved));
            if failed > 0 {
                let suffix = last_error
                    .as_deref()
                    .map(|err| format!(" {failed} failed: {err}"))
                    .unwrap_or_else(|| format!(" {failed} failed."));
                self.set_status(
                    format!("Moved {moved} sample(s) to '{collection_name}'.{suffix}"),
                    StatusTone::Warning,
                );
            } else {
                self.set_status(
                    format!("Moved {moved} sample(s) to '{collection_name}'"),
                    StatusTone::Info,
                );
            }
            self.rebuild_browser_lists();
        } else if let Some(err) = last_error {
            self.set_status(err, StatusTone::Error);
        }
    }

    fn resolve_collection_clip_root(
        &self,
        collection_id: &CollectionId,
    ) -> Result<PathBuf, String> {
        let preferred = self
            .library
            .collections
            .iter()
            .find(|collection| &collection.id == collection_id)
            .and_then(|collection| {
                collection_export::resolved_export_dir(
                    collection,
                    self.settings.collection_export_root.as_deref(),
                )
            });
        if let Some(path) = preferred {
            if path.exists() && !path.is_dir() {
                return Err(format!(
                    "Collection export path is not a directory: {}",
                    path.display()
                ));
            }
            std::fs::create_dir_all(&path).map_err(|err| {
                format!(
                    "Failed to create collection export path {}: {err}",
                    path.display()
                )
            })?;
            return Ok(path);
        }
        let fallback = crate::app_dirs::app_root_dir()
            .map_err(|err| err.to_string())?
            .join("collection_clips")
            .join(collection_id.as_str());
        std::fs::create_dir_all(&fallback)
            .map_err(|err| format!("Failed to create collection clip folder: {err}"))?;
        Ok(fallback)
    }

    fn remove_source_sample(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        let mut removal_error = None;
        match self.database_for(source) {
            Ok(db) => {
                if let Err(err) = db.remove_file(relative_path) {
                    removal_error = Some(format!("Failed to drop database row: {err}"));
                }
            }
            Err(err) => {
                removal_error = Some(format!("Database unavailable: {err}"));
            }
        }
        if let Some(primary_error) = removal_error {
            let _ = primary_error;
            SourceDatabase::open(&source.root)
                .and_then(|db| db.remove_file(relative_path))
                .map_err(|err| format!("Fallback database removal failed: {err}"))?;
        }
        self.prune_cached_sample(source, relative_path);
        let collections_changed = self.remove_sample_from_collections(&source.id, relative_path);
        if collections_changed {
            self.persist_config("Failed to save collection after move")?;
        }
        Ok(())
    }
}
