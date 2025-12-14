use super::style;
use super::*;
use eframe::egui::{self, Color32, Stroke, StrokeKind, TextStyle, text::LayoutJob};

pub(super) fn render_hover_overlay(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    pointer_pos: Option<egui::Pos2>,
    view: crate::egui_app::state::WaveformView,
    view_width: f32,
    cursor_color: Color32,
    to_screen_x: &impl Fn(f32, egui::Rect) -> f32,
) {
    app.controller.update_waveform_hover_time(None);
    let mut hover_x = None;
    let mut hovering = false;
    if let Some(pos) = pointer_pos.filter(|p| rect.contains(*p)) {
        let now = std::time::Instant::now();
        let moved = app
            .controller
            .ui
            .waveform
            .hover_pointer_pos
            .map_or(true, |prev| prev.distance(pos) > 0.5);
        if moved {
            app.controller.ui.waveform.hover_pointer_pos = Some(pos);
            app.controller.ui.waveform.hover_pointer_last_moved_at = Some(now);
        }

        let normalized = ((pos.x - rect.left()) / rect.width())
            .mul_add(view_width, view.start)
            .clamp(0.0, 1.0);
        hovering = true;
        let allow_hover_override = moved
            || app
                .controller
                .ui
                .waveform
                .cursor_last_navigation_at
                .is_none_or(|nav| {
                    app.controller
                        .ui
                        .waveform
                        .hover_pointer_last_moved_at
                        .is_none_or(|moved_at| nav <= moved_at)
                });

        if allow_hover_override {
            hover_x = Some(pos.x);
            app.controller.set_waveform_cursor_from_hover(normalized);
            app.controller.update_waveform_hover_time(Some(normalized));
        } else if let Some(cursor) = app.controller.ui.waveform.cursor {
            hover_x = Some(to_screen_x(cursor, rect));
            app.controller.update_waveform_hover_time(Some(cursor));
        } else {
            hover_x = Some(pos.x);
            app.controller.set_waveform_cursor_from_hover(normalized);
            app.controller.update_waveform_hover_time(Some(normalized));
        }
    }
    let cursor_alpha = app.controller.waveform_cursor_alpha(hovering);
    if let Some(cursor) = app.controller.ui.waveform.cursor {
        let x = to_screen_x(cursor, rect);
        let stroke_alpha = (220.0 * cursor_alpha).round().clamp(0.0, 255.0) as u8;
        if stroke_alpha > 0 {
            ui.painter().line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                Stroke::new(1.0, style::with_alpha(cursor_color, stroke_alpha)),
            );
        }
    }
    if let Some(label) = app.controller.ui.waveform.hover_time_label.as_deref()
        && let Some(pointer_x) = hover_x
    {
        let palette = style::palette();
        let text_color = style::with_alpha(palette.text_primary, 240);
        let galley = ui.ctx().fonts_mut(|f| {
            f.layout_job(LayoutJob::simple_singleline(
                label.to_string(),
                TextStyle::Monospace.resolve(ui.style()),
                text_color,
            ))
        });
        let padding = egui::vec2(6.0, 4.0);
        let size = galley.size() + padding * 2.0;
        let min_x = rect.left() + 4.0;
        let max_x = rect.right() - size.x - 4.0;
        let desired_x = pointer_x + 8.0;
        let label_x = desired_x.clamp(min_x, max_x);
        let label_y = rect.top() + 8.0;
        let label_rect = egui::Rect::from_min_size(egui::pos2(label_x, label_y), size);
        let bg = style::with_alpha(palette.bg_primary, 235);
        let border = Stroke::new(1.0, style::with_alpha(palette.panel_outline, 220));
        ui.painter().rect_filled(label_rect, 4.0, bg);
        ui.painter()
            .rect_stroke(label_rect, 4.0, border, StrokeKind::Inside);
        ui.painter()
            .galley(label_rect.min + padding, galley, text_color);
    }
}
