use super::selection_drag;
use super::selection_geometry::loop_bar_rect;
use super::style;
use super::*;
use eframe::egui::{self, Color32};

pub(super) fn render_loop_bar(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    view: crate::egui_app::state::WaveformView,
    view_width: f32,
    highlight: Color32,
) {
    let loop_bar_alpha = if app.controller.ui.waveform.loop_enabled {
        180
    } else {
        25
    };
    if loop_bar_alpha == 0 {
        return;
    }

    let selection = app.controller.ui.waveform.selection;
    let bar_rect = loop_bar_rect(rect, view, view_width as f64, selection, super::LOOP_BAR_HEIGHT);
    if let Some(selection) = selection {
        let response = ui.interact(
            bar_rect,
            ui.id().with("loop_bar_drag"),
            egui::Sense::click_and_drag(),
        );
        let hover_alpha = if response.hovered() || response.dragged() {
            (loop_bar_alpha + 50).min(255)
        } else {
            loop_bar_alpha
        };
        ui.painter()
            .rect_filled(bar_rect, 0.0, style::with_alpha(highlight, hover_alpha));
        if selection_is_power_of_two_beats(app, selection) {
            let radius = 2.0;
            let x = (bar_rect.right() - 4.0).max(bar_rect.left() + radius + 0.5);
            let y = bar_rect.center().y;
            ui.painter().circle_filled(
                egui::pos2(x, y),
                radius,
                style::with_alpha(egui::Color32::BLACK, 220),
            );
        }
        selection_drag::handle_selection_slide_drag(
            app,
            ui,
            rect,
            view,
            view_width as f64,
            selection,
            &response,
        );
    } else {
        ui.painter()
            .rect_filled(bar_rect, 0.0, style::with_alpha(highlight, loop_bar_alpha));
    }
}

fn selection_is_power_of_two_beats(
    app: &EguiApp,
    selection: crate::selection::SelectionRange,
) -> bool {
    if !app.controller.ui.waveform.bpm_snap_enabled {
        return false;
    }
    let bpm = app.controller.ui.waveform.bpm_value.unwrap_or(0.0);
    if !bpm.is_finite() || bpm <= 0.0 {
        return false;
    }
    let duration = app
        .controller
        .loaded_audio_duration_seconds()
        .unwrap_or(0.0);
    if !duration.is_finite() || duration <= 0.0 {
        return false;
    }
    let seconds = selection.width() * duration;
    if !seconds.is_finite() || seconds <= 0.0 {
        return false;
    }
    let beats = seconds * bpm / 60.0;
    if !beats.is_finite() || beats < 2.0 {
        return false;
    }
    let rounded = beats.round();
    if (beats - rounded).abs() > 1.0e-3 {
        return false;
    }
    let count = rounded as u32;
    count.is_power_of_two()
}
