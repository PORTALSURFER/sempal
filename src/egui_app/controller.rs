//! Controller is being integrated incrementally with the egui renderer.
//! This module now delegates responsibilities into focused submodules to
//! keep files small and behaviour easy to reason about.

mod library;
mod playback;
mod ui;

mod config;
pub(crate) mod controller_state;
pub(crate) mod jobs;
pub(crate) mod state;
pub(crate) mod undo;
pub(crate) mod updates;

use crate::{
    audio::AudioPlayer,
    egui_app::state::{
        FolderBrowserUiState, PlayheadState, ProgressOverlayState, SampleBrowserSort,
        SampleBrowserState, TriageFlagColumn, TriageFlagFilter, UiState, WaveformImage,
    },
    egui_app::{ui::style, view_model},
    sample_sources::scanner::ScanMode,
    sample_sources::{
        Collection, CollectionId, Rating, SampleSource, SourceDatabase, SourceDbError, SourceId,
        WavEntry,
    },
    selection::SelectionRange,
    waveform::{DecodedWaveform, WaveformRenderer},
};
pub(in crate::egui_app::controller) use library::analysis_jobs::AnalysisJobMessage;
use library::analysis_jobs::AnalysisWorkerPool;
use playback::audio_loader::{AudioLoadError, AudioLoadJob, AudioLoadOutcome};
pub(crate) use controller_state::*;
use egui::Color32;
use open;
use rfd::FileDialog;
pub(crate) use ui::hotkeys;
pub(crate) use ui::status_message::StatusMessage;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};pub(crate) use crate::egui_app::ui::style::StatusTone;


pub(crate) const MIN_SELECTION_WIDTH: f32 = 0.001;
pub(crate) const BPM_MIN_SELECTION_DIVISOR: f32 = 16.0;
pub(crate) const BPM_MIN_SELECTION_WIDTH: f32 = 0.0001;
pub(crate) const SIMILARITY_MIN_SELECTION_WIDTH: f32 = 0.002;
pub(crate) const AUDIO_CACHE_CAPACITY: usize = 12;
pub(crate) const AUDIO_HISTORY_LIMIT: usize = 8;
pub(crate) const RANDOM_HISTORY_LIMIT: usize = 20;
pub(crate) const FOCUS_HISTORY_LIMIT: usize = 100;
pub(crate) const UNDO_LIMIT: usize = 20;
pub(crate) const STATUS_LOG_LIMIT: usize = 200;

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
        let (wav_job_tx, wav_job_rx) = library::wav_entries_loader::spawn_wav_loader();
        let (audio_job_tx, audio_job_rx) = playback::audio_loader::spawn_audio_loader(renderer.clone());
        let (search_job_tx, search_job_rx) = library::wavs::browser_search_worker::spawn_search_worker();
        let jobs = jobs::ControllerJobs::new(
            wav_job_tx,
            wav_job_rx,
            audio_job_tx,
            audio_job_rx,
            search_job_tx,
            search_job_rx,
        );
        let analysis = AnalysisWorkerPool::new();
        Self {
            ui: UiState::default(),
            audio: ControllerAudioState::new(player, AUDIO_CACHE_CAPACITY, AUDIO_HISTORY_LIMIT),
            sample_view: ControllerSampleViewState::new(renderer),
            library: LibraryState::new(),
            cache: LibraryCacheState::new(),
            ui_cache: ControllerUiCacheState::new(),
            wav_entries: WavEntriesState::new(0, 1024),
            selection_state: ControllerSelectionState::new(),
            settings: AppSettingsState::new(),
            runtime: ControllerRuntimeState::new(jobs, analysis),
            history: ControllerHistoryState::new(UNDO_LIMIT),
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
        let analysis_active = self
            .ui
            .progress
            .analysis
            .as_ref()
            .is_some_and(|snapshot| snapshot.pending > 0 || snapshot.running > 0);
        let pause_claiming = (self.is_playing() || recent_input) && !analysis_active;
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
        let base_worker_count = if self.settings.analysis.analysis_worker_count == 0 {
            crate::egui_app::controller::library::analysis_jobs::default_worker_count()
        } else {
            self.settings.analysis.analysis_worker_count
        };
        let idle_target = self
            .runtime
            .performance
            .idle_worker_override
            .unwrap_or(base_worker_count);
        let target = if busy || !idle { 1 } else { idle_target };
        if pause_claiming {
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

    pub(crate) fn browser(&mut self) -> library::browser_controller::BrowserController<'_> {
        library::browser_controller::BrowserController::new(self)
    }

    pub(crate) fn waveform(&mut self) -> ui::waveform_controller::WaveformController<'_> {
        ui::waveform_controller::WaveformController::new(self)
    }

    pub(crate) fn drag_drop(&mut self) -> ui::drag_drop_controller::DragDropController<'_> {
        ui::drag_drop_controller::DragDropController::new(self)
    }

    pub(crate) fn collections_ctrl(&mut self) -> library::collections_controller::CollectionsController<'_> {
        library::collections_controller::CollectionsController::new(self)
    }

    pub(crate) fn hotkeys_ctrl(&mut self) -> ui::hotkeys_controller::HotkeysController<'_> {
        ui::hotkeys_controller::HotkeysController::new(self)
    }

    /// Returns the duration in seconds for the currently loaded audio, if any.
    pub(crate) fn loaded_audio_duration_seconds(&self) -> Option<f32> {
        self.sample_view
            .wav
            .loaded_audio
            .as_ref()
            .map(|audio| audio.duration_seconds)
    }

    pub(crate) fn is_issue_gateway_poll_in_progress(&self) -> bool {
        self.runtime.jobs.issue_gateway_poll_in_progress
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
mod test_support;
#[cfg(test)]
mod tests;
