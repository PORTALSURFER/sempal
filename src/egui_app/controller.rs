#![allow(dead_code)]
//! Controller is being integrated incrementally with the egui renderer.

use crate::audio::AudioPlayer;
use crate::egui_app::state::*;
use crate::egui_app::view_model;
use crate::sample_sources::config::{self, FeatureFlags};
use crate::sample_sources::scanner::{ScanError, ScanStats, scan_once};
use crate::sample_sources::{
    Collection, CollectionId, SampleSource, SampleTag, SourceDatabase, SourceDbError, SourceId,
    WavEntry,
};
use crate::selection::{SelectionRange, SelectionState};
use crate::waveform::WaveformRenderer;
use egui::Color32;
use rfd::FileDialog;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

/// Maintains app state and bridges core logic to the egui UI.
pub struct EguiController {
    pub ui: UiState,
    renderer: WaveformRenderer,
    player: Option<Rc<RefCell<AudioPlayer>>>,
    sources: Vec<SampleSource>,
    collections: Vec<Collection>,
    db_cache: HashMap<SourceId, Rc<SourceDatabase>>,
    wav_entries: Vec<WavEntry>,
    wav_lookup: HashMap<PathBuf, usize>,
    selected_source: Option<SourceId>,
    selected_collection: Option<CollectionId>,
    selected_wav: Option<PathBuf>,
    loaded_wav: Option<PathBuf>,
    feature_flags: FeatureFlags,
    selection: SelectionState,
    wav_job_tx: Sender<WavLoadJob>,
    wav_job_rx: Receiver<WavLoadResult>,
    pending_source: Option<SourceId>,
    scan_rx: Option<Receiver<ScanResult>>,
    scan_in_progress: bool,
}

const MIN_SELECTION_WIDTH: f32 = 0.001;

impl EguiController {
    /// Create a controller with shared renderer and optional audio player.
    pub fn new(renderer: WaveformRenderer, player: Option<Rc<RefCell<AudioPlayer>>>) -> Self {
        let (wav_job_tx, wav_job_rx) = spawn_wav_loader();
        Self {
            ui: UiState::default(),
            renderer,
            player,
            sources: Vec::new(),
            collections: Vec::new(),
            db_cache: HashMap::new(),
            wav_entries: Vec::new(),
            wav_lookup: HashMap::new(),
            selected_source: None,
            selected_collection: None,
            selected_wav: None,
            loaded_wav: None,
            feature_flags: FeatureFlags::default(),
            selection: SelectionState::new(),
            wav_job_tx,
            wav_job_rx,
            pending_source: None,
            scan_rx: None,
            scan_in_progress: false,
        }
    }

    /// Load persisted config and populate initial UI state.
    pub fn load_configuration(&mut self) -> Result<(), config::ConfigError> {
        let cfg = config::load_or_default()?;
        self.feature_flags = cfg.feature_flags;
        self.ui.collections.enabled = self.feature_flags.collections_enabled;
        self.sources = cfg.sources;
        self.collections = cfg.collections;
        self.ensure_collection_selection();
        self.refresh_sources_ui();
        self.refresh_collections_ui();
        Ok(())
    }

