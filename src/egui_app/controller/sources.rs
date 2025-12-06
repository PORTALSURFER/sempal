use super::*;

impl EguiController {
    /// Select the first available source or refresh the current one.
    pub fn select_first_source(&mut self) {
        if self.selected_source.is_none() {
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

    /// Change the selected source by id and refresh dependent state.
    pub fn select_source(&mut self, id: Option<SourceId>) {
        self.select_source_internal(id, None);
    }

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
        self.db_cache.remove(&removed.id);
        self.collections
            .iter_mut()
            .for_each(|collection| collection.prune_source(&removed.id));
        if self
            .selected_source
            .as_ref()
            .is_some_and(|id| id == &removed.id)
        {
            self.selected_source = None;
        }
        let _ = self.persist_config("Failed to save config after removing source");
        self.refresh_sources_ui();
        let _ = self.refresh_wavs();
        self.refresh_collections_ui();
        self.select_first_source();
        self.set_status("Source removed", StatusTone::Info);
    }

    pub(super) fn refresh_sources_ui(&mut self) {
        self.ui.sources.rows = self.sources.iter().map(view_model::source_row).collect();
        self.ui.sources.menu_row = None;
        self.ui.sources.selected = self
            .selected_source
            .as_ref()
            .and_then(|id| self.sources.iter().position(|s| &s.id == id));
        self.ui.sources.scroll_to = self.ui.sources.selected;
    }

    pub(super) fn current_source(&self) -> Option<SampleSource> {
        let selected = self.selected_source.as_ref()?;
        self.sources.iter().find(|s| &s.id == selected).cloned()
    }

    fn select_source_internal(&mut self, id: Option<SourceId>, pending_path: Option<PathBuf>) {
        let same_source = self.selected_source == id;
        self.pending_select_path = pending_path.clone();
        if same_source {
            self.refresh_sources_ui();
            if let Some(path) = self.pending_select_path.clone() {
                if self.wav_lookup.contains_key(&path) {
                    self.pending_select_path = None;
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
        self.selected_source = id;
        self.selected_wav = None;
        self.loaded_wav = None;
        self.refresh_sources_ui();
        self.queue_wav_load();
        let _ = self.persist_config("Failed to save selection");
        // Do not auto-scan; only run when explicitly requested.
    }

    fn clear_wavs(&mut self) {
        self.wav_entries.clear();
        self.wav_lookup.clear();
        self.selected_wav = None;
        self.loaded_wav = None;
        self.ui.triage = TriageState::default();
        self.ui.loaded_wav = None;
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
}
