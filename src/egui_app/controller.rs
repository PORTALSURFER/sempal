//! Controller is being integrated incrementally with the egui renderer.
//! This module now delegates responsibilities into focused submodules to
//! keep files small and behaviour easy to reason about.

mod collections;
mod config;
mod drag;
mod loading;
mod playback;
mod scans;
mod sources;
mod wavs;

use crate::{
    audio::AudioPlayer,
    egui_app::state::{
        PlayheadState, TriageColumn, TriageIndex, TriageState, UiState, WaveformImage,
    },
    egui_app::view_model,
    sample_sources::{
        Collection, CollectionId, SampleSource, SampleTag, SourceDatabase, SourceDbError, SourceId,
        WavEntry,
    },
    selection::{SelectionRange, SelectionState},
    waveform::WaveformRenderer,
};
use egui::Color32;
use rfd::FileDialog;
use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc::{Receiver, Sender},
    thread,
    time::{Duration, Instant, SystemTime},
};

/// Minimum selection width used to decide when to play a looped region.
const MIN_SELECTION_WIDTH: f32 = 0.001;

/// Maintains app state and bridges core logic to the egui UI.
pub struct EguiController {
    pub ui: UiState,
    renderer: WaveformRenderer,
    player: Option<Rc<RefCell<AudioPlayer>>>,
    sources: Vec<SampleSource>,
    collections: Vec<Collection>,
    db_cache: HashMap<SourceId, Rc<SourceDatabase>>,
    wav_cache: HashMap<SourceId, Vec<WavEntry>>,
    label_cache: HashMap<SourceId, Vec<String>>,
    wav_entries: Vec<WavEntry>,
    wav_lookup: HashMap<PathBuf, usize>,
    selected_source: Option<SourceId>,
    selected_collection: Option<CollectionId>,
    selected_wav: Option<PathBuf>,
    loaded_wav: Option<PathBuf>,
    suppress_autoplay_once: bool,
    feature_flags: crate::sample_sources::config::FeatureFlags,
    selection: SelectionState,
    wav_job_tx: Sender<WavLoadJob>,
    wav_job_rx: Receiver<WavLoadResult>,
    pending_source: Option<SourceId>,
    pending_select_path: Option<PathBuf>,
    scan_rx: Option<Receiver<ScanResult>>,
    scan_in_progress: bool,
}

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
            wav_cache: HashMap::new(),
            label_cache: HashMap::new(),
            wav_entries: Vec::new(),
            wav_lookup: HashMap::new(),
            selected_source: None,
            selected_collection: None,
            selected_wav: None,
            loaded_wav: None,
            suppress_autoplay_once: false,
            feature_flags: crate::sample_sources::config::FeatureFlags::default(),
            selection: SelectionState::new(),
            wav_job_tx,
            wav_job_rx,
            pending_source: None,
            pending_select_path: None,
            scan_rx: None,
            scan_in_progress: false,
        }
    }

    fn set_status(&mut self, text: impl Into<String>, tone: StatusTone) {
        let (label, color) = status_badge(tone);
        self.ui.status.text = text.into();
        self.ui.status.badge_label = label;
        self.ui.status.badge_color = color;
    }
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

#[derive(Clone)]
struct RowFlags {
    selected: bool,
    loaded: bool,
}

struct WavLoadJob {
    source_id: SourceId,
    root: PathBuf,
}

struct WavLoadResult {
    source_id: SourceId,
    result: Result<Vec<WavEntry>, LoadEntriesError>,
    elapsed: Duration,
}

struct ScanResult {
    source_id: SourceId,
    result: Result<
        crate::sample_sources::scanner::ScanStats,
        crate::sample_sources::scanner::ScanError,
    >,
}

fn spawn_wav_loader() -> (Sender<WavLoadJob>, Receiver<WavLoadResult>) {
    let (tx, rx) = std::sync::mpsc::channel::<WavLoadJob>();
    let (result_tx, result_rx) = std::sync::mpsc::channel::<WavLoadResult>();
    thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            let start = Instant::now();
            let result = load_entries(&job);
            let _ = result_tx.send(WavLoadResult {
                source_id: job.source_id.clone(),
                result,
                elapsed: start.elapsed(),
            });
        }
    });
    (tx, result_rx)
}

fn load_entries(job: &WavLoadJob) -> Result<Vec<WavEntry>, LoadEntriesError> {
    let db = SourceDatabase::open(&job.root).map_err(LoadEntriesError::Db)?;
    db.list_files().map_err(LoadEntriesError::Db)
}

#[derive(Debug)]
enum LoadEntriesError {
    Db(SourceDbError),
    Message(String),
}

impl std::fmt::Display for LoadEntriesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadEntriesError::Db(err) => write!(f, "{err}"),
            LoadEntriesError::Message(msg) => f.write_str(msg),
        }
    }
}

impl From<String> for LoadEntriesError {
    fn from(value: String) -> Self {
        LoadEntriesError::Message(value)
    }
}

#[cfg(test)]
mod tests;
