use super::collection_export;
use super::*;
use std::fs;

impl EguiController {
    /// Select the first available source or refresh the current one.
    pub fn select_first_source(&mut self) {
        if self.selection_ctx.selected_source.is_none() {
            if let Some(first) = self.sources.first().cloned() {
                self.select_source(Some(first.id));
            } else {
                self.clear_wavs();
            }
        } else {
            let _ = self.refresh_wavs();
        }
    }

    /// Change the selected source by index.
    pub fn select_source_by_index(&mut self, index: usize) {
        let id = self.sources.get(index).map(|s| s.id.clone());
        self.select_source(id);
    }

    /// Move source selection up or down by an offset.
    pub fn nudge_source_selection(&mut self, offset: isize) {
        if self.sources.is_empty() {
            return;
        }
        let current = self.ui.sources.selected.unwrap_or(0) as isize;
        let target = (current + offset).clamp(0, self.sources.len() as isize - 1) as usize;
        self.select_source_by_index(target);
        self.focus_sources_context();
    }

    /// Change the selected source by id and refresh dependent state.
    pub fn select_source(&mut self, id: Option<SourceId>) {
        self.select_source_internal(id, None);
    }

    /// Refresh the wav list for the selected source (delegates to background load).
    pub fn refresh_wavs(&mut self) -> Result<(), SourceDbError> {
        // Maintained for compatibility; now delegates to background load.
        self.queue_wav_load();
        Ok(())
    }

    /// Add a new source folder via file picker.
    pub fn add_source_via_dialog(&mut self) {
        let Some(path) = FileDialog::new().pick_folder() else {
            return;
        };
        if let Err(error) = self.add_source_from_path(path) {
            self.set_status(error, StatusTone::Error);
        }
    }

    /// Add a new source folder from a known path.
    pub fn add_source_from_path(&mut self, path: PathBuf) -> Result<(), String> {
        let normalized = crate::sample_sources::config::normalize_path(path.as_path());
        if !normalized.is_dir() {
            return Err("Please select a directory".into());
        }
        if self.sources.iter().any(|s| s.root == normalized) {
            self.set_status("Source already added", StatusTone::Info);
            return Ok(());
        }
        let source = SampleSource::new(normalized.clone());
        SourceDatabase::open(&normalized)
            .map_err(|err| format!("Failed to create database: {err}"))?;
        let _ = self.cache_db(&source);
        self.sources.push(source.clone());
        self.select_source(Some(source.id.clone()));
        self.persist_config("Failed to save config after adding source")?;
        Ok(())
    }

    /// Remove a configured source by index.
    pub fn remove_source(&mut self, index: usize) {
        if index >= self.sources.len() {
            return;
        }
        let removed = self.sources.remove(index);
        self.missing.sources.remove(&removed.id);
        let mut invalidator = source_cache_invalidator::SourceCacheInvalidator::new(
            &mut self.db_cache,
            &mut self.wav_cache.entries,
            &mut self.wav_cache.lookup,
            &mut self.browser_cache.labels,
            &mut self.missing.wavs,
            &mut self.folder_browsers.models,
        );
        invalidator.invalidate_all(&removed.id);
        for collection in self.collections.iter_mut() {
            let export_dir = collection_export::resolved_export_dir(
                collection,
                self.settings.collection_export_root.as_deref(),
            );
            let removed_members = collection.prune_source(&removed.id);
            for member in removed_members {
                collection_export::delete_exported_file(export_dir.clone(), &member);
            }
        }
        if self
            .selection_ctx
            .selected_source
            .as_ref()
            .is_some_and(|id| id == &removed.id)
        {
            self.selection_ctx.selected_source = None;
            self.wav_selection.selected_wav = None;
            self.clear_waveform_view();
        }
        let _ = self.persist_config("Failed to save config after removing source");
        self.refresh_sources_ui();
        let _ = self.refresh_wavs();
        self.refresh_collections_ui();
        self.select_first_source();
        self.set_status("Source removed", StatusTone::Info);
    }

    pub(super) fn refresh_sources_ui(&mut self) {
        self.ui.sources.rows = self
            .sources
            .iter()
            .map(|source| {
                let missing = self.missing.sources.contains(&source.id);
                view_model::source_row(source, missing)
            })
            .collect();
        self.ui.sources.menu_row = None;
        self.ui.sources.selected = self
            .selection_ctx
            .selected_source
            .as_ref()
            .and_then(|id| self.sources.iter().position(|s| &s.id == id));
        self.ui.sources.scroll_to = self.ui.sources.selected;
    }

    pub(crate) fn current_source(&self) -> Option<SampleSource> {
        let selected = self.selection_ctx.selected_source.as_ref()?;
        self.sources.iter().find(|s| &s.id == selected).cloned()
    }

    pub(super) fn rebuild_missing_sources(&mut self) {
        self.missing.sources.clear();
        for source in &self.sources {
            if !source.root.is_dir() {
                self.missing.sources.insert(source.id.clone());
                self.missing.wavs.entry(source.id.clone()).or_default();
            }
        }
    }

    pub(super) fn mark_source_missing(&mut self, source_id: &SourceId, reason: &str) {
        let inserted = self.missing.sources.insert(source_id.clone());
        if inserted && self.selection_ctx.selected_source.as_ref() == Some(source_id) {
            self.clear_waveform_view();
        }
        self.missing.wavs.entry(source_id.clone()).or_default();
        self.refresh_sources_ui();
        if let Some(source) = self.sources.iter().find(|s| &s.id == source_id) {
            self.set_status(
                format!("{reason}: {}", source.root.display()),
                StatusTone::Warning,
            );
        } else {
            self.set_status(reason, StatusTone::Warning);
        }
    }

