//! Controller is being integrated incrementally with the egui renderer.
//! This module now delegates responsibilities into focused submodules to
//! keep files small and behaviour easy to reason about.

mod audio_cache;
mod audio_loader;
mod audio_options;
mod browser_controller;
mod clipboard;
mod collection_export;
mod collection_items;
mod collection_items_helpers;
mod collections_controller;
mod config;
mod drag_drop_controller;
mod focus;
pub(crate) mod hotkeys;
mod hotkeys_controller;
mod interaction_options;
mod loading;
mod playback;
mod progress;
mod scans;
mod selection_edits;
mod selection_export;
mod source_folders;
mod sources;
mod trash;
mod waveform_controller;
mod wavs;

use crate::{
    audio::{AudioOutputConfig, AudioPlayer},
    egui_app::state::{
        FolderBrowserUiState, PlayheadState, ProgressOverlayState, SampleBrowserState,
        TriageFlagColumn, TriageFlagFilter, UiState, WaveformImage,
    },
    egui_app::{ui::style, ui::style::StatusTone, view_model},
    sample_sources::scanner::ScanMode,
    sample_sources::{
        Collection, CollectionId, SampleSource, SampleTag, SourceDatabase, SourceDbError, SourceId,
        WavEntry,
    },
    selection::{SelectionRange, SelectionState},
    waveform::{DecodedWaveform, WaveformRenderer},
};
use audio_cache::AudioCache;
use audio_loader::{AudioLoadError, AudioLoadJob, AudioLoadOutcome, AudioLoadResult};
use egui::Color32;
use open;
use rfd::FileDialog;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc::{Receiver, Sender},
    thread,
    time::{Duration, Instant, SystemTime},
};

/// Minimum selection width used to decide when to play a looped region.
const MIN_SELECTION_WIDTH: f32 = 0.001;
const AUDIO_CACHE_CAPACITY: usize = 12;
const AUDIO_HISTORY_LIMIT: usize = 8;
const RANDOM_HISTORY_LIMIT: usize = 20;

/// Maintains app state and bridges core logic to the egui UI.
pub struct EguiController {
    pub ui: UiState,
    renderer: WaveformRenderer,
    waveform_size: [u32; 2],
    decoded_waveform: Option<DecodedWaveform>,
    player: Option<Rc<RefCell<AudioPlayer>>>,
    sources: Vec<SampleSource>,
    missing_sources: HashSet<SourceId>,
    collections: Vec<Collection>,
    db_cache: HashMap<SourceId, Rc<SourceDatabase>>,
    wav_cache: HashMap<SourceId, Vec<WavEntry>>,
    missing_wavs: HashMap<SourceId, HashSet<PathBuf>>,
    label_cache: HashMap<SourceId, Vec<String>>,
    audio_cache: AudioCache,
    wav_entries: Vec<WavEntry>,
    wav_lookup: HashMap<PathBuf, usize>,
    selected_source: Option<SourceId>,
    last_selected_browsable_source: Option<SourceId>,
    selected_collection: Option<CollectionId>,
    selected_wav: Option<PathBuf>,
    loaded_wav: Option<PathBuf>,
    loaded_audio: Option<LoadedAudio>,
    waveform_render_meta: Option<crate::egui_app::controller::wavs::WaveformRenderMeta>,
    suppress_autoplay_once: bool,
    pending_loop_disable_at: Option<Instant>,
    feature_flags: crate::sample_sources::config::FeatureFlags,
    audio_output: AudioOutputConfig,
    controls: crate::sample_sources::config::InteractionOptions,
    trash_folder: Option<std::path::PathBuf>,
    collection_export_root: Option<PathBuf>,
    selection: SelectionState,
    wav_job_tx: Sender<WavLoadJob>,
    wav_job_rx: Receiver<WavLoadResult>,
    audio_job_tx: Sender<AudioLoadJob>,
    audio_job_rx: Receiver<AudioLoadResult>,
    pending_source: Option<SourceId>,
    pending_select_path: Option<PathBuf>,
    pending_audio: Option<PendingAudio>,
    pending_playback: Option<PendingPlayback>,
    next_audio_request_id: u64,
    scan_rx: Option<Receiver<ScanResult>>,
    scan_in_progress: bool,
    random_history: VecDeque<RandomHistoryEntry>,
    random_history_cursor: Option<usize>,
    folder_browsers: HashMap<SourceId, source_folders::FolderBrowserModel>,
    #[cfg(target_os = "windows")]
    drag_hwnd: Option<windows::Win32::Foundation::HWND>,
    #[cfg(test)]
    progress_cancel_after: Option<usize>,
}