    /// Select the first source if none is active.
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
        if self.selected_source == id {
            // Avoid reloading the same source just because it was clicked again.
            self.refresh_sources_ui();
            return;
        }
        self.selected_source = id;
        self.selected_wav = None;
        self.loaded_wav = None;
        self.refresh_sources_ui();
        self.queue_wav_load();
        // Do not auto-scan; only run when explicitly requested.
    }

    /// Refresh wav entries for the current source.
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
        let normalized = config::normalize_path(path.as_path());
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

    fn clear_wavs(&mut self) {
        self.wav_entries.clear();
        self.wav_lookup.clear();
        self.selected_wav = None;
        self.loaded_wav = None;
        self.ui.triage = TriageState::default();
        self.ui.loaded_wav = None;
    }

    /// Select a wav row based on its path.
    pub fn select_wav_by_path(&mut self, path: &Path) {
        if self.wav_lookup.contains_key(path) {
            self.selected_wav = Some(path.to_path_buf());
            if let Some(source) = self.current_source() {
                if let Err(err) = self.load_waveform_for_selection(&source, path) {
                    self.set_status(err, StatusTone::Error);
                }
            }
            self.rebuild_triage_lists();
        }
    }

    /// Begin a selection drag at the given normalized position.
    pub fn start_selection_drag(&mut self, position: f32) {
        let range = self.selection.begin_new(position);
        self.apply_selection(Some(range));
    }

    /// Update the selection drag with a new normalized position.
    pub fn update_selection_drag(&mut self, position: f32) {
        if let Some(range) = self.selection.update_drag(position) {
            self.apply_selection(Some(range));
        }
    }

    /// Finish a selection drag gesture.
    pub fn finish_selection_drag(&mut self) {
        self.selection.finish_drag();
    }

    /// Clear any active selection.
    pub fn clear_selection(&mut self) {
        if self.selection.clear() {
            self.apply_selection(None);
        }
    }

    /// Toggle loop playback state.
    pub fn toggle_loop(&mut self) {
        self.ui.waveform.loop_enabled = !self.ui.waveform.loop_enabled;
    }

    /// Seek to a normalized position and start playback.
    pub fn seek_to(&mut self, position: f32) {
        if let Err(err) = self.play_audio(false, Some(position)) {
            self.set_status(err, StatusTone::Error);
        }
    }

    /// Start playback over the current selection or full range.
    pub fn play_audio(&mut self, looped: bool, start_override: Option<f32>) -> Result<(), String> {
        let player = self.ensure_player()?;
        let Some(player) = player else {
            return Err("Audio unavailable".into());
        };
        let selection = self
            .selection
            .range()
            .filter(|range| range.width() >= MIN_SELECTION_WIDTH);
        let start = start_override
            .or_else(|| selection.as_ref().map(|range| range.start()))
            .unwrap_or(0.0);
        let span_end = selection.as_ref().map(|r| r.end()).unwrap_or(1.0);
        player.borrow_mut().play_range(start, span_end, looped)?;
        self.ui.waveform.playhead.visible = true;
        self.ui.waveform.playhead.position = start;
        Ok(())
    }

    fn rebuild_wav_lookup(&mut self) {
        self.wav_lookup.clear();
        for (index, entry) in self.wav_entries.iter().enumerate() {
            self.wav_lookup.insert(entry.relative_path.clone(), index);
        }
    }

    fn rebuild_triage_lists(&mut self) {
        let selected_index = self.selected_row_index();
        let loaded_index = self.loaded_row_index();
        self.reset_triage_ui();

        for i in 0..self.wav_entries.len() {
            let entry = self.wav_entries[i].clone();
            let flags = RowFlags {
                selected: Some(i) == selected_index,
                loaded: Some(i) == loaded_index,
            };
            self.push_triage_row(&entry, flags);
        }
    }

    fn selected_row_index(&self) -> Option<usize> {
        self.selected_wav
            .as_ref()
            .and_then(|path| self.wav_lookup.get(path).copied())
    }

    fn loaded_row_index(&self) -> Option<usize> {
        self.loaded_wav
            .as_ref()
            .and_then(|path| self.wav_lookup.get(path).copied())
    }

    fn reset_triage_ui(&mut self) {
        self.ui.triage.trash.clear();
        self.ui.triage.neutral.clear();
        self.ui.triage.keep.clear();
        self.ui.triage.selected = None;
        self.ui.triage.loaded = None;
        self.ui.loaded_wav = None;
    }

    fn push_triage_row(&mut self, entry: &WavEntry, flags: RowFlags) {
        let row = view_model::wav_row(entry, flags.selected, flags.loaded);
        let target = match entry.tag {
            SampleTag::Trash => &mut self.ui.triage.trash,
            SampleTag::Neutral => &mut self.ui.triage.neutral,
            SampleTag::Keep => &mut self.ui.triage.keep,
        };
        let row_index = target.len();
        target.push(row);
        if flags.selected {
            self.ui.triage.selected = Some(view_model::triage_index_for(entry.tag, row_index));
        }
        if flags.loaded {
            self.ui.triage.loaded = Some(view_model::triage_index_for(entry.tag, row_index));
            self.ui.loaded_wav = Some(entry.relative_path.clone());
        }
    }

    fn current_source(&self) -> Option<SampleSource> {
        let selected = self.selected_source.as_ref()?;
        self.sources.iter().find(|s| &s.id == selected).cloned()
    }

    fn refresh_sources_ui(&mut self) {
        self.ui.sources.rows = self.sources.iter().map(view_model::source_row).collect();
        self.ui.sources.menu_row = None;
        self.ui.sources.selected = self
            .selected_source
            .as_ref()
            .and_then(|id| self.sources.iter().position(|s| &s.id == id));
        self.ui.sources.scroll_to = self.ui.sources.selected;
    }

    fn refresh_collections_ui(&mut self) {
        let selected_id = self.selected_collection.clone();
        self.ui.collections.rows =
            view_model::collection_rows(&self.collections, selected_id.as_ref());
        self.ui.collections.selected = selected_id
            .as_ref()
            .and_then(|id| self.collections.iter().position(|c| &c.id == id));
        self.refresh_collection_samples();
    }

    fn refresh_collection_samples(&mut self) {
        let selected = self
            .selected_collection
            .as_ref()
            .and_then(|id| self.collections.iter().find(|c| &c.id == id));
        self.ui.collections.samples = view_model::collection_samples(selected, &self.sources);
    }

    fn ensure_collection_selection(&mut self) {
        if self.selected_collection.is_some() {
            return;
        }
        if let Some(first) = self.collections.first().cloned() {
            self.selected_collection = Some(first.id);
        }
    }

    /// Switch selected collection by index.
    pub fn select_collection_by_index(&mut self, index: Option<usize>) {
        if let Some(idx) = index {
            if let Some(collection) = self.collections.get(idx).cloned() {
                self.selected_collection = Some(collection.id);
            }
        } else {
            self.selected_collection = None;
        }
        self.refresh_collections_ui();
    }

    /// Create a new collection and persist.
    pub fn add_collection(&mut self) {
        if !self.feature_flags.collections_enabled {
            return;
        }
        let name = self.next_collection_name();
        let mut collection = Collection::new(name);
        let id = collection.id.clone();
        collection.members.clear();
        self.collections.push(collection);
        self.selected_collection = Some(id);
        let _ = self.persist_config("Failed to save collection");
        self.refresh_collections_ui();
        self.set_status("Collection created", StatusTone::Info);
    }

    /// Add a sample to the given collection id.
    pub fn add_sample_to_collection(
        &mut self,
        collection_id: &CollectionId,
        relative_path: &Path,
    ) -> Result<(), String> {
        if !self.feature_flags.collections_enabled {
            return Err("Collections are disabled".into());
        }
        let Some(source) = self.current_source() else {
            return Err("Select a source first".into());
        };
        if !self.wav_lookup.contains_key(relative_path) {
            return Err("Sample is not available to add".into());
        }
        let mut collections = self.collections.clone();
        let Some(collection) = collections.iter_mut().find(|c| &c.id == collection_id) else {
            return Err("Collection not found".into());
        };
        let added = collection.add_member(source.id.clone(), relative_path.to_path_buf());
        self.collections = collections;
        if added {
            self.persist_config("Failed to save collection")?;
            self.refresh_collections_ui();
            self.set_status(
                format!("Added {} to collection", relative_path.display()),
                StatusTone::Info,
            );
        } else {
            self.set_status("Already in collection", StatusTone::Info);
        }
        Ok(())
    }

    /// Manually trigger a scan of the selected source.
    pub fn request_scan(&mut self) {
        if self.scan_in_progress {
            self.set_status("Scan already in progress", StatusTone::Info);
            return;
        }
        let Some(source) = self.current_source() else {
            self.set_status("Select a source to scan", StatusTone::Warning);
            return;
        };
        let (tx, rx) = channel();
        self.scan_rx = Some(rx);
        self.scan_in_progress = true;
        self.set_status(
            format!("Scanning {}", source.root.display()),
            StatusTone::Busy,
        );
        let source_id = source.id.clone();
        thread::spawn(move || {
            let result = (|| -> Result<ScanStats, ScanError> {
                let db = SourceDatabase::open(&source.root)?;
                scan_once(&db)
            })();
            let _ = tx.send(ScanResult { source_id, result });
        });
    }

    fn database_for(&mut self, source: &SampleSource) -> Result<Rc<SourceDatabase>, SourceDbError> {
        if let Some(existing) = self.db_cache.get(&source.id) {
            return Ok(existing.clone());
        }
        let db = Rc::new(SourceDatabase::open(&source.root)?);
        self.db_cache.insert(source.id.clone(), db.clone());
        Ok(db)
    }

    fn cache_db(&mut self, source: &SampleSource) -> Result<Rc<SourceDatabase>, SourceDbError> {
        self.database_for(source)
    }

    /// Persist full config, reporting a friendly status on failure.
    fn persist_config(&mut self, error_prefix: &str) -> Result<(), String> {
        self.save_full_config()
            .map_err(|err| format!("{error_prefix}: {err}"))
    }

    fn save_full_config(&self) -> Result<(), config::ConfigError> {
        config::save(&config::AppConfig {
            sources: self.sources.clone(),
            collections: self.collections.clone(),
            feature_flags: self.feature_flags.clone(),
        })
    }

    fn load_waveform_for_selection(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        let full_path = source.root.join(relative_path);
        let bytes = fs::read(&full_path)
            .map_err(|err| format!("Failed to read {}: {err}", full_path.display()))?;
        let decoded = self.renderer.decode_from_bytes(&bytes)?;
        let color_image = self.renderer.render_color_image(&decoded.samples);
        self.ui.waveform.image = Some(WaveformImage { image: color_image });
        self.ui.waveform.playhead = PlayheadState::default();
        self.ui.waveform.selection = None;
        self.selection.clear();
        self.loaded_wav = Some(relative_path.to_path_buf());
        self.ui.loaded_wav = Some(relative_path.to_path_buf());
        if let Some(player) = self.ensure_player()? {
            let mut player = player.borrow_mut();
            player.stop();
            player.set_audio(bytes, decoded.duration_seconds);
        }
        self.set_status(
            format!("Loaded {}", relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }

    fn set_status(&mut self, text: impl Into<String>, tone: StatusTone) {
        let (label, color) = status_badge(tone);
        self.ui.status.text = text.into();
        self.ui.status.badge_label = label;
        self.ui.status.badge_color = color;
    }

    fn next_collection_name(&self) -> String {
        let base = "Collection";
        let mut index = self.collections.len() + 1;
        loop {
            let candidate = format!("{base} {index}");
            if !self.collections.iter().any(|c| c.name == candidate) {
                return candidate;
            }
            index += 1;
        }
    }

    fn ensure_player(&mut self) -> Result<Option<Rc<RefCell<AudioPlayer>>>, String> {
        if self.player.is_none() {
            let created = AudioPlayer::new().map_err(|err| format!("Audio init failed: {err}"))?;
            self.player = Some(Rc::new(RefCell::new(created)));
        }
        Ok(self.player.clone())
    }

    /// Advance playhead position and visibility from the underlying player.
    pub fn tick_playhead(&mut self) {
        self.poll_wav_loader();
        self.poll_scan();
        let Some(player) = self.player.as_ref() else {
            self.ui.waveform.playhead.visible = false;
            return;
        };
        let player_ref = player.borrow();
        if let Some(progress) = player_ref.progress() {
            self.ui.waveform.playhead.position = progress;
            self.ui.waveform.playhead.visible = player_ref.is_playing();
        } else {
            self.ui.waveform.playhead.visible = false;
        }
    }

    fn apply_selection(&mut self, range: Option<SelectionRange>) {
        if let Some(range) = range {
            self.ui.waveform.selection = Some(range);
        } else {
            self.ui.waveform.selection = None;
        }
    }

    /// Enqueue loading wav entries for the selected source on a worker thread.
    fn queue_wav_load(&mut self) {
        self.wav_entries.clear();
        self.rebuild_wav_lookup();
        self.rebuild_triage_lists();
        let Some(source) = self.current_source() else {
            return;
        };
        if self.pending_source.as_ref() == Some(&source.id) {
            return;
        }
        self.pending_source = Some(source.id.clone());
        let job = WavLoadJob {
            source_id: source.id.clone(),
            root: source.root.clone(),
        };
        let _ = self.wav_job_tx.send(job);
        self.set_status(
            format!("Loading wavs for {}", source.root.display()),
            StatusTone::Info,
        );
    }

    /// Process any completed wav load jobs.
    fn poll_wav_loader(&mut self) {
        while let Ok(message) = self.wav_job_rx.try_recv() {
            if Some(&message.source_id) != self.selected_source.as_ref() {
                continue;
            }
            match message.result {
                Ok(entries) => {
                    self.wav_entries = entries;
                    self.rebuild_wav_lookup();
                    self.rebuild_triage_lists();
                    self.set_status(
                        format!("{} wav files loaded", self.wav_entries.len()),
                        StatusTone::Info,
                    );
                }
                Err(err) => {
                    self.set_status(format!("Failed to load wavs: {err}"), StatusTone::Error);
                }
            }
            self.pending_source = None;
        }
    }

    /// Start tracking a drag for a sample.
    pub fn start_sample_drag(&mut self, path: PathBuf, label: String, pos: egui::Pos2) {
        self.ui.drag.active_path = Some(path);
        self.ui.drag.label = label;
        self.ui.drag.position = Some(pos);
        self.ui.drag.hovering_collection = None;
        self.ui.drag.hovering_drop_zone = false;
    }

    /// Update drag position and hover state.
    pub fn update_sample_drag(
        &mut self,
        pos: egui::Pos2,
        hovering_collection: Option<CollectionId>,
        hovering_drop_zone: bool,
    ) {
        self.ui.drag.position = Some(pos);
        self.ui.drag.hovering_collection = hovering_collection;
        self.ui.drag.hovering_drop_zone = hovering_drop_zone;
    }

    /// Finish drag and perform drop if applicable.
    pub fn finish_sample_drag(&mut self) {
        let path = match self.ui.drag.active_path.take() {
            Some(path) => path,
            None => {
                self.reset_drag();
                return;
            }
        };
        let target_id = if self.ui.drag.hovering_drop_zone {
            self.current_collection_id()
        } else {
            self.ui.drag.hovering_collection.clone()
        };
        self.reset_drag();
        if let Some(collection_id) = target_id {
            let _ = self.add_sample_to_collection(&collection_id, &path);
        }
    }

    fn reset_drag(&mut self) {
        self.ui.drag.active_path = None;
        self.ui.drag.label.clear();
        self.ui.drag.position = None;
        self.ui.drag.hovering_collection = None;
        self.ui.drag.hovering_drop_zone = false;
    }

    fn current_collection_id(&self) -> Option<CollectionId> {
        self.selected_collection.clone()
    }

    fn poll_scan(&mut self) {
        if let Some(rx) = &self.scan_rx {
            if let Ok(result) = rx.try_recv() {
                self.scan_in_progress = false;
                self.scan_rx = None;
                match result.result {
                    Ok(stats) => {
                        self.set_status(
                            format!(
                                "Scan complete: {} added, {} updated, {} removed",
                                stats.added, stats.updated, stats.removed
                            ),
                            StatusTone::Info,
                        );
                        self.queue_wav_load();
                    }
                    Err(err) => {
                        self.set_status(format!("Scan failed: {err}"), StatusTone::Error);
                    }
                }
            }
        }
    }
}

struct WavLoadJob {
    source_id: SourceId,
    root: PathBuf,
}

struct WavLoadResult {
    source_id: SourceId,
    result: Result<Vec<WavEntry>, String>,
}

struct ScanResult {
    source_id: SourceId,
    result: Result<ScanStats, ScanError>,
}

fn spawn_wav_loader() -> (Sender<WavLoadJob>, Receiver<WavLoadResult>) {
    let (tx, rx) = channel::<WavLoadJob>();
    let (result_tx, result_rx) = channel::<WavLoadResult>();
    thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            let result = load_entries(&job);
            let _ = result_tx.send(WavLoadResult {
                source_id: job.source_id.clone(),
                result,
            });
        }
    });
    (tx, result_rx)
}

fn load_entries(job: &WavLoadJob) -> Result<Vec<WavEntry>, String> {
    let db = SourceDatabase::open(&job.root).map_err(|err| format!("Database error: {err}"))?;
    db.list_files().map_err(|err| format!("Load failed: {err}"))
}

/// UI status tone for badge coloring.
#[derive(Clone, Copy, Debug)]
pub enum StatusTone {
    Idle,
    Busy,
    Info,
    Warning,
    Error,
}

fn status_badge(tone: StatusTone) -> (String, Color32) {
    match tone {
        StatusTone::Idle => ("Idle".into(), Color32::from_rgb(42, 42, 42)),
        StatusTone::Busy => ("Working".into(), Color32::from_rgb(31, 139, 255)),
        StatusTone::Info => ("Info".into(), Color32::from_rgb(64, 140, 112)),
        StatusTone::Warning => ("Warning".into(), Color32::from_rgb(192, 138, 43)),
        StatusTone::Error => ("Error".into(), Color32::from_rgb(192, 57, 43)),
    }
}

struct RowFlags {
    selected: bool,
    loaded: bool,
}
