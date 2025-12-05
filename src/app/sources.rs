use super::navigation::compute_target_index;
use super::*;

/// Error details surfaced when adding a new sample source fails validation.
struct AddSourceFailure {
    message: String,
    state: StatusState,
}

impl DropHandler {
    /// Prompt for a directory and add it as a new sample source.
    pub fn handle_add_source(&self) {
        let Some(app) = self.app() else {
            return;
        };
        let Some(path) = FileDialog::new().pick_folder() else {
            return;
        };
        if let Err(error) = self.add_source_from_path(&app, path) {
            self.set_status(&app, error.message, error.state);
        }
    }

    /// Validate and persist a new source selected from the filesystem.
    fn add_source_from_path(
        &self,
        app: &HelloWorld,
        path: PathBuf,
    ) -> Result<(), AddSourceFailure> {
        let normalized = config::normalize_path(path.as_path());
        if !normalized.is_dir() {
            return Err(AddSourceFailure {
                message: "Please select a directory".into(),
                state: StatusState::Warning,
            });
        }
        let mut sources = self.sources.borrow_mut();
        if sources.iter().any(|s| s.root == normalized) {
            return Err(AddSourceFailure {
                message: "Source already added".into(),
                state: StatusState::Info,
            });
        }
        let source = SampleSource::new(normalized.clone());
        if let Err(error) = SourceDatabase::open(&normalized) {
            return Err(AddSourceFailure {
                message: format!("Failed to create database: {error}"),
                state: StatusState::Error,
            });
        }
        let _ = self.cache_db(&source);
        sources.push(source.clone());
        drop(sources);
        if let Err(error) = self.save_sources() {
            return Err(AddSourceFailure {
                message: format!("Failed to save config: {error}"),
                state: StatusState::Error,
            });
        }
        self.refresh_sources(&app);
        self.select_source_by_id(&app, &source.id);
        self.start_scan_for(source, true);
        Ok(())
    }

    /// Respond to selecting a source in the UI.
    pub fn handle_source_selected(&self, index: i32) {
        if index < 0 {
            return;
        }
        let Some(app) = self.app() else {
            return;
        };
        let Some(source) = self.sources.borrow().get(index as usize).cloned() else {
            return;
        };
        self.select_source_by_id(&app, &source.id);
    }

    /// Respond to a wav row click.
    pub fn handle_wav_clicked(&self, path: slint::SharedString) {
        let Some(app) = self.app() else {
            return;
        };
        let Some(source) = self.current_source() else {
            return;
        };
        let target = path.as_str();
        let Some(entry) = self
            .wav_entries
            .borrow()
            .iter()
            .find(|e| e.relative_path.to_string_lossy() == target)
            .cloned()
        else {
            return;
        };
        self.select_entry(&app, &entry, &source);
    }

    /// Trigger a rescan of the selected source.
    pub fn handle_update_source(&self, index: i32) {
        if index < 0 {
            return;
        }
        let Some(source) = self.sources.borrow().get(index as usize).cloned() else {
            return;
        };
        self.start_scan_for(source, true);
    }

    /// Remove a configured source and clean related state.
    pub fn handle_remove_source(&self, index: i32) {
        if index < 0 {
            return;
        }
        let Some(app) = self.app() else {
            return;
        };
        let removed = {
            let mut sources = self.sources.borrow_mut();
            if (index as usize) >= sources.len() {
                return;
            }
            sources.remove(index as usize)
        };
        self.db_cache.borrow_mut().remove(&removed.id);
        self.scan_tracker.borrow_mut().forget(&removed.id);
        self.pending_tags.borrow_mut().remove(&removed.id);
        let mut selected = self.selected_source.borrow_mut();
        if selected.as_ref().is_some_and(|id| id == &removed.id) {
            *selected = None;
        }
        drop(selected);
        if let Err(error) = self.save_sources() {
            self.set_status(
                &app,
                format!("Failed to save config: {error}"),
                StatusState::Error,
            );
            return;
        }
        self.refresh_sources(&app);
        if self.selected_source.borrow().is_none() {
            self.select_first_source(&app);
        } else {
            self.refresh_wavs(&app);
        }
        self.set_status(&app, "Source removed", StatusState::Info);
    }

    fn select_wav_at_index(&self, app: &HelloWorld, index: usize) {
        let Some(source) = self.current_source() else {
            return;
        };
        let Some(entry) = self.wav_entries.borrow().get(index).cloned() else {
            return;
        };
        self.select_entry(app, &entry, &source);
    }

