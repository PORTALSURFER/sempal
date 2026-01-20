use super::*;

const MIN_SCROLL_SPEED: f32 = 0.2;
const MAX_SCROLL_SPEED: f32 = 5.0;
const MIN_ZOOM_FACTOR: f32 = 0.5;
const MAX_ZOOM_FACTOR: f32 = 0.995;
const WHEEL_ZOOM_ANCHOR_FACTOR: f32 = 0.96;
const MIN_WHEEL_ZOOM_SPEED: f32 = 0.1;
const MAX_WHEEL_ZOOM_SPEED: f32 = 20.0;
const MIN_ANTI_CLIP_FADE_MS: f32 = 0.0;
const MAX_ANTI_CLIP_FADE_MS: f32 = 20.0;

pub(crate) fn clamp_scroll_speed(speed: f32) -> f32 {
    speed.clamp(MIN_SCROLL_SPEED, MAX_SCROLL_SPEED)
}

pub(crate) fn clamp_zoom_factor(factor: f32) -> f32 {
    factor.clamp(MIN_ZOOM_FACTOR, MAX_ZOOM_FACTOR)
}

pub(crate) fn clamp_anti_clip_fade_ms(fade_ms: f32) -> f32 {
    fade_ms.clamp(MIN_ANTI_CLIP_FADE_MS, MAX_ANTI_CLIP_FADE_MS)
}

fn clamp_wheel_zoom_speed(speed: f32) -> f32 {
    speed.clamp(MIN_WHEEL_ZOOM_SPEED, MAX_WHEEL_ZOOM_SPEED)
}

fn wheel_zoom_speed_to_factor(speed: f32) -> f32 {
    let speed = clamp_wheel_zoom_speed(speed);
    clamp_zoom_factor(WHEEL_ZOOM_ANCHOR_FACTOR.powf(speed))
}

fn wheel_zoom_factor_to_speed(factor: f32) -> f32 {
    let factor = clamp_zoom_factor(factor);
    clamp_wheel_zoom_speed(factor.ln() / WHEEL_ZOOM_ANCHOR_FACTOR.ln())
}

impl EguiController {
    /// Set and persist waveform scroll speed (clamped).
    pub fn set_waveform_scroll_speed(&mut self, speed: f32) {
        let clamped = clamp_scroll_speed(speed);
        if (self.settings.controls.waveform_scroll_speed - clamped).abs() < f32::EPSILON {
            return;
        }
        self.settings.controls.waveform_scroll_speed = clamped;
        self.ui.controls.waveform_scroll_speed = clamped;
        self.persist_controls();
    }

    /// Toggle and persist inverted waveform scroll direction.
    pub fn set_invert_waveform_scroll(&mut self, invert: bool) {
        if self.settings.controls.invert_waveform_scroll == invert {
            return;
        }
        self.settings.controls.invert_waveform_scroll = invert;
        self.ui.controls.invert_waveform_scroll = invert;
        self.persist_controls();
    }

    /// Set and persist wheel zoom factor (clamped).
    pub fn set_wheel_zoom_factor(&mut self, factor: f32) {
        let clamped = clamp_zoom_factor(factor);
        if (self.settings.controls.wheel_zoom_factor - clamped).abs() < f32::EPSILON {
            return;
        }
        self.settings.controls.wheel_zoom_factor = clamped;
        self.ui.controls.wheel_zoom_factor = clamped;
        self.persist_controls();
    }

    /// Return the derived wheel zoom speed from the stored factor.
    pub fn wheel_zoom_speed(&self) -> f32 {
        wheel_zoom_factor_to_speed(self.ui.controls.wheel_zoom_factor)
    }

    /// Set and persist wheel zoom speed (low = slower, high = faster).
    pub fn set_wheel_zoom_speed(&mut self, speed: f32) {
        self.set_wheel_zoom_factor(wheel_zoom_speed_to_factor(speed));
    }

    /// Set and persist keyboard zoom factor (clamped).
    pub fn set_keyboard_zoom_factor(&mut self, factor: f32) {
        let clamped = clamp_zoom_factor(factor);
        if (self.settings.controls.keyboard_zoom_factor - clamped).abs() < f32::EPSILON {
            return;
        }
        self.settings.controls.keyboard_zoom_factor = clamped;
        self.ui.controls.keyboard_zoom_factor = clamped;
        self.persist_controls();
    }

    /// Toggle and persist the anti-clip fade.
    pub fn set_anti_clip_fade_enabled(&mut self, enabled: bool) {
        if self.settings.controls.anti_clip_fade_enabled == enabled {
            return;
        }
        self.settings.controls.anti_clip_fade_enabled = enabled;
        self.ui.controls.anti_clip_fade_enabled = enabled;
        self.apply_anti_clip_fade_settings();
        self.persist_controls();
    }

