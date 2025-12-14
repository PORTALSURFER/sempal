use super::*;

const MIN_SCROLL_SPEED: f32 = 0.2;
const MAX_SCROLL_SPEED: f32 = 5.0;
const MIN_ZOOM_FACTOR: f32 = 0.5;
const MAX_ZOOM_FACTOR: f32 = 0.995;

pub(super) fn clamp_scroll_speed(speed: f32) -> f32 {
    speed.clamp(MIN_SCROLL_SPEED, MAX_SCROLL_SPEED)
}

pub(super) fn clamp_zoom_factor(factor: f32) -> f32 {
    factor.clamp(MIN_ZOOM_FACTOR, MAX_ZOOM_FACTOR)
}

impl EguiController {
    /// Set and persist waveform scroll speed (clamped).
    pub fn set_waveform_scroll_speed(&mut self, speed: f32) {
        let clamped = clamp_scroll_speed(speed);
        if (self.controls.waveform_scroll_speed - clamped).abs() < f32::EPSILON {
            return;
        }
        self.controls.waveform_scroll_speed = clamped;
        self.ui.controls.waveform_scroll_speed = clamped;
        self.persist_controls();
    }

    /// Toggle and persist inverted waveform scroll direction.
    pub fn set_invert_waveform_scroll(&mut self, invert: bool) {
        if self.controls.invert_waveform_scroll == invert {
            return;
        }
        self.controls.invert_waveform_scroll = invert;
        self.ui.controls.invert_waveform_scroll = invert;
        self.persist_controls();
    }

    /// Set and persist wheel zoom factor (clamped).
    pub fn set_wheel_zoom_factor(&mut self, factor: f32) {
        let clamped = clamp_zoom_factor(factor);
        if (self.controls.wheel_zoom_factor - clamped).abs() < f32::EPSILON {
            return;
        }
        self.controls.wheel_zoom_factor = clamped;
        self.ui.controls.wheel_zoom_factor = clamped;
        self.persist_controls();
    }

    /// Set and persist keyboard zoom factor (clamped).
    pub fn set_keyboard_zoom_factor(&mut self, factor: f32) {
        let clamped = clamp_zoom_factor(factor);
        if (self.controls.keyboard_zoom_factor - clamped).abs() < f32::EPSILON {
            return;
        }
        self.controls.keyboard_zoom_factor = clamped;
        self.ui.controls.keyboard_zoom_factor = clamped;
        self.persist_controls();
    }

    /// Toggle and persist destructive "yolo mode" (skip confirmation prompts).
    pub fn set_destructive_yolo_mode(&mut self, enabled: bool) {
        if self.controls.destructive_yolo_mode == enabled {
            return;
        }
        self.controls.destructive_yolo_mode = enabled;
        self.ui.controls.destructive_yolo_mode = enabled;
        self.persist_controls();
    }

    /// Set and persist the waveform channel view mode and refresh the waveform image.
    pub fn set_waveform_channel_view(&mut self, view: crate::waveform::WaveformChannelView) {
        if self.controls.waveform_channel_view == view {
            return;
        }
        self.controls.waveform_channel_view = view;
        self.ui.controls.waveform_channel_view = view;
        self.ui.waveform.channel_view = view;
        self.waveform.render_meta = None;
        self.refresh_waveform_image();
        self.persist_controls();
    }

    fn persist_controls(&mut self) {
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }
}