impl EguiController {
    /// Create a controller with shared renderer and optional audio player.
    pub fn new(renderer: WaveformRenderer, player: Option<Rc<RefCell<AudioPlayer>>>) -> Self {
        let (wav_job_tx, wav_job_rx) = spawn_wav_loader();
        let (audio_job_tx, audio_job_rx) = audio_loader::spawn_audio_loader(renderer.clone());
        let (waveform_width, waveform_height) = renderer.dimensions();
        Self {
            ui: UiState::default(),
            renderer,
            waveform_size: [waveform_width, waveform_height],
            decoded_waveform: None,
            player,
            sources: Vec::new(),
            missing_sources: HashSet::new(),
            collections: Vec::new(),
            db_cache: HashMap::new(),
            wav_cache: HashMap::new(),
            missing_wavs: HashMap::new(),
            label_cache: HashMap::new(),
            audio_cache: AudioCache::new(AUDIO_CACHE_CAPACITY, AUDIO_HISTORY_LIMIT),
            wav_entries: Vec::new(),
            wav_lookup: HashMap::new(),
            selected_source: None,
            last_selected_browsable_source: None,
            selected_collection: None,
            selected_wav: None,
            loaded_wav: None,
            loaded_audio: None,
            waveform_render_meta: None,
            suppress_autoplay_once: false,
            pending_loop_disable_at: None,
            feature_flags: crate::sample_sources::config::FeatureFlags::default(),
            audio_output: AudioOutputConfig::default(),
            controls: crate::sample_sources::config::InteractionOptions::default(),
            trash_folder: None,
            collection_export_root: None,
            selection: SelectionState::new(),
            wav_job_tx,
            wav_job_rx,
            audio_job_tx,
            audio_job_rx,
            pending_source: None,
            pending_select_path: None,
            pending_audio: None,
            pending_playback: None,
            next_audio_request_id: 1,
            scan_rx: None,
            scan_in_progress: false,
            random_history: VecDeque::new(),
            random_history_cursor: None,
            folder_browsers: HashMap::new(),
            #[cfg(target_os = "windows")]
            drag_hwnd: None,
            #[cfg(test)]
            progress_cancel_after: None,
        }
    }

    #[cfg(target_os = "windows")]
    pub fn set_drag_hwnd(&mut self, hwnd: Option<windows::Win32::Foundation::HWND>) {
        self.drag_hwnd = hwnd;
    }

    #[cfg(not(target_os = "windows"))]
    pub fn set_drag_hwnd(&mut self, _hwnd: Option<()>) {}

    pub(crate) fn set_status(&mut self, text: impl Into<String>, tone: StatusTone) {
        let (label, color) = status_badge(tone);
        self.ui.status.text = text.into();
        self.ui.status.badge_label = label;
        self.ui.status.badge_color = color;
    }

    pub(crate) fn browser(&mut self) -> browser_controller::BrowserController<'_> {
        browser_controller::BrowserController::new(self)
    }

    pub(crate) fn waveform(&mut self) -> waveform_controller::WaveformController<'_> {
        waveform_controller::WaveformController::new(self)
    }

    pub(crate) fn drag_drop(&mut self) -> drag_drop_controller::DragDropController<'_> {
        drag_drop_controller::DragDropController::new(self)
    }

    pub(crate) fn collections_ctrl(&mut self) -> collections_controller::CollectionsController<'_> {
        collections_controller::CollectionsController::new(self)
    }

    pub(crate) fn hotkeys_ctrl(&mut self) -> hotkeys_controller::HotkeysController<'_> {
        hotkeys_controller::HotkeysController::new(self)
    }
}

/// UI status tone for badge coloring.
fn status_badge(tone: StatusTone) -> (String, Color32) {
    match tone {
        StatusTone::Idle => ("Idle".into(), style::status_badge_color(StatusTone::Idle)),
        StatusTone::Busy => (
            "Working".into(),
            style::status_badge_color(StatusTone::Busy),
        ),
        StatusTone::Info => ("Info".into(), style::status_badge_color(StatusTone::Info)),
        StatusTone::Warning => (
            "Warning".into(),
            style::status_badge_color(StatusTone::Warning),
        ),
        StatusTone::Error => ("Error".into(), style::status_badge_color(StatusTone::Error)),
    }
}

#[derive(Clone)]
struct RowFlags {
    focused: bool,
    loaded: bool,
}

#[derive(Clone)]
struct RandomHistoryEntry {
    source_id: SourceId,
    relative_path: PathBuf,
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

#[derive(Clone)]
struct PendingAudio {
    request_id: u64,
    source_id: SourceId,
    root: PathBuf,
    relative_path: PathBuf,
    intent: AudioLoadIntent,
}

#[derive(Clone)]
struct PendingPlayback {
    source_id: SourceId,
    relative_path: PathBuf,
    looped: bool,
    start_override: Option<f32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AudioLoadIntent {
    Selection,
    CollectionPreview,
}

struct ScanResult {
    source_id: SourceId,
    mode: ScanMode,
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
    let mut entries = db.list_files().map_err(LoadEntriesError::Db)?;
    if entries.is_empty() {
        // New sources start empty; trigger a quick scan to populate before reporting.
        let _ = crate::sample_sources::scanner::scan_once(&db);
        entries = db.list_files().map_err(LoadEntriesError::Db)?;
    }
    Ok(entries)
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

#[derive(Clone)]
struct LoadedAudio {
    source_id: SourceId,
    relative_path: PathBuf,
    bytes: Vec<u8>,
    duration_seconds: f32,
    sample_rate: u32,
    channels: u16,
}

#[cfg(test)]
mod collection_items_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
