use super::style;
use super::*;
use eframe::egui::{self, Color32, Stroke};

pub(super) fn render_overlays(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    view: crate::egui_app::state::WaveformView,
    view_width: f32,
    highlight: Color32,
    start_marker_color: Color32,
    to_screen_x: &impl Fn(f32, egui::Rect) -> f32,
) {
    if let Some(marker_pos) = app.controller.ui.waveform.last_start_marker
        && marker_pos >= view.start
        && marker_pos <= view.end
    {
        let x = to_screen_x(marker_pos, rect);
        let stroke = Stroke::new(1.5, style::with_alpha(start_marker_color, 230));
        let mut y = rect.top();
        let bottom = rect.bottom();
        let dash = 6.0;
        let gap = 4.0;
        while y < bottom {
            let end = (y + dash).min(bottom);
            ui.painter()
                .line_segment([egui::pos2(x, y), egui::pos2(x, end)], stroke);
            y += dash + gap;
        }
    }

    let loop_bar_alpha = if app.controller.ui.waveform.loop_enabled {
        180
    } else {
        25
    };
    if loop_bar_alpha > 0 {
        let (loop_start, loop_end) = app
            .controller
            .ui
            .waveform
            .selection
            .map(|range| (range.start(), range.end()))
            .unwrap_or((0.0, 1.0));
        let clamped_start = loop_start.clamp(0.0, 1.0);
        let clamped_end = loop_end.clamp(clamped_start, 1.0);
        let start_norm = ((clamped_start - view.start) / view_width).clamp(0.0, 1.0);
        let end_norm = ((clamped_end - view.start) / view_width).clamp(0.0, 1.0);
        let width = (end_norm - start_norm).max(0.0) * rect.width();
        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + rect.width() * start_norm, rect.top()),
            egui::vec2(width.max(2.0), 6.0),
        );
        ui.painter()
            .rect_filled(bar_rect, 0.0, style::with_alpha(highlight, loop_bar_alpha));
    }

    if app.controller.ui.waveform.playhead.visible {
        let playhead = &mut app.controller.ui.waveform.playhead;
        let now = ui.input(|i| i.time);
        let position = playhead.position.clamp(0.0, 1.0);
        const TRAIL_DURATION_SECS: f64 = 1.25;
        const MAX_TRAIL_SAMPLES: usize = 256;
        const POSITION_EPS: f32 = 0.0005;

        if let Some(last) = playhead.trail.back()
            && position + POSITION_EPS < last.position
        {
            playhead.trail.clear();
        }
        let should_push = match playhead.trail.back() {
            Some(last) => (position - last.position).abs() > POSITION_EPS,
            None => true,
        };
        if should_push {
            playhead
                .trail
                .push_back(crate::egui_app::state::PlayheadTrailSample {
                    position,
                    time: now,
                });
        }
        while let Some(front) = playhead.trail.front()
            && now - front.time > TRAIL_DURATION_SECS
        {
            playhead.trail.pop_front();
        }
        while playhead.trail.len() > MAX_TRAIL_SAMPLES {
            playhead.trail.pop_front();
        }

        let max_alpha: u8 = 160;
        for sample in playhead.trail.iter() {
            if sample.position < view.start || sample.position > view.end {
                continue;
            }
            let age = (now - sample.time).max(0.0);
            let t = (1.0 - (age / TRAIL_DURATION_SECS)).clamp(0.0, 1.0) as f32;
            let alpha = ((t * t) * max_alpha as f32).round() as u8;
            if alpha < 4 {
                continue;
            }
            let x = to_screen_x(sample.position, rect);
            ui.painter().line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                Stroke::new(2.0, style::with_alpha(highlight, alpha)),
            );
        }

        let x = to_screen_x(position, rect);
        ui.painter().line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            Stroke::new(2.0, highlight),
        );
    } else {
        app.controller.ui.waveform.playhead.trail.clear();
    }
}
