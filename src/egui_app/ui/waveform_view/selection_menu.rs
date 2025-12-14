use super::style;
use super::*;
use crate::egui_app::state::DestructiveSelectionEdit;
use eframe::egui::{self, RichText};

pub(super) fn render_selection_context_menu(app: &mut EguiApp, ui: &mut egui::Ui) {
    let palette = style::palette();
    let mut close_menu = false;
    let mut request_edit = |edit: DestructiveSelectionEdit| match app
        .controller
        .request_destructive_selection_edit(edit)
    {
        Ok(_) => {
            close_menu = true;
            true
        }
        Err(_) => false,
    };
    ui.label(RichText::new("Selection actions").color(palette.text_primary));
    if ui
        .button("Crop to selection")
        .on_hover_text("Overwrite the file with just this region")
        .clicked()
    {
        request_edit(DestructiveSelectionEdit::CropSelection);
    }
    if ui
        .button("Trim selection out")
        .on_hover_text("Remove the selection and close the gap")
        .clicked()
    {
        request_edit(DestructiveSelectionEdit::TrimSelection);
    }
    ui.separator();
    ui.horizontal(|ui| {
        let fade_lr_button =
            egui::Button::new(RichText::new("\\ Fade to null").color(palette.text_primary));
        let fade_lr = ui
            .add(fade_lr_button)
            .on_hover_text("Fade left to right down to silence");
        if fade_lr.clicked() {
            request_edit(DestructiveSelectionEdit::FadeLeftToRight);
        }
        let fade_rl_button =
            egui::Button::new(RichText::new("/ Fade to null").color(palette.text_primary));
        let fade_rl = ui
            .add(fade_rl_button)
            .on_hover_text("Fade right to left down to silence");
        if fade_rl.clicked() {
            request_edit(DestructiveSelectionEdit::FadeRightToLeft);
        }
    });
    if ui
        .button("Mute selection")
        .on_hover_text("Silence the selection without fades")
        .clicked()
    {
        request_edit(DestructiveSelectionEdit::MuteSelection);
    }
    if ui
        .button("Smooth edges")
        .on_hover_text("Apply short raised-cosine crossfades at selection edges")
        .clicked()
    {
        request_edit(DestructiveSelectionEdit::SmoothSelection);
    }
    if ui
        .button("Normalize selection")
        .on_hover_text("Scale selection to full range with 5ms edge fades")
        .clicked()
    {
        request_edit(DestructiveSelectionEdit::NormalizeSelection);
    }
    if close_menu {
        ui.close();
    }
}

