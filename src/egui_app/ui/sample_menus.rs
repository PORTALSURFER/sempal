use super::helpers;
use super::*;

use eframe::egui;
use crate::egui_app::ui::style::StatusTone;

impl EguiApp {
    pub(super) fn sample_tag_menu<F>(
        &mut self,
        ui: &mut egui::Ui,
        close_menu: &mut bool,
        mut on_tag: F,
    ) where
        F: FnMut(&mut EguiApp, crate::sample_sources::Rating) -> bool,
    {
        use crate::sample_sources::Rating;
        ui.menu_button("Tag", |ui| {
            let mut tag_clicked = false;
            ui.horizontal(|ui| {
                if ui.button("Trash (-3)").clicked() { tag_clicked |= on_tag(self, Rating::new(-3)); }
                if ui.button("Trash (-2)").clicked() { tag_clicked |= on_tag(self, Rating::new(-2)); }
                if ui.button("Trash (-1)").clicked() { tag_clicked |= on_tag(self, Rating::new(-1)); }
            });
            ui.separator();
             if ui.button("Neutral (0)").clicked() { tag_clicked |= on_tag(self, Rating::NEUTRAL); }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Keep (+1)").clicked() { tag_clicked |= on_tag(self, Rating::new(1)); }
                if ui.button("Keep (+2)").clicked() { tag_clicked |= on_tag(self, Rating::new(2)); }
                if ui.button("Keep (+3)").clicked() { tag_clicked |= on_tag(self, Rating::new(3)); }
            });

            if tag_clicked {
                *close_menu = true;
                ui.close();
            }
        });
    }

    pub(super) fn sample_rename_controls<F>(
        &mut self,
        ui: &mut egui::Ui,
        rename_id: egui::Id,
        default_name: &str,
        mut on_rename: F,
    ) -> bool
    where
        F: FnMut(&mut EguiApp, &str) -> bool,
    {
        ui.label("Rename");
        let mut value = ui.ctx().data_mut(|data| {
            let value = data.get_temp::<String>(rename_id);
            let value = value.unwrap_or_else(|| default_name.to_string());
            data.insert_temp(rename_id, value.clone());
            value
        });
        let edit = ui.text_edit_singleline(&mut value);
        ui.ctx()
            .data_mut(|data| data.insert_temp(rename_id, value.clone()));
        let requested = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Apply rename").clicked() || requested) && on_rename(self, value.as_str()) {
            return true;
        }
        false
    }

    /// Render a BPM input row that applies the value when confirmed.
    pub(super) fn sample_bpm_controls<F>(
        &mut self,
        ui: &mut egui::Ui,
        bpm_id: egui::Id,
        default_bpm: Option<f32>,
        mut on_apply: F,
    ) -> bool
    where
        F: FnMut(&mut EguiApp, f32) -> bool,
    {
        let mut value = ui.ctx().data_mut(|data| {
            let value = data.get_temp::<String>(bpm_id);
            let value = value.unwrap_or_else(|| {
                default_bpm
                    .map(helpers::format_bpm_input)
                    .unwrap_or_default()
            });
            data.insert_temp(bpm_id, value.clone());
            value
        });
        let mut apply_requested = false;
        ui.horizontal(|ui| {
            ui.label("BPM");
            let edit = ui.add(
                egui::TextEdit::singleline(&mut value)
                    .desired_width(64.0)
                    .hint_text("120"),
            );
            apply_requested =
                edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Apply BPM").clicked() {
                apply_requested = true;
            }
        });
        ui.ctx()
            .data_mut(|data| data.insert_temp(bpm_id, value.clone()));
        if apply_requested {
            match helpers::parse_bpm_input(&value) {
                Some(bpm) => {
                    if on_apply(self, bpm) {
                        return true;
                    }
                }
                None => {
                    self.controller
                        .set_status("Enter a positive BPM value", StatusTone::Warning);
                }
            }
        }
        false
    }
}
