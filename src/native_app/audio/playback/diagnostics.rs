#[cfg(not(test))]
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use radiant::runtime as runtime_ui;

#[cfg(not(test))]
const PLAYHEAD_FRAME_DIAGNOSTICS_ENV: &str = "WAVECRATE_PLAYHEAD_FRAME_DIAGNOSTICS";

#[cfg(not(test))]
static PLAYHEAD_FRAME_DIAGNOSTICS_ENABLED: OnceLock<bool> = OnceLock::new();

#[derive(Default)]
pub(in crate::native_app) struct PlayheadFrameDiagnosticsState {
    latest_overlay: Option<PlayheadOverlayFrameDiagnostics>,
    latest_frame_message: Option<PlayheadFrameMessageDiagnostics>,
    #[cfg(test)]
    enabled_override: Option<bool>,
    #[cfg(test)]
    last_logged_frame_message: Option<Option<PlayheadFrameMessageDiagnostics>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::native_app) struct PlayheadOverlayFrameDiagnostics {
    pub(in crate::native_app) animation_time: Duration,
    pub(in crate::native_app) progress_ratio: f32,
    pub(in crate::native_app) visible_ratio: f32,
    pub(in crate::native_app) cursor_x: f32,
    pub(in crate::native_app) progress_source: PlayheadProgressSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct PlayheadFrameMessageDiagnostics {
    pub(in crate::native_app) paint_only: bool,
    pub(in crate::native_app) reason: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum PlayheadProgressSource {
    InterpolatedVisualProgress,
    PreviewAuditionProgress,
    WaveformPlayheadFallback,
}

impl PlayheadProgressSource {
    pub(in crate::native_app) fn label(self) -> &'static str {
        match self {
            Self::InterpolatedVisualProgress => "interpolated_visual_progress",
            Self::PreviewAuditionProgress => "preview_audition_progress",
            Self::WaveformPlayheadFallback => "waveform_playhead_fallback",
        }
    }
}

impl PlayheadFrameDiagnosticsState {
    fn enabled(&self) -> bool {
        #[cfg(test)]
        if let Some(enabled) = self.enabled_override {
            return enabled;
        }

        playhead_frame_diagnostics_enabled()
    }

    #[cfg(test)]
    fn enabled_for_test() -> Self {
        Self {
            enabled_override: Some(true),
            ..Self::default()
        }
    }

    pub(in crate::native_app) fn record_overlay_frame(
        &mut self,
        sample: PlayheadOverlayFrameDiagnostics,
    ) {
        if !self.enabled() {
            return;
        }
        self.latest_overlay = Some(sample);
    }

    pub(in crate::native_app) fn record_frame_message(
        &mut self,
        sample: PlayheadFrameMessageDiagnostics,
    ) {
        if !self.enabled() {
            return;
        }
        self.latest_frame_message = Some(sample);
    }

    #[cfg(test)]
    pub(in crate::native_app) fn latest_overlay_frame(
        &self,
    ) -> Option<PlayheadOverlayFrameDiagnostics> {
        self.latest_overlay
    }

    pub(in crate::native_app) fn observe_native_frame(
        &mut self,
        diagnostics: runtime_ui::NativeFrameDiagnostics,
    ) {
        if !self.enabled() {
            return;
        }
        let frame_message = self.latest_frame_message.take();
        let Some(overlay) = self.latest_overlay.take() else {
            return;
        };
        #[cfg(test)]
        {
            self.last_logged_frame_message = Some(frame_message);
        }
        tracing::info!(
            target: "wavecrate::debug::ui_frame",
            event = "waveform.playhead.frame",
            since_last_present_ms = duration_ms(diagnostics.timings.since_last_present),
            animation_time_ms = duration_ms(overlay.animation_time),
            progress_ratio = overlay.progress_ratio,
            visible_ratio = overlay.visible_ratio,
            cursor_x = overlay.cursor_x,
            progress_source = overlay.progress_source.label(),
            transient_overlay_paint_ms =
                duration_ms(diagnostics.timings.transient_overlay.paint),
            transient_overlay_primitives = diagnostics.timings.transient_overlay.primitives,
            frame_work = diagnostics.presentation.frame_work_kind,
            frame_work_reason = diagnostics.presentation.frame_work_reason,
            native_paint_only = diagnostics.presentation.paint_only,
            native_scene_rebuild = diagnostics.presentation.scene_rebuild,
            app_frame_message = frame_message.is_some(),
            app_frame_message_paint_only = frame_message.is_some_and(|sample| sample.paint_only),
            app_frame_message_reason = frame_message.map(|sample| sample.reason).unwrap_or("none"),
            "Waveform playhead frame diagnostics"
        );
    }
}

pub(in crate::native_app) fn playhead_frame_diagnostics_observer_enabled() -> bool {
    playhead_frame_diagnostics_enabled()
        && tracing::enabled!(target: "wavecrate::debug::ui_frame", tracing::Level::INFO)
}

pub(super) fn log_slow_playback_phase(
    event: &'static str,
    file_name: &str,
    source_kind: &'static str,
    started_at: Instant,
) {
    let elapsed = started_at.elapsed();
    if elapsed < Duration::from_millis(4) {
        return;
    }
    tracing::warn!(
        target: "wavecrate::debug::playback",
        event,
        elapsed_ms = elapsed.as_secs_f64() * 1000.0,
        file_name,
        source_kind,
        "Slow playback UI phase"
    );
}

fn playhead_frame_diagnostics_enabled() -> bool {
    #[cfg(test)]
    {
        false
    }
    #[cfg(not(test))]
    *PLAYHEAD_FRAME_DIAGNOSTICS_ENABLED
        .get_or_init(|| env_var_truthy(PLAYHEAD_FRAME_DIAGNOSTICS_ENV))
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

#[cfg(not(test))]
fn env_var_truthy(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|value| is_truthy(&value))
}

#[cfg(not(test))]
fn is_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playhead_frame_diagnostics_are_disabled_by_default() {
        assert!(!playhead_frame_diagnostics_observer_enabled());
    }

    #[test]
    fn disabled_playhead_frame_diagnostics_do_not_store_overlay_samples() {
        let mut state = PlayheadFrameDiagnosticsState::default();
        state.record_overlay_frame(PlayheadOverlayFrameDiagnostics {
            animation_time: Duration::from_millis(16),
            progress_ratio: 0.25,
            visible_ratio: 0.25,
            cursor_x: 42.0,
            progress_source: PlayheadProgressSource::InterpolatedVisualProgress,
        });

        assert_eq!(state.latest_overlay_frame(), None);
    }

    #[test]
    fn playhead_frame_diagnostics_retire_frame_message_on_no_overlay_presentation() {
        let mut state = PlayheadFrameDiagnosticsState::enabled_for_test();
        state.record_frame_message(PlayheadFrameMessageDiagnostics {
            paint_only: true,
            reason: "stale-frame-message",
        });

        state.observe_native_frame(runtime_ui::NativeFrameDiagnostics::default());

        assert_eq!(state.latest_frame_message, None);
        assert_eq!(state.last_logged_frame_message, None);

        state.record_overlay_frame(PlayheadOverlayFrameDiagnostics {
            animation_time: Duration::from_millis(16),
            progress_ratio: 0.25,
            visible_ratio: 0.25,
            cursor_x: 42.0,
            progress_source: PlayheadProgressSource::InterpolatedVisualProgress,
        });
        state.observe_native_frame(runtime_ui::NativeFrameDiagnostics::default());

        assert_eq!(state.last_logged_frame_message, Some(None));
    }
}