    /// Set and persist the anti-clip fade duration in milliseconds.
    pub fn set_anti_clip_fade_ms(&mut self, fade_ms: f32) {
        let clamped = clamp_anti_clip_fade_ms(fade_ms);
        if (self.settings.controls.anti_clip_fade_ms - clamped).abs() < f32::EPSILON {
            return;
        }
        self.settings.controls.anti_clip_fade_ms = clamped;
        self.ui.controls.anti_clip_fade_ms = clamped;
        self.apply_anti_clip_fade_settings();
        self.persist_controls();
    }

    /// Toggle and persist destructive "yolo mode" (skip confirmation prompts).
    pub fn set_destructive_yolo_mode(&mut self, enabled: bool) {
        if self.settings.controls.destructive_yolo_mode == enabled {
            return;
        }
        self.settings.controls.destructive_yolo_mode = enabled;
        self.ui.controls.destructive_yolo_mode = enabled;
        self.persist_controls();
    }

    /// Toggle and persist input monitoring during recording.
    pub fn set_input_monitoring_enabled(&mut self, enabled: bool) {
        if self.settings.controls.input_monitoring_enabled == enabled {
            return;
        }
        self.settings.controls.input_monitoring_enabled = enabled;
        self.ui.controls.input_monitoring_enabled = enabled;
        if enabled && self.is_recording() {
            if let Some(recorder) = self.audio.recorder.take() {
                self.start_input_monitor(&recorder);
                self.audio.recorder = Some(recorder);
            }
        } else if !enabled {
            self.stop_input_monitor();
        }
        self.persist_controls();
    }

    fn apply_anti_clip_fade_settings(&mut self) {
        let fade_ms = self.settings.controls.anti_clip_fade_ms;
        let enabled = self.settings.controls.anti_clip_fade_enabled;
        if let Some(player) = self.audio.player.as_ref() {
            player.borrow_mut().set_anti_clip_settings(enabled, fade_ms);
        }
    }

    /// Set and persist the waveform channel view mode and refresh the waveform image.
    pub fn set_waveform_channel_view(&mut self, view: crate::waveform::WaveformChannelView) {
        if self.settings.controls.waveform_channel_view == view {
            return;
        }
        self.settings.controls.waveform_channel_view = view;
        self.ui.controls.waveform_channel_view = view;
        self.ui.waveform.channel_view = view;
        self.sample_view.waveform.render_meta = None;
        self.refresh_waveform_image();
        self.persist_controls();
    }

    /// Enable/disable BPM snapping and persist the setting.
    pub fn set_bpm_snap_enabled(&mut self, enabled: bool) {
        if self.settings.controls.bpm_snap_enabled == enabled {
            return;
        }
        self.settings.controls.bpm_snap_enabled = enabled;
        self.ui.waveform.bpm_snap_enabled = enabled;
        self.persist_controls();
    }

    /// Enable/disable BPM auto-override lock and persist the setting.
    pub fn set_bpm_lock_enabled(&mut self, enabled: bool) {
        if self.settings.controls.bpm_lock_enabled == enabled {
            return;
        }
        self.settings.controls.bpm_lock_enabled = enabled;
        self.ui.waveform.bpm_lock_enabled = enabled;
        self.persist_controls();
    }

    /// Enable/disable BPM stretch and persist the setting.
    pub fn set_bpm_stretch_enabled(&mut self, enabled: bool) {
        if self.settings.controls.bpm_stretch_enabled == enabled {
            return;
        }
        let was_playing = self.is_playing();
        let start_override = if was_playing {
            Some(self.ui.waveform.playhead.position)
        } else {
            None
        };
        let looped = self.ui.waveform.loop_enabled;
        self.settings.controls.bpm_stretch_enabled = enabled;
        self.ui.waveform.bpm_stretch_enabled = enabled;
        self.persist_controls();
        let (source, relative_path) = {
            let Some(loaded) = self.sample_view.wav.loaded_audio.as_ref() else {
                return;
            };
            (
                crate::sample_sources::SampleSource {
                    id: loaded.source_id.clone(),
                    root: loaded.root.clone(),
                },
                loaded.relative_path.clone(),
            )
        };
        self.reload_waveform_for_selection_if_active(&source, &relative_path);
        let loaded_matches = self
            .sample_view
            .wav
            .loaded_audio
            .as_ref()
            .is_some_and(|audio| {
                audio.source_id == source.id && audio.relative_path == relative_path
            });
        if was_playing && loaded_matches {
            if let Err(err) = self.play_audio(looped, start_override) {
                self.set_status(err, StatusTone::Error);
            }
        }
    }

    /// Enable/disable loop lock and persist the setting.
    pub fn set_loop_lock_enabled(&mut self, enabled: bool) {
        if self.settings.controls.loop_lock_enabled == enabled {
            return;
        }
        self.settings.controls.loop_lock_enabled = enabled;
        self.ui.waveform.loop_lock_enabled = enabled;
        self.persist_controls();
    }