    fn select_entry(&self, app: &HelloWorld, entry: &WavEntry, source: &SampleSource) {
        self.selected_wav
            .borrow_mut()
            .replace(entry.relative_path.clone());
        self.update_wav_view(app);
        self.load_from_source(app, source, entry);
    }

    fn load_from_source(&self, app: &HelloWorld, source: &SampleSource, entry: &WavEntry) {
        let full_path = source.root.join(&entry.relative_path);
        if !full_path.exists() {
            self.prune_missing_entry(source, entry);
            self.refresh_wavs(app);
            self.set_status(
                app,
                "File missing on disk. Removed from library.",
                StatusState::Warning,
            );
            return;
        }
        if self.handle_drop(full_path.as_path()) {
            self.loaded_wav
                .borrow_mut()
                .replace(entry.relative_path.clone());
            self.update_wav_view(app);
            let _ = self.play_audio(*self.loop_enabled.borrow());
        }
    }

    /// Move wav selection by delta rows; returns true when a move occurs.
    pub(super) fn move_selection(&self, delta: isize) -> bool {
        let target_index = {
            let entries = self.wav_entries.borrow();
            let current = Self::entry_index(&entries, &self.selected_wav.borrow());
            let target = compute_target_index(current, entries.len(), delta);
            match target {
                Some(target) if current != Some(target) => target,
                _ => return false,
            }
        };
        let Some(app) = self.app() else {
            return false;
        };
        self.select_wav_at_index(&app, target_index);
        true
    }

    /// Currently selected source, if one is set.
    pub(super) fn current_source(&self) -> Option<SampleSource> {
        let selected = self.selected_source.borrow().clone()?;
        self.sources
            .borrow()
            .iter()
            .find(|s| s.id == selected)
            .cloned()
    }

    fn prune_missing_entry(&self, source: &SampleSource, entry: &WavEntry) {
        if let Ok(db) = self.database_for(source) {
            let _ = db.remove_file(&entry.relative_path);
        }
        self.wav_entries
            .borrow_mut()
            .retain(|e| e.relative_path != entry.relative_path);
        if self
            .selected_wav
            .borrow()
            .as_ref()
            .is_some_and(|path| path == &entry.relative_path)
        {
            self.selected_wav.borrow_mut().take();
        }
        if self
            .loaded_wav
            .borrow()
            .as_ref()
            .is_some_and(|path| path == &entry.relative_path)
        {
            self.loaded_wav.borrow_mut().take();
        }
    }

    /// Refresh the wav list from disk/database for the current source.
    pub(super) fn refresh_wavs(&self, app: &HelloWorld) {
        let Some(source) = self.current_source() else {
            self.wav_entries.borrow_mut().clear();
            self.selected_wav.borrow_mut().take();
            self.loaded_wav.borrow_mut().take();
            self.update_wav_view(app);
            return;
        };
        match self.database_for(&source).and_then(|db| db.list_files()) {
            Ok(entries) => {
                self.wav_entries.replace(entries.clone());
                self.update_wav_view(app);
                self.set_status(
                    app,
                    format!("{} wav files loaded", entries.len()),
                    StatusState::Info,
                );
            }
            Err(error) => self.set_status(
                app,
                format!("Failed to load wavs: {error}"),
                StatusState::Error,
            ),
        }
    }

