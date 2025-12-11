use super::*;
use crate::sample_sources::SampleTag;
use eframe::egui;

impl EguiApp {
    pub(super) fn sample_tag_menu<F>(
        &mut self,
        ui: &mut egui::Ui,
        close_menu: &mut bool,
        mut on_tag: F,
    ) where
        F: FnMut(&mut EguiApp, SampleTag) -> bool,
    {
        ui.menu_button("Tag", |ui| {
            let mut tag_clicked = false;
            tag_clicked |= ui.button("Trash").clicked() && on_tag(self, SampleTag::Trash);
            tag_clicked |= ui.button("Neutral").clicked() && on_tag(self, SampleTag::Neutral);
            tag_clicked |= ui.button("Keep").clicked() && on_tag(self, SampleTag::Keep);
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
            let value = data.get_temp_mut_or_default::<String>(rename_id);
            if value.is_empty() {
                *value = default_name.to_string();
            }
            value.clone()
        });
        let edit = ui.text_edit_singleline(&mut value);
        ui.ctx()
            .data_mut(|data| data.insert_temp(rename_id, value.clone()));
        let requested = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Apply rename").clicked() || requested)
            && on_rename(self, value.as_str())
        {
            return true;
        }
        false
    }
}
