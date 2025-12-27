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
const MIN_TRANSIENT_K_HIGH: f32 = 1.0;
const MAX_TRANSIENT_K_HIGH: f32 = 12.0;
const MIN_TRANSIENT_K_LOW: f32 = 0.5;
const MAX_TRANSIENT_K_LOW: f32 = 8.0;
const MIN_TRANSIENT_FLOOR_QUANTILE: f32 = 0.1;
const MAX_TRANSIENT_FLOOR_QUANTILE: f32 = 0.9;
const MIN_TRANSIENT_GAP_SECONDS: f32 = 0.02;
const MAX_TRANSIENT_GAP_SECONDS: f32 = 0.2;

pub(super) fn clamp_scroll_speed(speed: f32) -> f32 {
    speed.clamp(MIN_SCROLL_SPEED, MAX_SCROLL_SPEED)
}

pub(super) fn clamp_zoom_factor(factor: f32) -> f32 {
    factor.clamp(MIN_ZOOM_FACTOR, MAX_ZOOM_FACTOR)
}

pub(super) fn clamp_anti_clip_fade_ms(fade_ms: f32) -> f32 {
    fade_ms.clamp(MIN_ANTI_CLIP_FADE_MS, MAX_ANTI_CLIP_FADE_MS)
}

pub(super) fn clamp_transient_sensitivity(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

pub(super) fn clamp_transient_k_high(value: f32) -> f32 {
    value.clamp(MIN_TRANSIENT_K_HIGH, MAX_TRANSIENT_K_HIGH)
}

pub(super) fn clamp_transient_k_low(value: f32) -> f32 {
    value.clamp(MIN_TRANSIENT_K_LOW, MAX_TRANSIENT_K_LOW)
}

pub(super) fn clamp_transient_floor_quantile(value: f32) -> f32 {
    value.clamp(MIN_TRANSIENT_FLOOR_QUANTILE, MAX_TRANSIENT_FLOOR_QUANTILE)
}

pub(super) fn clamp_transient_min_gap_seconds(value: f32) -> f32 {
    value.clamp(MIN_TRANSIENT_GAP_SECONDS, MAX_TRANSIENT_GAP_SECONDS)
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

    /// Update and persist the BPM snap value when valid.
    pub fn set_bpm_value(&mut self, value: f32) {
        if !value.is_finite() || value <= 0.0 {
            return;
        }
        if (self.settings.controls.bpm_value - value).abs() < f32::EPSILON {
            return;
        }
        self.settings.controls.bpm_value = value;
        self.ui.waveform.bpm_value = Some(value);
        self.persist_controls();
    }

    /// Enable/disable transient snapping and persist the setting.
    pub fn set_transient_snap_enabled(&mut self, enabled: bool) {
        if self.settings.controls.transient_snap_enabled == enabled {
            return;
        }
        self.settings.controls.transient_snap_enabled = enabled;
        self.ui.waveform.transient_snap_enabled = enabled;
        self.persist_controls();
    }

    /// Enable/disable transient marker rendering and persist the setting.
    pub fn set_transient_markers_enabled(&mut self, enabled: bool) {
        if self.settings.controls.transient_markers_enabled == enabled {
            return;
        }
        self.settings.controls.transient_markers_enabled = enabled;
        self.ui.waveform.transient_markers_enabled = enabled;
        if !enabled {
            self.settings.controls.transient_snap_enabled = false;
            self.ui.waveform.transient_snap_enabled = false;
        }
        self.persist_controls();
    }

    /// Enable/disable realtime transient updates and persist the setting.
    pub fn set_transient_realtime_enabled(&mut self, enabled: bool) {
        if self.settings.controls.transient_realtime_enabled == enabled {
            return;
        }
        self.settings.controls.transient_realtime_enabled = enabled;
        self.ui.waveform.transient_realtime_enabled = enabled;
        if enabled
            && (self.ui.waveform.transient_sensitivity
                - self.ui.waveform.transient_sensitivity_draft)
                .abs()
                > f32::EPSILON
        {
            let value = self.ui.waveform.transient_sensitivity_draft;
            self.apply_transient_sensitivity(value);
            return;
        }
        self.persist_controls();
    }

    /// Update and persist the transient detection sensitivity, then recompute.
    pub fn apply_transient_sensitivity(&mut self, value: f32) {
        let clamped = clamp_transient_sensitivity(value);
        if (self.settings.controls.transient_sensitivity - clamped).abs() < f32::EPSILON {
            return;
        }
        self.settings.controls.transient_sensitivity = clamped;
        self.ui.waveform.transient_sensitivity = clamped;
        self.ui.waveform.transient_sensitivity_draft = clamped;
        self.refresh_waveform_transients();
        self.persist_controls();
    }

    /// Enable/disable custom transient tuning and refresh markers.
    pub fn set_transient_use_custom_tuning(&mut self, enabled: bool) {
        if self.settings.controls.transient_use_custom_tuning == enabled {
            return;
        }
        self.settings.controls.transient_use_custom_tuning = enabled;
        self.ui.waveform.transient_use_custom_tuning = enabled;
        self.refresh_waveform_transients();
        self.persist_controls();
    }

    /// Update and persist the transient high threshold multiplier.
    pub fn set_transient_k_high(&mut self, value: f32) {
        let clamped = clamp_transient_k_high(value);
        if (self.settings.controls.transient_k_high - clamped).abs() < f32::EPSILON {
            return;
        }
        self.settings.controls.transient_k_high = clamped;
        self.ui.waveform.transient_k_high = clamped;
        if self.settings.controls.transient_use_custom_tuning {
            self.refresh_waveform_transients();
        }
        self.persist_controls();
    }

    /// Update and persist the transient low threshold multiplier.
    pub fn set_transient_k_low(&mut self, value: f32) {
        let clamped = clamp_transient_k_low(value);
        if (self.settings.controls.transient_k_low - clamped).abs() < f32::EPSILON {
            return;
        }
        self.settings.controls.transient_k_low = clamped;
        self.ui.waveform.transient_k_low = clamped;
        if self.settings.controls.transient_use_custom_tuning {
            self.refresh_waveform_transients();
        }
        self.persist_controls();
    }

    /// Update and persist the transient floor quantile.
    pub fn set_transient_floor_quantile(&mut self, value: f32) {
        let clamped = clamp_transient_floor_quantile(value);
        if (self.settings.controls.transient_floor_quantile - clamped).abs() < f32::EPSILON {
            return;
        }
        self.settings.controls.transient_floor_quantile = clamped;
        self.ui.waveform.transient_floor_quantile = clamped;
        if self.settings.controls.transient_use_custom_tuning {
            self.refresh_waveform_transients();
        }
        self.persist_controls();
    }

    /// Update and persist the transient minimum gap in seconds.
    pub fn set_transient_min_gap_seconds(&mut self, value: f32) {
        let clamped = clamp_transient_min_gap_seconds(value);
        if (self.settings.controls.transient_min_gap_seconds - clamped).abs() < f32::EPSILON {
            return;
        }
        self.settings.controls.transient_min_gap_seconds = clamped;
        self.ui.waveform.transient_min_gap_seconds = clamped;
        if self.settings.controls.transient_use_custom_tuning {
            self.refresh_waveform_transients();
        }
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
