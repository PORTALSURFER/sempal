use super::style;
use super::*;
use crate::egui_app::state::DestructiveSelectionEdit;
use eframe::egui::{self, RichText};


pub(super) fn render_selection_context_menu(app: &mut EguiApp, ui: &mut egui::Ui) {
    let palette = style::palette();
    let fade_ms = app.controller.ui.controls.anti_clip_fade_ms;
    let mut close_menu = false;
    let has_edit_selection = app.controller.ui.waveform.edit_selection.is_some();
    let has_selection = app.controller.ui.waveform.selection.is_some();
    let title = if has_edit_selection {
        "Edit selection actions"
    } else if has_selection {
        "Selection actions"
    } else {
        "Audio actions"
    };
    let tooltip_mode = app.controller.ui.controls.tooltip_mode;

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
    ui.label(RichText::new(title).color(palette.text_primary));
    if helpers::tooltip(
        ui.button("Crop to selection"),
        "Crop to selection",
        "Overwrite the source file on disk with only the currently selected audio region. This permanently discards the rest of the file.",
        tooltip_mode,
    ).clicked() {
        request_edit(DestructiveSelectionEdit::CropSelection);
    }
    if helpers::tooltip(
        ui.button("Trim selection out"),
        "Trim selection out",
        "Delete the selected region from the file and join the remaining parts. This will shorten the audio file on disk.",
        tooltip_mode,
    ).clicked() {
        request_edit(DestructiveSelectionEdit::TrimSelection);
    }
    if helpers::tooltip(
        ui.button("Reverse selection"),
        "Reverse selection",
        "Flip the selected audio backwards in time. This is written directly back to the source file.",
        tooltip_mode,
    ).clicked() {
        request_edit(DestructiveSelectionEdit::ReverseSelection);
    }
    ui.separator();
    ui.horizontal(|ui| {
        let fade_lr = helpers::tooltip(
            ui.add(egui::Button::new(RichText::new("\\ Fade to null").color(palette.text_primary))),
            "Fade to silence",
            "Apply a linear volume fade-out from start to finish across the selection.",
            tooltip_mode,
        );
        if fade_lr.clicked() {
            request_edit(DestructiveSelectionEdit::FadeLeftToRight);
        }
        let fade_rl = helpers::tooltip(
            ui.add(egui::Button::new(RichText::new("/ Fade to null").color(palette.text_primary))),
            "Fade from silence",
            "Apply a linear volume fade-in from start to finish across the selection.",
            tooltip_mode,
        );
        if fade_rl.clicked() {
            request_edit(DestructiveSelectionEdit::FadeRightToLeft);
        }
    });
    if helpers::tooltip(
        ui.button("Mute selection"),
        "Mute selection",
        "Immediately zero out the volume for this region without any crossfading.",
        tooltip_mode,
    ).clicked() {
        request_edit(DestructiveSelectionEdit::MuteSelection);
    }
    if helpers::tooltip(
        ui.button("Remove clicks"),
        "Remove clicks",
        "Intelligently interpolate the audio to remove sharp single-sample discontinuities (clicks) in the selection.",
        tooltip_mode,
    ).clicked() {
        request_edit(DestructiveSelectionEdit::ClickRemoval);
    }
    if helpers::tooltip(
        ui.button("Short edge fades"),
        "Short edge fades",
        &format!("Apply ~{fade_ms:.1}ms fade-in/out ramps at the very edges of the selection to prevent popping when the audio is cut or pasted."),
        tooltip_mode,
    ).clicked() {
        request_edit(DestructiveSelectionEdit::ShortEdgeFades);
    }
    let mut auto_edge_fades = app.controller.ui.controls.auto_edge_fades_on_selection_exports;
    let auto_edge_response = helpers::tooltip(
        ui.checkbox(&mut auto_edge_fades, "Auto short edge fades on new samples"),
        "Auto short edge fades on new samples",
        "When enabled, samples created from selections are saved with the same short edge fades used by the Short edge fades action.",
        tooltip_mode,
    );
    if auto_edge_response.changed() {
        app.controller
            .set_auto_edge_fades_on_selection_exports(auto_edge_fades);
    }
    if helpers::tooltip(
        ui.button("Normalize selection"),
        "Normalize selection",
        "Scale the audio volume so that the loudest peak hits 0dB, while applying 5ms safety fades at the boundaries.",
        tooltip_mode,
    ).clicked() {
        request_edit(DestructiveSelectionEdit::NormalizeSelection);
    }
    if close_menu {
        ui.close();
    }
}
