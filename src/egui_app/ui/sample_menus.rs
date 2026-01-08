use super::*;

use eframe::egui;

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
}
