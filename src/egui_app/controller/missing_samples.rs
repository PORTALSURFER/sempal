use super::*;
use std::collections::HashSet;
use std::path::Path;

impl EguiController {
    pub(in crate::egui_app::controller) fn sync_missing_from_db(&mut self, source_id: &SourceId) {
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|source| &source.id == source_id)
            .cloned()
        else {
            return;
        };
        let db = match self.database_for(&source) {
            Ok(db) => db,
            Err(_) => return,
        };
        if let Ok(paths) = db.list_missing_paths() {
            self.library
                .missing
                .wavs
                .insert(source_id.clone(), paths.into_iter().collect());
        }
    }

    pub(in crate::egui_app::controller) fn rebuild_missing_lookup_for_source(
        &mut self,
        source_id: &SourceId,
    ) {
        let mut missing = HashSet::new();
        if let Some(cache) = self.cache.wav.entries.get(source_id) {
            for page in cache.pages.values() {
                for entry in page {
                    if entry.missing {
                        missing.insert(entry.relative_path.clone());
                    }
                }
            }
        } else if self.selection_state.ctx.selected_source.as_ref() == Some(source_id) {
            for page in self.wav_entries.pages.values() {
                for entry in page {
                    if entry.missing {
                        missing.insert(entry.relative_path.clone());
                    }
                }
            }
        }
        self.library.missing.wavs.insert(source_id.clone(), missing);
    }

    pub(in crate::egui_app::controller) fn mark_sample_missing(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) {
        match self.database_for(source) {
            Ok(db) => {
                let _ = db.set_missing(relative_path, true);
            }
            Err(SourceDbError::InvalidRoot(_)) => {
                self.mark_source_missing(&source.id, "Source folder missing");
            }
            Err(err) => {
                self.set_status(
                    format!("Failed to update missing flag: {err}"),
                    StatusTone::Warning,
                );
            }
        }
        if let Some(cache) = self.cache.wav.entries.get_mut(&source.id)
            && let Some(index) = cache.lookup.get(relative_path).copied()
            && let Some(entry) = cache.entry_mut(index)
        {
            entry.missing = true;
        }
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id)
            && let Some(index) = self.wav_index_for_path(relative_path)
            && let Some(entry) = self.wav_entries.entry_mut(index)
        {
            entry.missing = true;
        }
        self.library
            .missing
            .wavs
            .entry(source.id.clone())
            .or_default()
            .insert(relative_path.to_path_buf());
        self.invalidate_cached_audio(&source.id, relative_path);
    }

    fn ensure_missing_lookup_for_source(&mut self, source: &SampleSource) -> Result<(), String> {
        if self.library.missing.wavs.contains_key(&source.id) {
            return Ok(());
        }
        if self.library.missing.sources.contains(&source.id) {
            self.library
                .missing
                .wavs
                .entry(source.id.clone())
                .or_default();
            return Ok(());
        }
        let db = match self.database_for(source) {
            Ok(db) => db,
            Err(err) => {
                if matches!(err, SourceDbError::InvalidRoot(_)) {
                    self.mark_source_missing(&source.id, "Source folder missing");
                }
                return Err(err.to_string());
            }
        };
        let paths = db
            .list_missing_paths()
            .map_err(|err| format!("Failed to read missing files: {err}"))?;
        self.library
            .missing
            .wavs
            .insert(source.id.clone(), paths.into_iter().collect());
        Ok(())
    }

    pub(in crate::egui_app::controller) fn sample_missing(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
    ) -> bool {
        if self.library.missing.sources.contains(source_id) {
            return true;
        }
        if self.selection_state.ctx.selected_source.as_ref() == Some(source_id)
            && let Some(index) = self.wav_index_for_path(relative_path)
            && let Some(entry) = self.wav_entries.entry(index)
        {
            return entry.missing;
        }
        if let Some(cache) = self.cache.wav.entries.get(source_id) {
            if let Some(index) = cache.lookup.get(relative_path).copied()
                && let Some(entry) = cache.entry(index)
            {
                return entry.missing;
            }
        }
        if let Some(set) = self.library.missing.wavs.get(source_id) {
            return set.contains(relative_path);
        }
        if let Some(source) = self
            .library
            .sources
            .iter()
            .find(|s| &s.id == source_id)
            .cloned()
        {
            if let Err(err) = self.ensure_missing_lookup_for_source(&source) {
                self.set_status(err, StatusTone::Warning);
                return true;
            }
            if let Some(set) = self.library.missing.wavs.get(source_id) {
                return set.contains(relative_path);
            }
        }
        false
    }

    pub(in crate::egui_app::controller) fn show_missing_waveform_notice(
        &mut self,
        relative_path: &Path,
    ) {
        let message = format!("File missing: {}", relative_path.display());
        self.clear_waveform_view();
        self.ui.waveform.notice = Some(message);
    }

    pub(in crate::egui_app::controller) fn remove_dead_links_for_source_entries(
        &mut self,
        source: &SampleSource,
    ) -> Result<usize, String> {
        if self.library.missing.sources.contains(&source.id) {
            return Err("Source folder missing; remap source before removing dead links".into());
        }
        self.ensure_missing_lookup_for_source(source)?;
        let missing_paths: Vec<PathBuf> = self
            .library
            .missing
            .wavs
            .get(&source.id)
            .map(|paths| paths.iter().cloned().collect())
            .unwrap_or_default();
        if missing_paths.is_empty() {
            return Ok(0);
        }
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        let mut collections_changed = false;
        for path in &missing_paths {
            db.remove_file(path)
                .map_err(|err| format!("Failed to drop database row: {err}"))?;
            self.prune_cached_sample(source, path);
            if self.remove_sample_from_collections(&source.id, path) {
                collections_changed = true;
            }
        }
        if collections_changed {
            self.persist_config("Failed to save collection after removing dead links")?;
        }
        Ok(missing_paths.len())
    }
}
