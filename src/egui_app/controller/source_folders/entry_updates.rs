use super::*;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

impl EguiController {
    pub(super) fn folder_entries(&self, folder: &Path) -> Vec<WavEntry> {
        self.wav_entries
            .entries
            .iter()
            .filter(|entry| entry.relative_path.starts_with(folder))
            .cloned()
            .collect()
    }

    pub(super) fn rewrite_entries_for_folder(
        &mut self,
        source: &SampleSource,
        old_folder: &Path,
        new_folder: &Path,
        entries: &[WavEntry],
    ) -> Result<(), String> {
        if entries.is_empty() {
            return Ok(());
        }
        self.update_folder_db_entries(source, old_folder, new_folder, entries)?;
        self.update_folder_caches(source, old_folder, new_folder, entries)
    }

    fn update_folder_db_entries(
        &mut self,
        source: &SampleSource,
        old_folder: &Path,
        new_folder: &Path,
        entries: &[WavEntry],
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        let mut batch = db
            .write_batch()
            .map_err(|err| format!("Failed to start database update: {err}"))?;
        for entry in entries {
            let suffix = entry
                .relative_path
                .strip_prefix(old_folder)
                .unwrap_or_else(|_| Path::new(""));
            let updated_path = new_folder.join(suffix);
            batch
                .remove_file(&entry.relative_path)
                .map_err(|err| format!("Failed to drop old entry: {err}"))?;
            batch
                .upsert_file(&updated_path, entry.file_size, entry.modified_ns)
                .map_err(|err| format!("Failed to register renamed file: {err}"))?;
            batch
                .set_tag(&updated_path, entry.tag)
                .map_err(|err| format!("Failed to copy tag: {err}"))?;
        }
        batch
            .commit()
            .map_err(|err| format!("Failed to save rename: {err}"))
    }

    fn update_folder_caches(
        &mut self,
        source: &SampleSource,
        old_folder: &Path,
        new_folder: &Path,
        entries: &[WavEntry],
    ) -> Result<(), String> {
        let mut collections_changed = false;
        let mut updates: Vec<(WavEntry, WavEntry)> = Vec::with_capacity(entries.len());
        for entry in entries {
            let suffix = entry
                .relative_path
                .strip_prefix(old_folder)
                .unwrap_or_else(|_| Path::new(""));
            let updated_path = new_folder.join(suffix);
            let mut new_entry = entry.clone();
            new_entry.relative_path = updated_path.clone();
            new_entry.missing = false;
            updates.push((entry.clone(), new_entry));
            if self.update_collections_for_rename(&source.id, &entry.relative_path, &updated_path) {
                collections_changed = true;
            }
        }
        self.apply_folder_entry_updates(source, &updates);
        if collections_changed {
            self.persist_config("Failed to save collection after folder rename")?;
        }
        Ok(())
    }

    fn apply_folder_entry_updates(&mut self, source: &SampleSource, updates: &[(WavEntry, WavEntry)]) {
        if updates.is_empty() {
            return;
        }
        if let Some(cache) = self.cache.wav.entries.get_mut(&source.id) {
            apply_entry_updates(cache, updates);
            self.rebuild_wav_cache_lookup(&source.id);
        }
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
            apply_entry_updates(&mut self.wav_entries.entries, updates);
            for (old_entry, new_entry) in updates {
                self.update_selection_paths(
                    source,
                    &old_entry.relative_path,
                    &new_entry.relative_path,
                );
            }
            self.invalidate_cached_audio_for_entry_updates(&source.id, updates);
            self.sync_browser_after_wav_entries_mutation_keep_search_cache(&source.id);
        } else {
            self.browser_cache.labels.remove(&source.id);
        }
        self.rebuild_missing_lookup_for_source(&source.id);
    }

    pub(super) fn update_manual_folders<F>(&mut self, mut update: F)
    where
        F: FnMut(&mut BTreeSet<PathBuf>),
    {
        let Some(model) = self.current_folder_model_mut() else {
            return;
        };
        update(&mut model.manual_folders);
    }

    pub(super) fn remap_manual_folders(&mut self, old: &Path, new: &Path) {
        self.update_manual_folders(|set| {
            let descendants: Vec<PathBuf> = set
                .iter()
                .filter(|path| path.starts_with(old))
                .cloned()
                .collect();
            set.retain(|path| !path.starts_with(old));
            for path in descendants {
                let suffix = path.strip_prefix(old).unwrap_or_else(|_| Path::new(""));
                set.insert(new.join(suffix));
            }
        });
    }
}

fn apply_entry_updates(list: &mut Vec<WavEntry>, updates: &[(WavEntry, WavEntry)]) {
    if updates.is_empty() {
        return;
    }
    let mut index_map: HashMap<PathBuf, usize> = list
        .iter()
        .enumerate()
        .map(|(idx, entry)| (entry.relative_path.clone(), idx))
        .collect();
    for (old_entry, new_entry) in updates {
        if let Some(idx) = index_map.remove(&old_entry.relative_path) {
            list[idx] = new_entry.clone();
            index_map.insert(new_entry.relative_path.clone(), idx);
        } else {
            list.push(new_entry.clone());
            index_map.insert(new_entry.relative_path.clone(), list.len() - 1);
        }
    }
    list.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
}