    /// Update UI bindings for the wav list selection and loaded file state.
    pub(super) fn update_wav_view(&self, app: &HelloWorld) {
        let entries = self.wav_entries.borrow();
        let selected_index = {
            let selected = self.selected_wav.borrow();
            let index = Self::entry_index(&entries, &selected);
            if index.is_none() && selected.is_some() {
                drop(selected);
                self.selected_wav.borrow_mut().take();
            }
            index
        };
        let loaded_index = {
            let loaded = self.loaded_wav.borrow();
            let index = Self::entry_index(&entries, &loaded);
            if index.is_none() && loaded.is_some() {
                drop(loaded);
                self.loaded_wav.borrow_mut().take();
            }
            index
        };
        let mut trash_rows = Vec::new();
        let mut neutral_rows = Vec::new();
        let mut keep_rows = Vec::new();
        let mut selected_target: Option<(SampleTag, usize)> = None;
        let mut loaded_path = String::new();

        for (i, entry) in entries.iter().enumerate() {
            let selected = Some(i) == selected_index;
            let loaded = Some(i) == loaded_index;
            if loaded {
                loaded_path = entry.relative_path.to_string_lossy().to_string();
            }
            let row = Self::wav_row(entry, selected, loaded);
            match entry.tag {
                SampleTag::Trash => {
                    if selected {
                        selected_target = Some((SampleTag::Trash, trash_rows.len()));
                    }
                    trash_rows.push(row);
                }
                SampleTag::Neutral => {
                    if selected {
                        selected_target = Some((SampleTag::Neutral, neutral_rows.len()));
                    }
                    neutral_rows.push(row);
                }
                SampleTag::Keep => {
                    if selected {
                        selected_target = Some((SampleTag::Keep, keep_rows.len()));
                    }
                    keep_rows.push(row);
                }
            }
        }

        let trash_model = Rc::new(slint::VecModel::from(trash_rows));
        app.set_wavs_trash(trash_model.into());
        let neutral_model = Rc::new(slint::VecModel::from(neutral_rows));
        app.set_wavs_neutral(neutral_model.into());
        let keep_model = Rc::new(slint::VecModel::from(keep_rows));
        app.set_wavs_keep(keep_model.into());

        let (selected_trash, selected_neutral, selected_keep) = match selected_target {
            Some((SampleTag::Trash, index)) => (index as i32, -1, -1),
            Some((SampleTag::Neutral, index)) => (-1, index as i32, -1),
            Some((SampleTag::Keep, index)) => (-1, -1, index as i32),
            None => (-1, -1, -1),
        };

        app.set_selected_trash(selected_trash);
        app.set_selected_neutral(selected_neutral);
        app.set_selected_keep(selected_keep);
        self.scroll_wavs_to(app, selected_target);
        app.set_loaded_wav_path(loaded_path.into());
    }

    pub(super) fn entry_index(entries: &[WavEntry], target: &Option<PathBuf>) -> Option<usize> {
        target.as_ref().and_then(|path| {
            entries
                .iter()
                .position(|entry| &entry.relative_path == path)
        })
    }

    /// Fetch or open the cached database for a sample source.
    pub(super) fn database_for(
        &self,
        source: &SampleSource,
    ) -> Result<Rc<SourceDatabase>, SourceDbError> {
        if let Some(existing) = self.db_cache.borrow().get(&source.id) {
            return Ok(existing.clone());
        }
        let db = Rc::new(SourceDatabase::open(&source.root)?);
        self.db_cache
            .borrow_mut()
            .insert(source.id.clone(), db.clone());
        Ok(db)
    }

    /// Ensure the database is opened and cached for later use.
    pub(super) fn cache_db(
        &self,
        source: &SampleSource,
    ) -> Result<Rc<SourceDatabase>, SourceDbError> {
        self.database_for(source)
    }

    /// Load persisted sources from disk and update the UI.
    pub(super) fn load_sources(&self, app: &HelloWorld) {
        match config::load_or_default() {
            Ok(cfg) => {
                self.sources.replace(cfg.sources);
                self.refresh_sources(app);
                self.select_first_source(app);
            }
            Err(error) => self.set_status(
                app,
                format!("Config load failed: {error}"),
                StatusState::Error,
            ),
        }
    }

    /// Save the current source list to disk.
    pub(super) fn save_sources(&self) -> Result<(), config::ConfigError> {
        config::save(&AppConfig {
            sources: self.sources.borrow().clone(),
        })
    }

    /// Push the current sources into the UI model and selection.
    pub(super) fn refresh_sources(&self, app: &HelloWorld) {
        let rows = self
            .sources
            .borrow()
            .iter()
            .map(Self::source_row)
            .collect::<Vec<_>>();
        let model = Rc::new(slint::VecModel::from(rows));
        app.set_sources(model.into());
        let index = self
            .selected_source
            .borrow()
            .as_ref()
            .and_then(|id| self.sources.borrow().iter().position(|s| &s.id == id))
            .map(|i| i as i32)
            .unwrap_or(-1);
        app.set_selected_source(index);
        app.set_source_menu_index(-1);
        self.scroll_sources_to(app, index);
    }

    fn select_first_source(&self, app: &HelloWorld) {
        if let Some(first) = self.sources.borrow().first().cloned() {
            self.select_source_by_id(app, &first.id);
        } else {
            self.wav_entries.borrow_mut().clear();
            self.selected_wav.borrow_mut().take();
            self.loaded_wav.borrow_mut().take();
            self.update_wav_view(app);
        }
    }

    fn select_source_by_id(&self, app: &HelloWorld, id: &SourceId) {
        self.selected_source.replace(Some(id.clone()));
        self.selected_wav.borrow_mut().take();
        self.loaded_wav.borrow_mut().take();
        self.refresh_sources(app);
        self.refresh_wavs(app);
    }
}
