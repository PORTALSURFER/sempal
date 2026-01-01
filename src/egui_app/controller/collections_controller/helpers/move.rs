use super::super::*;
use super::members::BrowserSampleContext;
use super::CollectionsController;
use std::fs;
use std::path::{Path, PathBuf};

impl CollectionsController<'_> {
    pub(super) fn primary_visible_row_for_browser_selection(&mut self) -> Option<usize> {
        let selected_index = self.selected_row_index()?;
        let path = self
            .wav_entry(selected_index)
            .map(|entry| entry.relative_path.clone())?;
        self.visible_row_for_path(&path)
    }

    pub(super) fn browser_selection_rows_for_move(&mut self) -> Vec<usize> {
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

    pub(super) fn next_browser_focus_path_after_move(
        &mut self,
        rows: &[usize],
    ) -> Option<PathBuf> {
        if rows.is_empty() || self.ui.browser.visible.is_empty() {
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

    pub(super) fn move_browser_rows_to_collection(
        &mut self,
        collection_id: &CollectionId,
        rows: &[usize],
    ) {
        if !self.settings.feature_flags.collections_enabled {
            self.set_status("Collections are disabled", StatusTone::Warning);
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
        let mut moved = 0usize;
        for ctx in contexts {
            let source = ctx.source.clone();
            let relative_path = ctx.entry.relative_path.clone();
            let absolute = source.root.join(&relative_path);
            if !absolute.is_file() {
                last_error = Some(format!("File missing: {}", relative_path.display()));
                continue;
            }
            let clip_relative = match unique_destination_name(&clip_root, &relative_path) {
                Ok(path) => path,
                Err(err) => {
                    last_error = Some(err);
                    continue;
                }
            };
            let clip_absolute = clip_root.join(&clip_relative);
            if let Err(err) = move_sample_file(&absolute, &clip_absolute) {
                last_error = Some(err);
                continue;
            }
            if let Err(err) = self.add_clip_to_collection(
                collection_id,
                clip_root.clone(),
                clip_relative.clone(),
            ) {
                let _ = fs::rename(&clip_absolute, &absolute);
                last_error = Some(err);
                continue;
            }
            if let Err(err) = self.remove_source_sample(&source, &relative_path) {
                last_error = Some(err);
                continue;
            }
            moved += 1;
        }
        if moved > 0 {
            self.set_status(
                format!("Moved {moved} sample(s) to '{collection_name}'"),
                StatusTone::Info,
            );
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
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.remove_file(relative_path)
            .map_err(|err| format!("Failed to drop database row: {err}"))?;
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
