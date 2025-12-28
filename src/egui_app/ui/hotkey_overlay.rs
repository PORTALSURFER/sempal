use super::style;
use crate::egui_app::{
    controller::hotkeys::{self, HotkeyAction, HotkeyGesture},
    state::FocusContext,
};
use eframe::egui::{self, Align, Align2, Color32, Id, Layout, RichText, Vec2};

pub(super) fn render_hotkey_overlay(
    ctx: &egui::Context,
    focus: FocusContext,
    focus_actions: &[HotkeyAction],
    global_actions: &[HotkeyAction],
    visible: &mut bool,
) {
    if !*visible {
        return;
    }
    draw_backdrop(ctx);
    let palette = style::palette();
    let title = RichText::new("Hotkeys")
        .strong()
        .color(palette.accent_copper);
    let focus_label = focus_header(focus);
    egui::Area::new(egui::Id::new("hotkey_overlay_panel"))
        .order(egui::Order::Tooltip)
        .constrain(true)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            let frame = egui::Frame::window(&ctx.style()).fill(style::compartment_fill());
            frame.show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.heading(title);
                    ui.add_space(8.0);
                    render_section(ui, focus_label, focus_actions, palette.accent_mint);
                    ui.add_space(6.0);
                    render_section(ui, "Global", global_actions, palette.accent_ice);
                    ui.add_space(10.0);
                    if ui.button("Close").clicked() {
                        *visible = false;
                    }
                });
            });
        });
}

fn render_section(ui: &mut egui::Ui, title: &str, actions: &[HotkeyAction], title_color: Color32) {
    ui.label(RichText::new(title).strong().color(title_color));
    if actions.is_empty() {
        ui.label("No actions available");
        return;
    }
    for action in actions {
        ui.horizontal(|ui| {
            ui.label(action.label);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(gesture_label(&action.gesture));
            });
        });
    }
}

fn focus_header(focus: FocusContext) -> &'static str {
    match focus {
        FocusContext::Waveform => "Waveform",
        FocusContext::SampleBrowser => "Focused sample (browser)",
        FocusContext::SourceFolders => "Source folders",
        FocusContext::CollectionSample => "Focused sample (collection)",
        FocusContext::SourcesList => "Sources list",
        FocusContext::CollectionsList => "Collections list",
        FocusContext::None => "Focused sample",
    }
}

fn draw_backdrop(ctx: &egui::Context) {
    let screen_rect = ctx.viewport_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        Id::new("hotkey_overlay_backdrop"),
    ));
    painter.rect_filled(
        screen_rect,
        0.0,
        Color32::from_rgba_premultiplied(0, 0, 0, 120),
    );
}

fn gesture_label(gesture: &HotkeyGesture) -> String {
    let mut parts = vec![hotkeys::format_keypress(&gesture.first)];
    if let Some(chord) = gesture.chord {
        parts.push(hotkeys::format_keypress(&chord));
    }
    parts.join(", ")
}
