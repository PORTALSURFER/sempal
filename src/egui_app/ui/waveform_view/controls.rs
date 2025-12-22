use super::style;
use super::*;
use eframe::egui::{self, RichText, Ui};

pub(super) fn render_waveform_controls(app: &mut EguiApp, ui: &mut Ui, palette: &style::Palette) {
    let mut view_mode = app.controller.ui.waveform.channel_view;
    ui.horizontal(|ui| {
        let mono = ui.selectable_value(
            &mut view_mode,
            crate::waveform::WaveformChannelView::Mono,
            "Mono envelope",
        );
        mono.on_hover_text("Show peak envelope across all channels");
        let split = ui.selectable_value(
            &mut view_mode,
            crate::waveform::WaveformChannelView::SplitStereo,
            "Split L/R",
        );
        split.on_hover_text("Render the first two channels separately");
        ui.add_space(10.0);
        let loop_enabled = app.controller.ui.waveform.loop_enabled;
        let loop_label = if loop_enabled {
            RichText::new("Loop: On").color(palette.accent_mint)
        } else {
            RichText::new("Loop: Off").color(palette.text_muted)
        };
        if ui
            .add(egui::Button::new(loop_label))
            .on_hover_text("Toggle loop playback for the current selection (or whole sample)")
            .clicked()
        {
            app.controller.toggle_loop();
        }
    });
    if view_mode != app.controller.ui.waveform.channel_view {
        app.controller.set_waveform_channel_view(view_mode);
    }
}

fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |start: u8, end: u8| -> u8 {
        let start = start as f32;
        let end = end as f32;
        (start + (end - start) * t).round().clamp(0.0, 255.0) as u8
    };
    egui::Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
}
