use super::super::style;
use super::super::*;
use crate::egui_app::state::WaveformView;
use eframe::egui::{self, Ui};

pub(in super::super) fn render_waveform_scrollbar(
    app: &mut EguiApp,
    ui: &mut Ui,
    scroll_rect: egui::Rect,
    view: WaveformView,
    view_width: f64,
) {
    let palette = style::palette();
    let scroll_resp = ui.interact(
        scroll_rect,
        ui.id().with("waveform_scrollbar"),
        egui::Sense::click_and_drag(),
    );
    let scroll_bg = style::with_alpha(palette.bg_primary, 140);
    ui.painter().rect_filled(scroll_rect, 0.0, scroll_bg);
    let indicator_width = (scroll_rect.width() as f64 * view_width) as f32;
    let indicator_x = scroll_rect.left() + (scroll_rect.width() as f64 * view.start) as f32;
    let indicator_rect = egui::Rect::from_min_size(
        egui::pos2(indicator_x, scroll_rect.top()),
        egui::vec2(indicator_width.max(8.0), scroll_rect.height()),
    );
    let thumb_color = style::with_alpha(palette.accent_ice, 200);
    ui.painter().rect_filled(indicator_rect, 0.0, thumb_color);
    if (scroll_resp.dragged() || scroll_resp.clicked())
        && scroll_rect.width() > f32::EPSILON
        && let Some(pos) = scroll_resp.interact_pointer_pos()
    {
        let frac = ((pos.x - scroll_rect.left()) / scroll_rect.width()).clamp(0.0, 1.0);
        app.controller.scroll_waveform_view(frac as f64);
    }
}
