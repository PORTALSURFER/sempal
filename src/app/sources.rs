use super::navigation::compute_target_index;
use super::*;
use crate::app::metrics;
use crate::app::wav_list::{WavListJob, WavListJobResult};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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
        let target = PathBuf::from(path.as_str());
        let Some(entry) = self.entry_for_path(&target) else {
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
        if self
            .loaded_wav
            .borrow()
            .as_ref()
            .is_some_and(|path| path != &entry.relative_path)
        {
            self.loaded_wav.borrow_mut().take();
        }
        self.update_wav_view(app, false);
        self.stop_playback_ui(app);
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
        if let Some(cached) = self
            .waveform_cache
            .borrow_mut()
            .get(&source.id, &entry.relative_path)
        {
            self.loaded_wav
                .borrow_mut()
                .replace(entry.relative_path.clone());
            self.update_wav_view(app, false);
            self.apply_loaded_waveform(app, &cached);
            self.set_status(
                app,
                format!("Loaded {}", entry.relative_path.display()),
                StatusState::Info,
            );
            let _ = self.play_audio(*self.loop_enabled.borrow());
            return;
        }
        self.set_status(
            app,
            format!("Loading {}", entry.relative_path.display()),
            StatusState::Busy,
        );
        self.enqueue_waveform_load(&source.id, &source.root, &entry.relative_path);
    }

    /// Move wav selection by delta rows; returns true when a move occurs.
    pub(super) fn move_selection(&self, delta: isize) -> bool {
        let target_index = {
            let entries = self.wav_entries.borrow();
            self.sync_wav_lookup(&entries);
            let current = {
                let selected = self.selected_wav.borrow();
                self.lookup_index_in_entries(&entries, &selected)
            };
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
        {
            let mut entries = self.wav_entries.borrow_mut();
            entries.retain(|e| e.relative_path != entry.relative_path);
        }
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
        let entries = self.wav_entries.borrow();
        self.rebuild_wav_lookup(&entries);
    }

    /// Refresh the wav list from disk/database for the current source.
    pub(super) fn refresh_wavs(&self, app: &HelloWorld) {
        let Some(source) = self.current_source() else {
            self.wav_entries.borrow_mut().clear();
            self.selected_wav.borrow_mut().take();
            self.loaded_wav.borrow_mut().take();
            self.wav_lookup.borrow_mut().clear();
            self.wav_batch.borrow_mut().take();
            self.update_wav_view(app, true);
            return;
        };
        match metrics::profile("list_files", || {
            self.database_for(&source).and_then(|db| db.list_files())
        }) {
            Ok(entries) => {
                self.wav_entries.replace(entries.clone());
                self.rebuild_wav_lookup(&entries);
                metrics::profile("update_wav_view_initial", || {
                    self.update_wav_view(app, true);
                });
                self.wav_batch.borrow_mut().take();
                self.set_status(
                    app,
                    format!("{} wav files loaded", entries.len()),
                    StatusState::Info,
                );
                if !*self.shutting_down.borrow() {
                    let job = WavListJob {
                        source_id: source.id.clone(),
                        root: source.root.clone(),
                    };
                    let _ = self.wav_list_tx.send(job);
                }
            }
            Err(error) => self.set_status(
                app,
                format!("Failed to load wavs: {error}"),
                StatusState::Error,
            ),
        }
    }

    /// Update UI bindings for the wav list selection and loaded file state.
    pub(super) fn update_wav_view(&self, app: &HelloWorld, rebuild_models: bool) {
        let entries = self.wav_entries.borrow();
        self.sync_wav_lookup(&entries);
        let selected_index = {
            let selected = self.selected_wav.borrow();
            let index = self.lookup_index_in_entries(&entries, &selected);
            if index.is_none() && selected.is_some() {
                drop(selected);
                self.selected_wav.borrow_mut().take();
            }
            index
        };
        let loaded_index = {
            let loaded = self.loaded_wav.borrow();
            let index = self.lookup_index_in_entries(&entries, &loaded);
            if index.is_none() && loaded.is_some() {
                drop(loaded);
                self.loaded_wav.borrow_mut().take();
            }
            index
        };
        let (selected_target, loaded_path, refreshed_models) = {
            let mut models = self.wav_models.borrow_mut();
            if rebuild_models || !models.is_synced(entries.len()) {
                let (sel, loaded) = metrics::profile("wavs_rebuild_models", || {
                    models.rebuild(entries.as_slice(), selected_index, loaded_index)
                });
                (sel, loaded, true)
            } else {
                let selected_path = selected_index
                    .and_then(|i| entries.get(i))
                    .map(|e| e.relative_path.as_path());
                let loaded_path = loaded_index
                    .and_then(|i| entries.get(i))
                    .map(|e| e.relative_path.as_path());
                let (sel, loaded) = metrics::profile("wavs_update_selection", || {
                    models.update_selection(entries.as_slice(), selected_path, loaded_path)
                });
                (sel, loaded, false)
            }
        };
        if refreshed_models {
            let (trash_model, neutral_model, keep_model) = self.wav_models.borrow().models();
            app.set_wavs_trash(trash_model.into());
            app.set_wavs_neutral(neutral_model.into());
            app.set_wavs_keep(keep_model.into());
        }
        Self::apply_selection_to_app(app, selected_target);
        self.scroll_wavs_to(app, selected_target);
        app.set_loaded_wav_path(loaded_path.unwrap_or_default().into());
    }

    /// Start polling for wav list load results.
    pub(super) fn start_wav_list_polling(&self) {
        if *self.shutting_down.borrow() {
            return;
        }
        let poller = self.clone();
        self.wav_list_poll_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(60),
            move || poller.process_wav_list_queue(),
        );
    }

    /// Start batching large wav list updates to keep UI responsive.
    pub(super) fn start_wav_batching(&self) {
        if *self.shutting_down.borrow() {
            return;
        }
        let poller = self.clone();
        self.wav_batch_poll_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(30),
            move || poller.process_pending_batches(),
        );
    }

    /// Apply any pending batched wav list rebuilds.
    pub(super) fn process_pending_batches(&self) {
        let Some(app) = self.app() else {
            return;
        };
        self.consume_pending_batch(&app);
    }

    /// Process any queued wav list results from the background worker.
    pub(super) fn process_wav_list_queue(&self) {
        let Some(app) = self.app() else {
            return;
        };
        while let Ok(message) = self.wav_list_rx.borrow().try_recv() {
            self.handle_wav_list_result(&app, message);
        }
    }

    fn handle_wav_list_result(&self, app: &HelloWorld, message: WavListJobResult) {
        if !self
            .sources
            .borrow()
            .iter()
            .any(|source| source.id == message.source_id)
        {
            return;
        }
        match message.result {
            Ok(payload) => {
                if self
                    .selected_source
                    .borrow()
                    .as_ref()
                    .is_some_and(|id| id == &message.source_id)
                {
                    let current_len = self.wav_entries.borrow().len();
                    if payload.entries.len() != current_len {
                        self.enqueue_batch_refresh(payload.entries);
                    }
                    if !payload.missing_paths.is_empty() {
                        let missing_count = payload.missing_paths.len();
                        self.set_status(
                            app,
                            format!("Some files missing on disk ({missing_count} paths)"),
                            StatusState::Warning,
                        );
                    } else {
                        // Keep the existing "wav files loaded" status from the synchronous path.
                    }
                }
            }
            Err(error) => {
                if self
                    .selected_source
                    .borrow()
                    .as_ref()
                    .is_some_and(|id| id == &message.source_id)
                {
                    self.set_status(
                        app,
                        format!("Failed to load wavs: {error}"),
                        StatusState::Error,
                    );
                }
            }
        }
    }

    fn apply_selection_to_app(app: &HelloWorld, selected: Option<(SampleTag, usize)>) {
        let (selected_trash, selected_neutral, selected_keep) = match selected {
            Some((SampleTag::Trash, index)) => (index as i32, -1, -1),
            Some((SampleTag::Neutral, index)) => (-1, index as i32, -1),
            Some((SampleTag::Keep, index)) => (-1, -1, index as i32),
            None => (-1, -1, -1),
        };
        app.set_selected_trash(selected_trash);
        app.set_selected_neutral(selected_neutral);
        app.set_selected_keep(selected_keep);
    }

    pub(super) fn sync_wav_lookup(&self, entries: &[WavEntry]) {
        let needs_rebuild = {
            let lookup = self.wav_lookup.borrow();
            lookup.len() != entries.len()
        };
        if needs_rebuild {
            self.rebuild_wav_lookup(entries);
        }
    }

    fn rebuild_wav_lookup(&self, entries: &[WavEntry]) {
        let mut lookup = self.wav_lookup.borrow_mut();
        lookup.clear();
        for (index, entry) in entries.iter().enumerate() {
            lookup.insert(entry.relative_path.clone(), index);
        }
    }

    pub(super) fn lookup_index_in_entries(
        &self,
        entries: &[WavEntry],
        target: &Option<PathBuf>,
    ) -> Option<usize> {
        let path = target.as_ref()?;
        self.lookup_index_for_path(entries, path)
    }

    fn lookup_index_for_path(&self, entries: &[WavEntry], path: &Path) -> Option<usize> {
        if let Some(index) = self.wav_lookup.borrow().get(path).copied() {
            return Some(index);
        }
        let index = entries
            .iter()
            .position(|entry| entry.relative_path == path)?;
        self.wav_lookup
            .borrow_mut()
            .insert(path.to_path_buf(), index);
        Some(index)
    }

    fn entry_for_path(&self, path: &Path) -> Option<WavEntry> {
        let entries = self.wav_entries.borrow();
        self.sync_wav_lookup(&entries);
        let index = self.lookup_index_for_path(&entries, path)?;
        entries.get(index).cloned()
    }

    fn enqueue_batch_refresh(&self, entries: Vec<WavEntry>) {
        *self.wav_batch.borrow_mut() = Some(BatchState::new(entries));
    }

    fn consume_pending_batch(&self, app: &HelloWorld) {
        let mut batch_opt = self.wav_batch.borrow_mut();
        let Some(batch) = batch_opt.as_mut() else {
            return;
        };
        let applied = batch.apply_next_chunk(&self.wav_entries);
        if applied {
            self.rebuild_wav_lookup(&self.wav_entries.borrow());
            self.update_wav_view(app, true);
        }
        if batch.is_finished() {
            batch_opt.take();
        }
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
            self.wav_lookup.borrow_mut().clear();
            self.update_wav_view(app, true);
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

/// Incremental wav list builder to avoid long UI stalls on large sources.
#[derive(Debug)]
pub struct BatchState {
    entries: Vec<WavEntry>,
    applied: usize,
    chunk_size: usize,
}

impl BatchState {
    pub fn new(entries: Vec<WavEntry>) -> Self {
        Self {
            entries,
            applied: 0,
            chunk_size: 300,
        }
    }

    /// Copy the next chunk of entries into the shared wav list; returns true when a change was applied.
    pub fn apply_next_chunk(&mut self, target: &Rc<RefCell<Vec<WavEntry>>>) -> bool {
        if self.applied >= self.entries.len() {
            return false;
        }
        let end = (self.applied + self.chunk_size).min(self.entries.len());
        let mut dest = target.borrow_mut();
        dest.clear();
        dest.extend_from_slice(&self.entries[..end]);
        self.applied = end;
        true
    }

    pub fn is_finished(&self) -> bool {
        self.applied >= self.entries.len()
    }
}
