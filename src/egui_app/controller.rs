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
mod jobs;
mod loading;
mod os_explorer;
mod playback;
mod progress;
mod scans;
mod selection_edits;
mod selection_export;
mod source_folders;
mod source_cache_invalidator;
mod sources;
mod tagging_service;
mod trash;
mod trash_move;
mod undo;
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
const UNDO_LIMIT: usize = 20;

/// Maintains app state and bridges core logic to the egui UI.
pub struct EguiController {
    pub ui: UiState,
    renderer: WaveformRenderer,
    audio: ControllerAudioState,
    sample_view: ControllerSampleViewState,
    library: LibraryState,
    cache: LibraryCacheState,
    ui_cache: ControllerUiCacheState,
    wav_entries: WavEntriesState,
    selection_state: ControllerSelectionState,
    settings: AppSettingsState,
    runtime: ControllerRuntimeState,
    history: ControllerHistoryState,
    #[cfg(target_os = "windows")]
    drag_hwnd: Option<windows::Win32::Foundation::HWND>,
}

impl EguiController {
    /// Create a controller with shared renderer and optional audio player.
    pub fn new(renderer: WaveformRenderer, player: Option<Rc<RefCell<AudioPlayer>>>) -> Self {
        let (wav_job_tx, wav_job_rx) = spawn_wav_loader();
        let (audio_job_tx, audio_job_rx) = audio_loader::spawn_audio_loader(renderer.clone());
        let (waveform_width, waveform_height) = renderer.dimensions();
        let jobs = jobs::ControllerJobs {
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
            trash_move_rx: None,
            trash_move_cancel: None,
        };
        Self {
            ui: UiState::default(),
            renderer,
            audio: ControllerAudioState {
                player,
                cache: AudioCache::new(AUDIO_CACHE_CAPACITY, AUDIO_HISTORY_LIMIT),
                pending_loop_disable_at: None,
            },
            sample_view: ControllerSampleViewState {
                waveform: WaveformState {
                    size: [waveform_width, waveform_height],
                    decoded: None,
                    render_meta: None,
                },
                wav: WavSelectionState {
                    selected_wav: None,
                    loaded_wav: None,
                    loaded_audio: None,
                },
            },
            library: LibraryState {
                sources: Vec::new(),
                collections: Vec::new(),
                missing: MissingState {
                    sources: HashSet::new(),
                    wavs: HashMap::new(),
                },
            },
            cache: LibraryCacheState {
                db: HashMap::new(),
                wav: WavCacheState {
                    entries: HashMap::new(),
                    lookup: HashMap::new(),
                },
            },
            ui_cache: ControllerUiCacheState {
                browser: BrowserCacheState {
                    labels: HashMap::new(),
                    search: wavs::BrowserSearchCache::default(),
                },
                folders: FolderBrowsersState {
                    models: HashMap::new(),
                },
            },
            wav_entries: WavEntriesState {
                entries: Vec::new(),
                lookup: HashMap::new(),
            },
            selection_state: ControllerSelectionState {
                ctx: SelectionContextState {
                    selected_source: None,
                    last_selected_browsable_source: None,
                    selected_collection: None,
                },
                range: SelectionState::new(),
                suppress_autoplay_once: false,
            },
            settings: AppSettingsState {
                feature_flags: crate::sample_sources::config::FeatureFlags::default(),
                audio_output: AudioOutputConfig::default(),
                controls: crate::sample_sources::config::InteractionOptions::default(),
                trash_folder: None,
                collection_export_root: None,
            },
            runtime: ControllerRuntimeState {
                jobs,
                #[cfg(test)]
                progress_cancel_after: None,
            },
            history: ControllerHistoryState {
                undo_stack: undo::UndoStack::new(UNDO_LIMIT),
                random_history: RandomHistoryState {
                    entries: VecDeque::new(),
                    cursor: None,
                },
            },
            #[cfg(target_os = "windows")]
            drag_hwnd: None,
        }
    }

    #[cfg(target_os = "windows")]
    pub fn set_drag_hwnd(&mut self, hwnd: Option<windows::Win32::Foundation::HWND>) {
        self.drag_hwnd = hwnd;
    }

    pub(crate) fn set_status(&mut self, text: impl Into<String>, tone: StatusTone) {
        let (label, color) = status_badge(tone);
        self.ui.status.text = text.into();
        self.ui.status.badge_label = label;
        self.ui.status.badge_color = color;
    }