    /// Update and persist the BPM snap value for waveform snapping and stretching.
    ///
    /// When stretch is enabled and a sample is loaded, the waveform reloads to
    /// apply the new tempo.
    pub fn set_bpm_value(&mut self, value: f32) {
        if !value.is_finite() || value <= 0.0 {
            return;
        }
        let settings_match = (self.settings.controls.bpm_value - value).abs() < f32::EPSILON;
        let ui_match = self
            .ui
            .waveform
            .bpm_value
            .is_some_and(|bpm| (bpm - value).abs() < f32::EPSILON);
        if settings_match && ui_match {
            return;
        }
        let was_playing = self.is_playing();
        let start_override = if was_playing {
            Some(self.ui.waveform.playhead.position)
        } else {
            None
        };
        let looped = self.ui.waveform.loop_enabled;
        self.settings.controls.bpm_value = value;
        self.ui.waveform.bpm_value = Some(value);
        self.persist_controls();
        if self.ui.waveform.bpm_stretch_enabled
            && !self.selection_state.range.is_dragging()
            && !self.selection_state.edit_range.is_dragging()
        {
            let (source, relative_path) = {
                let Some(loaded) = self.sample_view.wav.loaded_audio.as_ref() else {
                    return;
                };
                (
                    crate::sample_sources::SampleSource {
                        id: loaded.source_id.clone(),
                        root: loaded.root.clone(),
                    },
                    loaded.relative_path.clone(),
                )
            };
            self.reload_waveform_for_selection_if_active(&source, &relative_path);
            let loaded_matches = self
                .sample_view
                .wav
                .loaded_audio
                .as_ref()
                .is_some_and(|audio| {
                    audio.source_id == source.id && audio.relative_path == relative_path
                });
            if was_playing && loaded_matches {
                if let Err(err) = self.play_audio(looped, start_override) {
                    self.set_status(err, StatusTone::Error);
                }
            }
        }
    }

    /// Enable/disable transient snapping and persist the setting.
    pub fn set_transient_snap_enabled(&mut self, enabled: bool) {
        if self.settings.controls.transient_snap_enabled == enabled {
            self.ui.waveform.transient_snap_enabled =
                enabled && self.ui.waveform.transient_markers_enabled;
            return;
        }
        self.settings.controls.transient_snap_enabled = enabled;
        self.ui.waveform.transient_snap_enabled =
            enabled && self.ui.waveform.transient_markers_enabled;
        self.persist_controls();
    }

    /// Enable/disable transient marker rendering and persist the setting.
    pub fn set_transient_markers_enabled(&mut self, enabled: bool) {
        if self.settings.controls.transient_markers_enabled == enabled {
            return;
        }
        self.settings.controls.transient_markers_enabled = enabled;
        self.ui.waveform.transient_markers_enabled = enabled;
        self.ui.waveform.transient_snap_enabled = if enabled {
            self.settings.controls.transient_snap_enabled
        } else {
            false
        };
        self.persist_controls();
    }

    /// Enable/disable normalized audition playback and persist the setting.
    pub fn set_normalized_audition_enabled(&mut self, enabled: bool) {
        if self.settings.controls.normalized_audition_enabled == enabled {
            return;
        }
        self.settings.controls.normalized_audition_enabled = enabled;
        self.ui.waveform.normalized_audition_enabled = enabled;
        self.persist_controls();
    }

    /// Set and persist the tooltip detail level.
    pub fn set_tooltip_mode(&mut self, mode: crate::sample_sources::config::TooltipMode) {
        if self.settings.controls.tooltip_mode == mode {
            return;
        }
        self.settings.controls.tooltip_mode = mode;
        self.ui.controls.tooltip_mode = mode;
        self.persist_controls();
    }

    /// Toggle and persist auto-advance after rating/tagging.
    pub fn set_advance_after_rating(&mut self, enabled: bool) {
        if self.settings.controls.advance_after_rating == enabled {
            return;
        }
        self.settings.controls.advance_after_rating = enabled;
        self.ui.controls.advance_after_rating = enabled;
        self.persist_controls();
    }

    fn persist_controls(&mut self) {
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_zoom_speed_mapping_is_monotonic() {
        let slow = wheel_zoom_speed_to_factor(0.2);
        let medium = wheel_zoom_speed_to_factor(1.0);
        let fast = wheel_zoom_speed_to_factor(10.0);

        assert!(slow > medium, "expected slower speed to zoom less per step");
        assert!(medium > fast, "expected higher speed to zoom more per step");
    }

    #[test]
    fn wheel_zoom_speed_round_trips_with_factor() {
        let speeds = [0.2, 0.5, 1.0, 2.0, 8.0, 16.0];
        for speed in speeds {
            let factor = wheel_zoom_speed_to_factor(speed);
            let round_tripped = wheel_zoom_factor_to_speed(factor);
            assert!(
                (speed - round_tripped).abs() < 0.02,
                "speed {speed} round-tripped to {round_tripped} via factor {factor}"
            );
        }
    }
}
