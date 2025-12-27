//! Controller is being integrated incrementally with the egui renderer.
//! This module now delegates responsibilities into focused submodules to
//! keep files small and behaviour easy to reason about.

mod analysis_backfill;
mod analysis_jobs;
mod analysis_options;
mod audio_cache;
mod audio_loader;
mod audio_options;
mod background_jobs;
mod browser_controller;
mod clipboard;
mod collection_export;
mod collection_items;
mod collection_items_helpers;
mod collections_controller;
mod config;
pub(crate) mod controller_state;
mod drag_drop_controller;
mod feedback_issue;
mod focus;
pub(crate) mod hotkeys;
mod hotkeys_controller;
mod interaction_options;
mod jobs;
mod loading;
mod map_view;
mod missing_samples;
mod os_explorer;
mod playback;
mod progress;
mod progress_messages;
mod scans;
mod selection_edits;
mod selection_export;
mod similarity_prep;
mod source_cache_invalidator;
mod source_folders;
mod sources;
mod status_message;
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
pub(in crate::egui_app::controller) use analysis_jobs::AnalysisJobMessage;
use analysis_jobs::AnalysisWorkerPool;
use audio_cache::AudioCache;
use audio_loader::{AudioLoadError, AudioLoadJob, AudioLoadOutcome, AudioLoadResult};
pub(in crate::egui_app::controller) use controller_state::*;
use egui::Color32;
use open;
use rfd::FileDialog;
pub(crate) use status_message::StatusMessage;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

