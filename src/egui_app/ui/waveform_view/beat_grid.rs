use super::style;
use super::*;
use crate::egui_app::state::WaveformView;
use eframe::egui;

const MIN_BEAT_SPACING_PX: f32 = 4.0;
const MIN_QUARTER_SPACING_PX: f32 = 8.0;
const MAX_GRID_LINES: usize = 2400;

/// Draw subtle beat and quarter-beat guides behind the waveform when BPM snapping is active.
pub(super) fn render_waveform_beat_grid(
    app: &EguiApp,
    ui: &egui::Ui,
    rect: egui::Rect,
    _palette: &style::Palette,
    view: WaveformView,
    view_width: f32,
) {
    if !app.controller.ui.waveform.bpm_snap_enabled
        && !app.controller.ui.waveform.bpm_stretch_enabled
    {
        return;
    }
    let bpm = app.controller.ui.waveform.bpm_value.unwrap_or(0.0);
    if !bpm.is_finite() || bpm <= 0.0 {
        return;
    }
    let duration = match app.controller.loaded_audio_duration_seconds() {
        Some(duration) => duration,
        None => return,
    };
    if !duration.is_finite() || duration <= 0.0 {
        return;
    }
    if !view_width.is_finite() || view_width <= 0.0 {
        return;
    }
    let beat_step = 60.0 / bpm / duration;
    if !beat_step.is_finite() || beat_step <= 0.0 {
        return;
    }
    let beat_spacing_px = rect.width() * (beat_step / view_width);
    if !beat_spacing_px.is_finite() || beat_spacing_px < MIN_BEAT_SPACING_PX {
        return;
    }
    let quarter_step = beat_step * 0.25;
    let quarter_spacing_px = beat_spacing_px * 0.25;

    let grid_base = egui::Color32::from_rgb(200, 200, 200);
    let beat_stroke = egui::Stroke::new(1.0, style::with_alpha(grid_base, 90));
    let quarter_stroke = egui::Stroke::new(1.0, style::with_alpha(grid_base, 55));
    let mut draw_quarters = quarter_spacing_px >= MIN_QUARTER_SPACING_PX;

    let visible_start = view.start.max(0.0);
    let visible_end = view.end.min(1.0);
    if visible_end <= visible_start {
        return;
    }

    let painter = ui.painter();
    let draw_line = |position: f32, stroke: egui::Stroke| {
        let normalized = ((position - view.start) / view_width).clamp(0.0, 1.0);
        let x = rect.left() + rect.width() * normalized;
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            stroke,
        );
    };

    if draw_quarters {
        let start_index = (visible_start / quarter_step).floor() as i64;
        let end_index = (visible_end / quarter_step).ceil() as i64;
        let line_count = (end_index - start_index + 1).max(0) as usize;
        if line_count > MAX_GRID_LINES {
            draw_quarters = false;
        } else {
            for index in start_index..=end_index {
                let position = (index as f32) * quarter_step;
                let stroke = if index % 4 == 0 {
                    beat_stroke
                } else {
                    quarter_stroke
                };
                draw_line(position, stroke);
            }
        }
    }

    if !draw_quarters {
        let start_index = (visible_start / beat_step).floor() as i64;
        let end_index = (visible_end / beat_step).ceil() as i64;
        let line_count = (end_index - start_index + 1).max(0) as usize;
        if line_count > MAX_GRID_LINES {
            return;
        }
        for index in start_index..=end_index {
            let position = (index as f32) * beat_step;
            draw_line(position, beat_stroke);
        }
    }
}
