use crate::audio::AudioPlayer;
use crate::egui_app::state::*;
use crate::egui_app::view_model;
use crate::sample_sources::config::{self, FeatureFlags};
use crate::sample_sources::{
    Collection, CollectionId, SampleSource, SampleTag, SourceDatabase, SourceDbError, SourceId,
    WavEntry,
};
use crate::waveform::WaveformRenderer;
use egui::Color32;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

/// Maintains app state and bridges core logic to the egui UI.
pub struct EguiController {
    pub ui: UiState,
    renderer: WaveformRenderer,
    _player: Rc<RefCell<AudioPlayer>>,
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
}

impl EguiController {
    pub fn new(renderer: WaveformRenderer, player: Rc<RefCell<AudioPlayer>>) -> Self {
        Self {
            ui: UiState::default(),
            renderer,
            _player: player,
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
        self.select_first_source();
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
        self.selected_source = id;
        self.selected_wav = None;
        self.loaded_wav = None;
        self.refresh_sources_ui();
        let _ = self.refresh_wavs();
    }

    /// Refresh wav entries for the current source.
    pub fn refresh_wavs(&mut self) -> Result<(), SourceDbError> {
        let Some(source) = self.current_source() else {
            self.clear_wavs();
            return Ok(());
        };
        let db = self.database_for(&source)?;
        let entries = db.list_files()?;
        self.wav_entries = entries;
        self.rebuild_wav_lookup();
        self.rebuild_triage_lists();
        self.set_status(
            format!("{} wav files loaded", self.wav_entries.len()),
            StatusTone::Info,
        );
        Ok(())
    }

    fn clear_wavs(&mut self) {
        self.wav_entries.clear();
        self.wav_lookup.clear();
        self.selected_wav = None;
        self.loaded_wav = None;
        self.ui.triage = TriageState::default();
        self.ui.loaded_wav = None;
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
        self.sources
            .iter()
            .find(|s| &s.id == selected)
            .cloned()
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
        self.ui.collections.rows = view_model::collection_rows(
            &self.collections,
            selected_id.as_ref(),
        );
        self.ui.collections.selected = selected_id.as_ref().and_then(|id| {
            self.collections.iter().position(|c| &c.id == id)
        });
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

    fn database_for(&mut self, source: &SampleSource) -> Result<Rc<SourceDatabase>, SourceDbError> {
        if let Some(existing) = self.db_cache.get(&source.id) {
            return Ok(existing.clone());
        }
        let db = Rc::new(SourceDatabase::open(&source.root)?);
        self.db_cache.insert(source.id.clone(), db.clone());
        Ok(db)
    }

    fn set_status(&mut self, text: impl Into<String>, tone: StatusTone) {
        let (label, color) = status_badge(tone);
        self.ui.status.text = text.into();
        self.ui.status.badge_label = label;
        self.ui.status.badge_color = color;
    }
}

#[derive(Clone, Copy, Debug)]
enum StatusTone {
    Idle,
    Busy,
    Info,
    Warning,
    Error,
}

fn status_badge(tone: StatusTone) -> (String, Color32) {
    match tone {
        StatusTone::Idle => ("Idle".into(), Color32::from_rgb(42, 42, 42)),
        StatusTone::Busy => ("Scanning".into(), Color32::from_rgb(31, 139, 255)),
        StatusTone::Info => ("Info".into(), Color32::from_rgb(64, 140, 112)),
        StatusTone::Warning => ("Warning".into(), Color32::from_rgb(192, 138, 43)),
        StatusTone::Error => ("Error".into(), Color32::from_rgb(192, 57, 43)),
    }
}

struct RowFlags {
    selected: bool,
    loaded: bool,
}