/// Minimum selection width used to decide when to play a looped region.
const MIN_SELECTION_WIDTH: f32 = 0.001;
const AUDIO_CACHE_CAPACITY: usize = 12;
const AUDIO_HISTORY_LIMIT: usize = 8;
const RANDOM_HISTORY_LIMIT: usize = 20;
const UNDO_LIMIT: usize = 20;
const STATUS_LOG_LIMIT: usize = 200;

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
                },
            },
            ui_cache: ControllerUiCacheState {
                browser: BrowserCacheState {
                    labels: HashMap::new(),
                    analysis_failures: HashMap::new(),
                    search: wavs::BrowserSearchCache::default(),
                    features: HashMap::new(),
                },
                folders: FolderBrowsersState {
                    models: HashMap::new(),
                },
            },
            wav_entries: WavEntriesState::new(0, 1024),
            selection_state: ControllerSelectionState {
                ctx: SelectionContextState {
                    selected_source: None,
                    last_selected_browsable_source: None,
                    selected_collection: None,
                },
                range: SelectionState::new(),
                pending_undo: None,
                suppress_autoplay_once: false,
                bpm_scale_beats: None,
            },
            settings: AppSettingsState {
                feature_flags: crate::sample_sources::config::FeatureFlags::default(),
                analysis: crate::sample_sources::config::AnalysisSettings::default(),
                updates: crate::sample_sources::config::UpdateSettings::default(),
                app_data_dir: None,
                audio_output: AudioOutputConfig::default(),
                controls: crate::sample_sources::config::InteractionOptions::default(),
                trash_folder: None,
                collection_export_root: None,
            },
            runtime: ControllerRuntimeState {
                jobs,
                analysis,
                performance: PerformanceGovernorState {
                    last_user_activity_at: None,
                    last_slow_frame_at: None,
                    last_frame_at: None,
                    last_worker_count: None,
                    idle_worker_override: None,
                },
                similarity_prep: None,
                similarity_prep_last_error: None,
                similarity_prep_force_full_analysis_next: false,
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

    pub(crate) fn update_performance_governor(&mut self, user_active: bool) {
        const ACTIVE_WINDOW: Duration = Duration::from_millis(300);
        const IDLE_WINDOW: Duration = Duration::from_secs(2);
        const SLOW_FRAME_THRESHOLD: Duration = Duration::from_millis(40);
        let now = Instant::now();
        if let Some(last_frame) = self.runtime.performance.last_frame_at {
            let frame_delta = now.saturating_duration_since(last_frame);
            if frame_delta >= SLOW_FRAME_THRESHOLD {
                self.runtime.performance.last_slow_frame_at = Some(now);
            }
        }
        self.runtime.performance.last_frame_at = Some(now);
        if user_active {
            self.runtime.performance.last_user_activity_at = Some(now);
        }
        let recent_input = self
            .runtime
            .performance
            .last_user_activity_at
            .is_some_and(|time| now.saturating_duration_since(time) <= ACTIVE_WINDOW);
        let recent_slow_frame = self
            .runtime
            .performance
            .last_slow_frame_at
            .is_some_and(|time| now.saturating_duration_since(time) <= ACTIVE_WINDOW);
        let busy = self.is_playing() || recent_input || recent_slow_frame;
        let last_activity_at = match (
            self.runtime.performance.last_user_activity_at,
            self.runtime.performance.last_slow_frame_at,
        ) {
            (Some(input), Some(slow)) => Some(input.max(slow)),
            (Some(input), None) => Some(input),
            (None, Some(slow)) => Some(slow),
            (None, None) => None,
        };
        let idle = !self.is_playing()
            && last_activity_at
                .is_some_and(|time| now.saturating_duration_since(time) >= IDLE_WINDOW);
        let idle_target = self
            .runtime
            .performance
            .idle_worker_override
            .unwrap_or(self.settings.analysis.analysis_worker_count);
        let target = if busy || !idle { 1 } else { idle_target };
        if busy {
            self.runtime.analysis.pause_claiming();
        } else {
            self.runtime.analysis.resume_claiming();
        }
        if self.runtime.performance.last_worker_count != Some(target) {
            self.runtime.analysis.set_worker_count(target);
            self.runtime.performance.last_worker_count = Some(target);
        }
    }

    #[cfg(target_os = "windows")]
    pub fn set_drag_hwnd(&mut self, hwnd: Option<windows::Win32::Foundation::HWND>) {
        self.drag_hwnd = hwnd;
    }

    pub(crate) fn set_status(&mut self, text: impl Into<String>, tone: StatusTone) {
        let (label, color) = status_badge(tone);
        let text = text.into();
        self.ui.status.text = text.clone();
        self.ui.status.badge_label = label;
        self.ui.status.badge_color = color;
        let entry = format!("[{}] {}", self.ui.status.badge_label, text);
        if self.ui.status.log.last().is_some_and(|last| last == &entry) {
            return;
        }
        self.ui.status.log.push(entry);
        if self.ui.status.log.len() > STATUS_LOG_LIMIT {
            let overflow = self.ui.status.log.len() - STATUS_LOG_LIMIT;
            self.ui.status.log.drain(0..overflow);
        }
        log_status_entry(tone, self.ui.status.log.last().expect("just pushed"));
    }

    pub(crate) fn set_status_message(&mut self, message: StatusMessage) {
        let (text, tone) = message.into_text_and_tone();
        self.set_status(text, tone);
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

    pub(crate) fn begin_selection_undo(&mut self, label: impl Into<String>) {
        if self.selection_state.pending_undo.is_some() {
            return;
        }
        let before = self
            .selection_state
            .range
            .range()
            .or(self.ui.waveform.selection);
        self.selection_state.pending_undo = Some(SelectionUndoState {
            label: label.into(),
            before,
        });
    }

    pub(crate) fn commit_selection_undo(&mut self) {
        let Some(pending) = self.selection_state.pending_undo.take() else {
            return;
        };
        let after = self
            .selection_state
            .range
            .range()
            .or(self.ui.waveform.selection);
        self.push_selection_undo(pending.label, pending.before, after);
    }

    pub(crate) fn push_selection_undo(
        &mut self,
        label: impl Into<String>,
        before: Option<SelectionRange>,
        after: Option<SelectionRange>,
    ) {
        if before == after {
            return;
        }
        let label = label.into();
        self.push_undo_entry(undo::UndoEntry::<EguiController>::new(
            label,
            move |controller| {
                controller.selection_state.range.set_range(before);
                controller.apply_selection(before);
                Ok(())
            },
            move |controller| {
                controller.selection_state.range.set_range(after);
                controller.apply_selection(after);
                Ok(())
            },
        ));
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

    /// Returns the duration in seconds for the currently loaded audio, if any.
    pub(crate) fn loaded_audio_duration_seconds(&self) -> Option<f32> {
        self.sample_view
            .wav
            .loaded_audio
            .as_ref()
            .map(|audio| audio.duration_seconds)
    }
}

fn log_status_entry(tone: StatusTone, entry: &str) {
    match tone {
        StatusTone::Warning => tracing::warn!("{entry}"),
        StatusTone::Error => tracing::error!("{entry}"),
        StatusTone::Info | StatusTone::Busy | StatusTone::Idle => tracing::info!("{entry}"),
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
