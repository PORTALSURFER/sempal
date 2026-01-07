use super::style;
use super::*;
use eframe::egui::{self, Color32, Stroke};

pub(super) fn render_markers(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    view: crate::egui_app::state::WaveformView,
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

    draw_transient_markers(app, ui, rect, view, to_screen_x);
}

fn draw_transient_markers(
    app: &EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    view: crate::egui_app::state::WaveformView,
    to_screen_x: &impl Fn(f32, egui::Rect) -> f32,
) {
    let transients = &app.controller.ui.waveform.transients;
    if !app.controller.ui.waveform.transient_markers_enabled || transients.is_empty() {
        return;
    }
    let palette = style::palette();
    let stroke = Stroke::new(1.0, style::with_alpha(palette.accent_mint, 150));
    let triangle_fill = style::with_alpha(palette.accent_mint, 200);
    let triangle_height = 6.0;
    let triangle_half = 4.0;
    let top = rect.top() + super::LOOP_BAR_HEIGHT;
    for &marker in transients {
        if marker < view.start || marker > view.end {
            continue;
        }
        let x = to_screen_x(marker, rect);
        ui.painter()
            .line_segment([egui::pos2(x, top), egui::pos2(x, rect.bottom())], stroke);
        let base_y = rect.top() + 1.0;
        let tip_y = base_y + triangle_height;
        let points = vec![
            egui::pos2(x - triangle_half, base_y),
            egui::pos2(x + triangle_half, base_y),
            egui::pos2(x, tip_y),
        ];
        ui.painter().add(egui::Shape::convex_polygon(
            points,
            triangle_fill,
            Stroke::NONE,
        ));
    }
}
