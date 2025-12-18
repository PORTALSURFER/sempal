//! Controller is being integrated incrementally with the egui renderer.
//! This module now delegates responsibilities into focused submodules to
//! keep files small and behaviour easy to reason about.

mod audio_cache;
mod audio_loader;
mod audio_options;
mod analysis_jobs;
mod background_jobs;
mod browser_controller;
mod clipboard;
mod collection_export;
mod collection_items;
mod collection_items_helpers;
mod collections_controller;
mod config;
mod controller_state;
mod drag_drop_controller;
mod focus;
mod feedback_issue;
pub(crate) mod hotkeys;
mod hotkeys_controller;
mod interaction_options;
mod jobs;
mod loading;
mod os_explorer;
mod playback;
mod predictions;
mod progress;
mod scans;
mod selection_edits;
mod selection_export;
mod source_cache_invalidator;
mod source_folders;
mod sources;
mod tagging_service;
mod trash;
mod trash_move;
mod undo;
mod updates;
mod wav_entries_loader;
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
pub(in crate::egui_app::controller) use analysis_jobs::AnalysisJobMessage;
use analysis_jobs::AnalysisWorkerPool;
pub(in crate::egui_app::controller) use controller_state::*;
use egui::Color32;
use open;
use rfd::FileDialog;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, SystemTime},
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
        let (wav_job_tx, wav_job_rx) = wav_entries_loader::spawn_wav_loader();
        let (audio_job_tx, audio_job_rx) = audio_loader::spawn_audio_loader(renderer.clone());
        let (waveform_width, waveform_height) = renderer.dimensions();
        let jobs = jobs::ControllerJobs::new(wav_job_tx, wav_job_rx, audio_job_tx, audio_job_rx);
        let analysis = AnalysisWorkerPool::new();
        Self {
            ui: UiState::default(),
            audio: ControllerAudioState {
                player,
                cache: AudioCache::new(AUDIO_CACHE_CAPACITY, AUDIO_HISTORY_LIMIT),
                pending_loop_disable_at: None,
            },
            sample_view: ControllerSampleViewState {
                renderer,
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
                    predictions: HashMap::new(),
                    prediction_categories: None,
                    prediction_categories_checked: false,
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
                model: crate::sample_sources::config::ModelSettings::default(),
                updates: crate::sample_sources::config::UpdateSettings::default(),
                audio_output: AudioOutputConfig::default(),
                controls: crate::sample_sources::config::InteractionOptions::default(),
                trash_folder: None,
                collection_export_root: None,
            },
            runtime: ControllerRuntimeState {
                jobs,
                analysis,
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

#[cfg(test)]
mod collection_items_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
