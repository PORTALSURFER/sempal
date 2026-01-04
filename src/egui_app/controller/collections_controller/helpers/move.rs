use super::super::*;
use super::members::BrowserSampleContext;
use super::CollectionsController;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use crate::sample_sources::SourceDatabase;

impl CollectionsController<'_> {
    pub(in crate::egui_app::controller::collections_controller) fn primary_visible_row_for_browser_selection(
        &mut self,
    ) -> Option<usize> {
        let selected_index = self.selected_row_index()?;
        let path = self
            .wav_entry(selected_index)
            .map(|entry| entry.relative_path.clone())?;
        self.visible_row_for_path(&path)
    }

    pub(in crate::egui_app::controller::collections_controller) fn browser_selection_rows_for_move(
        &mut self,
    ) -> Vec<usize> {
        let mut rows: Vec<usize> = self
            .ui
            .browser
            .selected_paths
            .clone()
            .iter()
            .filter_map(|path| self.visible_row_for_path(path))
            .collect();
        if rows.is_empty() {
            if let Some(row) = self
                .focused_browser_row()
                .or_else(|| self.primary_visible_row_for_browser_selection())
            {
                rows.push(row);
            }
        }
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    pub(in crate::egui_app::controller::collections_controller) fn next_browser_focus_path_after_move(
        &mut self,
        rows: &[usize],
    ) -> Option<PathBuf> {
        if rows.is_empty() || self.ui.browser.visible.len() == 0 {
            return None;
        }
        let mut sorted = rows.to_vec();
        sorted.sort_unstable();
        let highest = sorted.last().copied()?;
        let first = sorted.first().copied().unwrap_or(highest);
        let after = highest
            .checked_add(1)
            .and_then(|idx| self.ui.browser.visible.get(idx))
            .and_then(|entry_idx| self.wav_entry(entry_idx))
            .map(|entry| entry.relative_path.clone());
        if after.is_some() {
            return after;
        }
        first
            .checked_sub(1)
            .and_then(|idx| self.ui.browser.visible.get(idx))
            .and_then(|entry_idx| self.wav_entry(entry_idx))
            .map(|entry| entry.relative_path.clone())
    }

    pub(in crate::egui_app::controller::collections_controller) fn move_browser_rows_to_collection(
        &mut self,
        collection_id: &CollectionId,
        rows: &[usize],
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
        let (contexts, mut last_error) = self.collect_browser_contexts(rows);
        let total_contexts = contexts.len();
        let move_requests: Vec<CollectionMoveRequest> = contexts
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

    pub(in crate::egui_app::controller) fn move_sample_to_collection(
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
        if let Err(err) = self.add_clip_to_collection(
            collection_id,
            clip_root.clone(),
            clip_relative.clone(),
        ) {
            let _ = fs::rename(&clip_absolute, &absolute);
            return Err(err);
        }
        self.remove_source_sample(source, relative_path)?;
        self.rebuild_browser_lists();
        Ok(collection_name)
    }

    pub(in crate::egui_app::controller::collections_controller) fn apply_collection_move_result(
        &mut self,
        result: crate::egui_app::controller::jobs::CollectionMoveResult,
    ) {
        let collection_name = self
            .library
            .collections
            .iter()
            .find(|collection| collection.id == result.collection_id)
            .map(|collection| collection.name.clone())
            .unwrap_or_else(|| "collection".to_string());
        let total_moved = result.moved.len();
        let mut moved = 0usize;
        let mut last_error = result.errors.last().cloned();
        for entry in result.moved {
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
            if let Err(err) = self.add_clip_to_collection(
                &result.collection_id,
                entry.clip_root.clone(),
                entry.clip_relative.clone(),
            ) {
                let _ = fs::rename(
                    entry.clip_root.join(&entry.clip_relative),
                    source.root.join(&entry.relative_path),
                );
                last_error = Some(err);
                continue;
            }
            if let Err(err) = self.remove_source_sample(&source, &entry.relative_path) {
                last_error = Some(err);
                continue;
            }
            moved += 1;
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

    pub(super) fn collect_browser_contexts(
        &mut self,
        rows: &[usize],
    ) -> (Vec<BrowserSampleContext>, Option<String>) {
        let mut contexts = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut last_error = None;
        for row in rows {
            match self.resolve_browser_sample(*row) {
                Ok(ctx) => {
                    if seen.insert(ctx.entry.relative_path.clone()) {
                        contexts.push(BrowserSampleContext {
                            source: ctx.source,
                            entry: ctx.entry,
                        });
                    }
                }
                Err(err) => last_error = Some(err),
            }
        }
        (contexts, last_error)
    }

    fn resolve_collection_clip_root(&self, collection_id: &CollectionId) -> Result<PathBuf, String> {
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

fn unique_destination_name(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| "Sample has no file name".to_string())?;
    let candidate = PathBuf::from(file_name);
    if !root.join(&candidate).exists() {
        return Ok(candidate);
    }
    let stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "sample".to_string());
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_string());
    for index in 1..=999 {
        let suffix = format!("{stem}_move{index:03}");
        let file_name = if let Some(ext) = &extension {
            format!("{suffix}.{ext}")
        } else {
            suffix
        };
        let candidate = PathBuf::from(file_name);
        if !root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err("Failed to find destination file name".into())
}

fn move_sample_file(source: &Path, destination: &Path) -> Result<(), String> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(rename_err) => {
            if let Err(copy_err) = fs::copy(source, destination) {
                return Err(format!(
                    "Failed to move file: {rename_err}; copy failed: {copy_err}"
                ));
            }
            if let Err(remove_err) = fs::remove_file(source) {
                let _ = fs::remove_file(destination);
                return Err(format!("Failed to remove original file: {remove_err}"));
            }
            Ok(())
        }
    }
}

struct CollectionMoveRequest {
    source_id: SourceId,
    source_root: PathBuf,
    relative_path: PathBuf,
}

fn run_collection_move_task(
    collection_id: CollectionId,
    clip_root: PathBuf,
    requests: Vec<CollectionMoveRequest>,
) -> crate::egui_app::controller::jobs::CollectionMoveResult {
    let mut moved = Vec::new();
    let mut errors = Vec::new();
    for request in requests {
        let absolute = request.source_root.join(&request.relative_path);
        if !absolute.is_file() {
            errors.push(format!(
                "File missing: {}",
                request.relative_path.display()
            ));
            continue;
        }
        let clip_relative = match unique_destination_name(&clip_root, &request.relative_path) {
            Ok(path) => path,
            Err(err) => {
                errors.push(err);
                continue;
            }
        };
        let clip_absolute = clip_root.join(&clip_relative);
        if let Err(err) = move_sample_file(&absolute, &clip_absolute) {
            errors.push(err);
            continue;
        }
        moved.push(crate::egui_app::controller::jobs::CollectionMoveSuccess {
            source_id: request.source_id,
            relative_path: request.relative_path,
            clip_root: clip_root.clone(),
            clip_relative,
        });
    }
    crate::egui_app::controller::jobs::CollectionMoveResult {
        collection_id,
        moved,
        errors,
    }
}
