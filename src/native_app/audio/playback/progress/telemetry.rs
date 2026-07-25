#[cfg(test)]
use std::cell::Cell;
use std::time::Duration;
#[cfg(test)]
use std::time::Instant;

use radiant::runtime::{RepaintScope, SurfaceRevisions};

use super::super::diagnostics::PlayheadFrameMessageDiagnostics;
use crate::native_app::{
    app::{
        NativeAppState, SamplePlaybackIntent, SamplePlaybackSession, SamplePlaybackSessionState,
    },
    starmap_audition_telemetry::{
        self as starmap_telemetry, StarmapAuditionCounter, StarmapAuditionDuration,
    },
};

#[cfg(test)]
thread_local! {
    static BROAD_FRAME_REVISION_OBSERVATIONS: Cell<u64> = const { Cell::new(0) };
}

#[derive(Default)]
/// Converts frame-relevant application state into monotonic runtime revision keys.
pub(in crate::native_app) struct FrameSurfaceRevisionTracker {
    last: Option<FrameSurfaceRevisionInputs>,
    revisions: SurfaceRevisions,
    transient_fast_path_active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FrameSurfaceRevisionInputs {
    structure: FrameStructureState,
    layout: FrameLayoutState,
    projection: FrameProjectionState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameStructureState {
    playing: bool,
    drag_hover_auto_expand_pending: bool,
    folder_progress_active: bool,
    normalization_progress_active: bool,
    file_move_progress_active: bool,
    source_cache_progress_active: bool,
    waveform_loading_active: bool,
    sample_loading: bool,
    audio_opening: bool,
    audio_settings_error_active: bool,
    startup_source_scan_pending: bool,
    startup_auto_load_pending: bool,
    pending_playback_start: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameLayoutState {
    audio_output_sample_rate: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameProjectionState {
    playback_visual_generation: u64,
    play_selection_flash_active: bool,
    copy_flash_frames: u8,
    selection_flash_frames: u8,
    protected_source_error_flash_frames: u8,
    keyboard_focus_alpha: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FrameSurfaceRevisionGuard {
    before: Option<TransientFrameRevisionInputs>,
    forced_surface: bool,
    starmap_retained_scene_active_before_message: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransientFrameRevisionInputs {
    structure: TransientFrameStructureState,
    layout: FrameLayoutState,
    projection: TransientFrameProjectionState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransientFrameStructureState {
    drag_hover_auto_expand_pending: bool,
    source_cache_progress_active: bool,
    sample_loading: bool,
    audio_opening: bool,
    audio_settings_error_active: bool,
    pending_playback_start: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransientFrameProjectionState {
    playback_visual_generation: u64,
    play_selection_flash_active: bool,
    copy_flash_frames: u8,
    selection_flash_frames: u8,
    protected_source_error_flash_frames: u8,
    keyboard_focus_alpha: u8,
    progress_tick_bits: u32,
}

impl NativeAppState {
    pub(in crate::native_app) fn frame_surface_revisions(&mut self) -> SurfaceRevisions {
        let transient_active = self.transient_frame_fast_path_active();
        if transient_active
            || self
                .frame_surface_revision_tracker
                .transient_fast_path_active
        {
            if transient_active {
                self.frame_surface_revision_tracker
                    .transient_fast_path_active = true;
            }
            return self.frame_surface_revision_tracker.revisions;
        }
        let inputs = FrameSurfaceRevisionInputs::from_state(self);
        self.frame_surface_revision_tracker.observe(inputs)
    }

    #[cfg(test)]
    pub(in crate::native_app) fn capture_frame_surface_inputs(&self) -> FrameSurfaceRevisionInputs {
        FrameSurfaceRevisionInputs::from_state(self)
    }

    #[cfg(test)]
    pub(in crate::native_app) fn capture_frame_surface_inputs_at(
        &self,
        now: Instant,
    ) -> FrameSurfaceRevisionInputs {
        FrameSurfaceRevisionInputs::from_state_at(self, now)
    }

    #[cfg(test)]
    pub(in crate::native_app) fn frame_scope_since_at(
        &self,
        before: FrameSurfaceRevisionInputs,
        now: Instant,
    ) -> RepaintScope {
        FrameSurfaceRevisionInputs::from_state_at(self, now).repaint_scope_since(before)
    }

    #[cfg(test)]
    pub(in crate::native_app) fn frame_can_use_paint_only_since(
        &self,
        before: FrameSurfaceRevisionInputs,
    ) -> bool {
        self.frame_scope_since(before).is_paint_only()
    }

    #[cfg(test)]
    pub(in crate::native_app) fn frame_scope_since(
        &self,
        before: FrameSurfaceRevisionInputs,
    ) -> RepaintScope {
        FrameSurfaceRevisionInputs::from_state(self).repaint_scope_since(before)
    }

    pub(in crate::native_app) fn begin_frame_surface_revision_tracking(
        &mut self,
    ) -> FrameSurfaceRevisionGuard {
        let transient_active = self.transient_frame_fast_path_active();
        let forced_surface = self
            .frame_surface_revision_tracker
            .transient_fast_path_active
            && !transient_active;
        if forced_surface {
            self.frame_surface_revision_tracker
                .bump(RepaintScope::Surface);
            self.frame_surface_revision_tracker
                .transient_fast_path_active = false;
        }
        FrameSurfaceRevisionGuard {
            before: transient_active.then(|| TransientFrameRevisionInputs::from_state(self)),
            forced_surface,
            starmap_retained_scene_active_before_message: self.starmap_retained_scene_active(),
        }
    }

    pub(in crate::native_app) fn finish_frame_surface_revision_tracking(
        &mut self,
        guard: FrameSurfaceRevisionGuard,
    ) {
        let transient_active = self.transient_frame_fast_path_active();
        let after = transient_active.then(|| TransientFrameRevisionInputs::from_state(self));
        let starmap_retained_scene_touched_frame = guard
            .starmap_retained_scene_active_before_message
            || self.starmap_retained_scene_active();
        let keyboard_focus_alpha_changed =
            guard.before.zip(after).is_some_and(|(before, after)| {
                before.projection.keyboard_focus_alpha != after.projection.keyboard_focus_alpha
            });
        let scope = if starmap_retained_scene_touched_frame {
            if keyboard_focus_alpha_changed {
                RepaintScope::Projection
            } else {
                RepaintScope::PaintOnly
            }
        } else if guard.forced_surface || guard.before.is_some() != after.is_some() {
            RepaintScope::Surface
        } else {
            match (guard.before, after) {
                (Some(before), Some(after)) => after.repaint_scope_since(before),
                (None, None) => RepaintScope::PaintOnly,
                _ => unreachable!("playback presence mismatch handled above"),
            }
        };
        if !guard.forced_surface {
            self.frame_surface_revision_tracker.bump(scope);
        }
        self.frame_surface_revision_tracker
            .transient_fast_path_active = transient_active;
        if guard.before.is_some() || after.is_some() || guard.forced_surface {
            self.playhead_frame_diagnostics
                .record_frame_message(PlayheadFrameMessageDiagnostics {
                    paint_only: scope.is_paint_only(),
                    reason: frame_repaint_reason(scope),
                });
        }
    }

    #[cfg(test)]
    pub(in crate::native_app) fn broad_frame_revision_observations() -> u64 {
        BROAD_FRAME_REVISION_OBSERVATIONS.get()
    }

    fn transient_frame_fast_path_active(&self) -> bool {
        self.playback_visual_activity_active() || self.starmap_retained_scene_active()
    }

    pub(in crate::native_app) fn starmap_retained_scene_active(&self) -> bool {
        self.ui.chrome.starmap_audition_drag.is_some()
            || self
                .audio
                .sample_playback_session
                .as_ref()
                .is_some_and(|session| {
                    session.request.intent == SamplePlaybackIntent::StarmapDrag
                        && !matches!(session.state, SamplePlaybackSessionState::Failed(_))
                })
            || self
                .audio
                .pending_sample_playback
                .as_ref()
                .is_some_and(|request| request.intent == SamplePlaybackIntent::StarmapDrag)
    }

    pub(in crate::native_app) fn observe_playhead_native_frame_diagnostics(
        &mut self,
        diagnostics: radiant::runtime::NativeFrameDiagnostics,
    ) {
        self.playhead_frame_diagnostics
            .observe_native_frame(diagnostics);
    }
}

pub(super) fn log_runtime_playback_event(
    stage: &'static str,
    outcome: &'static str,
    starmap_counter: Option<StarmapAuditionCounter>,
    session: &SamplePlaybackSession,
    elapsed: Option<Duration>,
    error: Option<&str>,
) {
    if !starmap_telemetry::enabled() {
        return;
    }
    tracing::info!(
        target: "perf::audio_start",
        module = "wavecrate_native_playback",
        stage,
        outcome,
        origin = session.request.origin,
        source_kind = session.source_kind,
        path = %session.request.path,
        start_ratio = session.request.span.0,
        end_ratio = session.request.span.1,
        elapsed_ms = elapsed.map(duration_ms).unwrap_or(0.0),
        error = error.unwrap_or_default(),
        "Native playback runtime event"
    );
    if session.request.origin != "starmap_drag" {
        return;
    }
    if let Some(elapsed) = elapsed {
        starmap_telemetry::record_duration(StarmapAuditionDuration::RuntimeStart, elapsed);
    }
    starmap_telemetry::record_event(
        starmap_counter,
        stage,
        outcome,
        Some(session.request.path.as_str()),
        0,
        0,
        true,
        elapsed,
    );
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

impl FrameSurfaceRevisionTracker {
    fn observe(&mut self, current: FrameSurfaceRevisionInputs) -> SurfaceRevisions {
        #[cfg(test)]
        BROAD_FRAME_REVISION_OBSERVATIONS
            .set(BROAD_FRAME_REVISION_OBSERVATIONS.get().wrapping_add(1));
        let scope = self.last.map_or(RepaintScope::PaintOnly, |previous| {
            current.repaint_scope_since(previous)
        });
        self.bump(scope);
        self.last = Some(current);
        self.revisions
    }

    fn bump(&mut self, scope: RepaintScope) {
        match scope {
            RepaintScope::Surface => {
                self.revisions.structure = self.revisions.structure.wrapping_add(1);
            }
            RepaintScope::Layout => {
                self.revisions.layout = self.revisions.layout.wrapping_add(1);
            }
            RepaintScope::Projection => {
                self.revisions.projection = self.revisions.projection.wrapping_add(1);
            }
            RepaintScope::PaintOnly => {}
        }
    }
}

impl FrameSurfaceRevisionInputs {
    fn from_state(state: &NativeAppState) -> Self {
        Self::from_state_with_keyboard_focus_alpha(
            state,
            state.library.folder_browser.keyboard_focus_alpha(),
        )
    }

    #[cfg(test)]
    fn from_state_at(state: &NativeAppState, now: Instant) -> Self {
        Self::from_state_with_keyboard_focus_alpha(
            state,
            state.library.folder_browser.keyboard_focus_alpha_at(now),
        )
    }

    fn from_state_with_keyboard_focus_alpha(
        state: &NativeAppState,
        keyboard_focus_alpha: u8,
    ) -> Self {
        Self {
            structure: FrameStructureState {
                playing: state.playback_visual_activity_active(),
                drag_hover_auto_expand_pending: state
                    .library
                    .folder_browser
                    .drag_hover_auto_expand_pending(),
                folder_progress_active: state.library.folder_scan_active(),
                normalization_progress_active: state.background.normalization_progress.is_some(),
                file_move_progress_active: state.background.file_move_progress.is_some(),
                source_cache_progress_active: state
                    .waveform
                    .cache
                    .active_folder_warm_folder_id
                    .is_some(),
                waveform_loading_active: state.waveform.load.label.is_some(),
                sample_loading: state.active_sample_load_task().is_some(),
                audio_opening: state.background.audio_open.active().is_some(),
                audio_settings_error_active: state.audio.settings_error.is_some(),
                startup_source_scan_pending: state.ui.startup.source_scan_pending,
                startup_auto_load_pending: state.ui.startup.auto_load_pending,
                pending_playback_start: state.audio.pending_playback_start.is_some(),
            },
            layout: FrameLayoutState {
                audio_output_sample_rate: state
                    .audio
                    .output_resolved
                    .as_ref()
                    .map(|output| output.sample_rate),
            },
            projection: FrameProjectionState {
                playback_visual_generation: state.waveform.current.playback_visual_generation(),
                play_selection_flash_active: state.waveform.current.play_selection_flash_active(),
                copy_flash_frames: state
                    .library
                    .folder_browser
                    .copy_flash_frames()
                    .max(state.waveform.current.copy_flash_frames()),
                selection_flash_frames: state.library.folder_browser.selection_flash_frames(),
                protected_source_error_flash_frames: state
                    .library
                    .folder_browser
                    .protected_source_error_flash_frames()
                    .max(state.waveform.current.protected_source_error_flash_frames()),
                keyboard_focus_alpha,
            },
        }
    }

    fn repaint_scope_since(self, previous: Self) -> RepaintScope {
        if self.structure != previous.structure {
            RepaintScope::Surface
        } else if self.layout != previous.layout {
            RepaintScope::Layout
        } else if self.projection != previous.projection || previous.requires_projection_refresh() {
            RepaintScope::Projection
        } else {
            RepaintScope::PaintOnly
        }
    }

    fn requires_projection_refresh(self) -> bool {
        self.projection.play_selection_flash_active
            || self.structure.folder_progress_active
            || self.structure.file_move_progress_active
            || self.structure.source_cache_progress_active
            || self.structure.sample_loading
            || self.structure.audio_opening
            || self.structure.startup_source_scan_pending
            || self.structure.startup_auto_load_pending
            || self.structure.pending_playback_start
    }
}

impl TransientFrameRevisionInputs {
    fn from_state(state: &NativeAppState) -> Self {
        Self {
            structure: TransientFrameStructureState {
                drag_hover_auto_expand_pending: state
                    .library
                    .folder_browser
                    .drag_hover_auto_expand_pending(),
                source_cache_progress_active: state
                    .waveform
                    .cache
                    .active_folder_warm_folder_id
                    .is_some(),
                sample_loading: state.active_sample_load_task().is_some(),
                audio_opening: state.background.audio_open.active().is_some(),
                audio_settings_error_active: state.audio.settings_error.is_some(),
                pending_playback_start: state.audio.pending_playback_start.is_some(),
            },
            layout: FrameLayoutState {
                audio_output_sample_rate: state
                    .audio
                    .output_resolved
                    .as_ref()
                    .map(|output| output.sample_rate),
            },
            projection: TransientFrameProjectionState {
                playback_visual_generation: state.waveform.current.playback_visual_generation(),
                play_selection_flash_active: state.waveform.current.play_selection_flash_active(),
                copy_flash_frames: state
                    .library
                    .folder_browser
                    .copy_flash_frames()
                    .max(state.waveform.current.copy_flash_frames()),
                selection_flash_frames: state.library.folder_browser.selection_flash_frames(),
                protected_source_error_flash_frames: state
                    .library
                    .folder_browser
                    .protected_source_error_flash_frames()
                    .max(state.waveform.current.protected_source_error_flash_frames()),
                keyboard_focus_alpha: state.library.folder_browser.keyboard_focus_alpha(),
                progress_tick_bits: state.background.progress_tick.to_bits(),
            },
        }
    }

    fn repaint_scope_since(self, previous: Self) -> RepaintScope {
        if self.structure != previous.structure {
            RepaintScope::Surface
        } else if self.layout != previous.layout {
            RepaintScope::Layout
        } else if self.projection != previous.projection {
            RepaintScope::Projection
        } else {
            RepaintScope::PaintOnly
        }
    }
}

fn frame_repaint_reason(scope: RepaintScope) -> &'static str {
    match scope {
        RepaintScope::Surface => "structure_revision",
        RepaintScope::Layout => "layout_revision",
        RepaintScope::Projection => "projection_revision",
        RepaintScope::PaintOnly => "paint_only",
    }
}