    pub(super) fn clear_source_missing(&mut self, source_id: &SourceId) {
        let removed = self.missing.sources.remove(source_id);
        self.missing.wavs.remove(source_id);
        if removed {
            self.refresh_sources_ui();
        }
    }

    pub(super) fn select_source_internal(
        &mut self,
        id: Option<SourceId>,
        pending_path: Option<PathBuf>,
    ) {
        let same_source = self.selection_ctx.selected_source == id;
        self.jobs.pending_select_path = pending_path.clone();
            if same_source {
                self.refresh_sources_ui();
                if let Some(path) = self.jobs.pending_select_path.clone() {
                if self.wav_entries.lookup.contains_key(&path) {
                    self.jobs.pending_select_path = None;
                    self.select_wav_by_path(&path);
                } else {
                    self.queue_wav_load();
                }
            }
            return;
        }
        if pending_path.is_none() {
            self.ui.collections.selected_sample = None;
        }
        if let Some(ref source_id) = id
            && self.sources.iter().any(|s| &s.id == source_id)
        {
            self.selection_ctx.last_selected_browsable_source = Some(source_id.clone());
        }
        self.selection_ctx.selected_source = id;
        self.wav_selection.selected_wav = None;
        self.clear_waveform_view();
        self.refresh_sources_ui();
        self.queue_wav_load();
        let _ = self.persist_config("Failed to save selection");
        // Do not auto-scan; only run when explicitly requested.
    }

    fn clear_wavs(&mut self) {
        self.wav_entries.entries.clear();
        self.wav_entries.lookup.clear();
        self.wav_selection.selected_wav = None;
        self.ui.browser = SampleBrowserState::default();
        self.ui.sources.folders = FolderBrowserUiState::default();
        self.clear_waveform_view();
        if let Some(selected) = self.selection_ctx.selected_source.as_ref() {
            self.missing.wavs.remove(selected);
        } else {
            self.missing.wavs.clear();
        }
    }

    pub(super) fn database_for(
        &mut self,
        source: &SampleSource,
    ) -> Result<Rc<SourceDatabase>, SourceDbError> {
        if let Some(existing) = self.db_cache.get(&source.id) {
            return Ok(existing.clone());
        }
        let db = Rc::new(SourceDatabase::open(&source.root)?);
        self.db_cache.insert(source.id.clone(), db.clone());
        Ok(db)
    }

    pub(super) fn cache_db(
        &mut self,
        source: &SampleSource,
    ) -> Result<Rc<SourceDatabase>, SourceDbError> {
        self.database_for(source)
    }

    /// Remap a source root via folder picker.
    pub fn remap_source_via_dialog(&mut self, index: usize) {
        let Some(path) = FileDialog::new().pick_folder() else {
            return;
        };
        if let Err(error) = self.remap_source_to(index, path) {
            self.set_status(error, StatusTone::Error);
        }
    }

    /// Remap a source to a new root path, preserving the source id and tags.
    pub fn remap_source_to(&mut self, index: usize, new_root: PathBuf) -> Result<(), String> {
        let Some(existing) = self.sources.get(index) else {
            return Err("Source not found".into());
        };
        let normalized = crate::sample_sources::config::normalize_path(new_root.as_path());
        if !normalized.is_dir() {
            return Err("Please select a directory".into());
        }
        if self
            .sources
            .iter()
            .enumerate()
            .any(|(i, source)| i != index && source.root == normalized)
        {
            return Err("Source already added".into());
        }
        let old_db_path = crate::sample_sources::database_path_for(&existing.root);
        let new_db_path = crate::sample_sources::database_path_for(&normalized);
        if old_db_path.exists() && !new_db_path.exists() {
            let _ = fs::create_dir_all(&normalized);
            fs::copy(&old_db_path, &new_db_path)
                .map_err(|err| format!("Failed to copy database: {err}"))?;
        }
        SourceDatabase::open(&normalized)
            .map_err(|err| format!("Failed to prepare database: {err}"))?;
        let source_id = existing.id.clone();
        self.sources[index].root = normalized.clone();
        self.missing.sources.remove(&source_id);
        let mut invalidator = source_cache_invalidator::SourceCacheInvalidator::new(
            &mut self.db_cache,
            &mut self.wav_cache.entries,
            &mut self.wav_cache.lookup,
            &mut self.browser_cache.labels,
            &mut self.missing.wavs,
            &mut self.folder_browsers.models,
        );
        invalidator.invalidate_db_cache(&source_id);
        invalidator.invalidate_wav_related(&source_id);
        if self.selection_ctx.selected_source.as_ref() == Some(&source_id) {
            self.clear_wavs();
            self.selection_ctx.selected_source = Some(source_id.clone());
        }
        self.persist_config("Failed to save config after remapping source")?;
        self.refresh_sources_ui();
        self.queue_wav_load();
        self.set_status("Source remapped", StatusTone::Info);
        Ok(())
    }

    /// Open the source root in the OS file explorer.
    pub fn open_source_folder(&mut self, index: usize) {
        let Some(source) = self.sources.get(index) else {
            self.set_status("Source not found", StatusTone::Error);
            return;
        };
        if !source.root.exists() {
            self.set_status(
                format!("Source folder missing: {}", source.root.display()),
                StatusTone::Warning,
            );
            return;
        }
        if let Err(err) = open::that(&source.root) {
            self.set_status(
                format!("Could not open folder {}: {err}", source.root.display()),
                StatusTone::Error,
            );
        }
    }
}