    #[allow(dead_code)]
    pub(crate) fn can_undo(&self) -> bool {
        self.history.undo_stack.can_undo()
    }

    #[allow(dead_code)]
    pub(crate) fn can_redo(&self) -> bool {
        self.history.undo_stack.can_redo()
    }

    pub(crate) fn undo(&mut self) {
        let mut stack = std::mem::replace(
            &mut self.history.undo_stack,
            undo::UndoStack::new(UNDO_LIMIT),
        );
        let result = stack.undo(self);
        self.history.undo_stack = stack;
        match result {
            Ok(Some(label)) => self.set_status(format!("Undid {label}"), StatusTone::Info),
            Ok(None) => self.set_status("Nothing to undo", StatusTone::Info),
            Err(err) => self.set_status(format!("Undo failed: {err}"), StatusTone::Error),
        }
    }

    pub(crate) fn redo(&mut self) {
        let mut stack = std::mem::replace(
            &mut self.history.undo_stack,
            undo::UndoStack::new(UNDO_LIMIT),
        );
        let result = stack.redo(self);
        self.history.undo_stack = stack;
        match result {
            Ok(Some(label)) => self.set_status(format!("Redid {label}"), StatusTone::Info),
            Ok(None) => self.set_status("Nothing to redo", StatusTone::Info),
            Err(err) => self.set_status(format!("Redo failed: {err}"), StatusTone::Error),
        }
    }

    pub(crate) fn push_undo_entry(&mut self, entry: undo::UndoEntry<EguiController>) {
        self.history.undo_stack.push(entry);
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

struct MissingState {
    sources: HashSet<SourceId>,
    wavs: HashMap<SourceId, HashSet<PathBuf>>,
}

struct LibraryState {
    sources: Vec<SampleSource>,
    collections: Vec<Collection>,
    missing: MissingState,
}

struct WavCacheState {
    entries: HashMap<SourceId, Vec<WavEntry>>,
    lookup: HashMap<SourceId, HashMap<PathBuf, usize>>,
}

struct WavSelectionState {
    selected_wav: Option<PathBuf>,
    loaded_wav: Option<PathBuf>,
    loaded_audio: Option<LoadedAudio>,
}

struct ControllerSampleViewState {
    waveform: WaveformState,
    wav: WavSelectionState,
}

struct SelectionContextState {
    selected_source: Option<SourceId>,
    last_selected_browsable_source: Option<SourceId>,
    selected_collection: Option<CollectionId>,
}

struct AppSettingsState {
    feature_flags: crate::sample_sources::config::FeatureFlags,
    audio_output: AudioOutputConfig,
    controls: crate::sample_sources::config::InteractionOptions,
    trash_folder: Option<std::path::PathBuf>,
    collection_export_root: Option<PathBuf>,
}

struct LibraryCacheState {
    db: HashMap<SourceId, Rc<SourceDatabase>>,
    wav: WavCacheState,
}

struct BrowserCacheState {
    labels: HashMap<SourceId, Vec<String>>,
    search: wavs::BrowserSearchCache,
}

struct FolderBrowsersState {
    models: HashMap<SourceId, source_folders::FolderBrowserModel>,
}

struct ControllerUiCacheState {
    browser: BrowserCacheState,
    folders: FolderBrowsersState,
}

struct ControllerSelectionState {
    ctx: SelectionContextState,
    range: SelectionState,
    suppress_autoplay_once: bool,
}

struct ControllerAudioState {
    player: Option<Rc<RefCell<AudioPlayer>>>,
    cache: AudioCache,
    pending_loop_disable_at: Option<Instant>,
}

struct ControllerRuntimeState {
    jobs: jobs::ControllerJobs,
    #[cfg(test)]
    progress_cancel_after: Option<usize>,
}

struct ControllerHistoryState {
    undo_stack: undo::UndoStack<EguiController>,
    random_history: RandomHistoryState,
}

struct WavEntriesState {
    entries: Vec<WavEntry>,
    lookup: HashMap<PathBuf, usize>,
}

struct WaveformState {
    size: [u32; 2],
    decoded: Option<DecodedWaveform>,
    render_meta: Option<crate::egui_app::controller::wavs::WaveformRenderMeta>,
}

#[derive(Clone)]
struct RandomHistoryEntry {
    source_id: SourceId,
    relative_path: PathBuf,
}

struct RandomHistoryState {
    entries: VecDeque<RandomHistoryEntry>,
    cursor: Option<usize>,
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
