use super::helpers::InlineTextEditAction;
use super::helpers::render_inline_text_edit;
use super::style;
use super::*;
use crate::egui_app::state::CollectionRowView;
use crate::egui_app::view_model;
use eframe::egui::{self, RichText};
use std::path::PathBuf;

impl EguiApp {
    pub(super) fn render_collection_rename_editor(
        &mut self,
        ui: &mut egui::Ui,
        row_response: &egui::Response,
    ) {
        let Some(prompt) = self.controller.ui.collections.pending_action.as_mut() else {
            return;
        };
        let name = match prompt {
            crate::egui_app::state::CollectionActionPrompt::Rename { name, .. } => name,
        };
        let padding = ui.spacing().button_padding.x;
        let mut edit_rect = row_response.rect;
        edit_rect.min.x += padding;
        edit_rect.max.x -= padding;
        edit_rect.min.y += 2.0;
        edit_rect.max.y -= 2.0;
        match render_inline_text_edit(
            ui,
            edit_rect,
            name,
            "Rename collection",
            &mut self.controller.ui.collections.rename_focus_requested,
        ) {
            InlineTextEditAction::Submit => self.controller.apply_pending_collection_rename(),
            InlineTextEditAction::Cancel => self.controller.cancel_collection_rename(),
            InlineTextEditAction::None => {}
        }
    }

    pub(super) fn collection_sample_menu(
        &mut self,
        response: &egui::Response,
        row: usize,
        sample: &crate::egui_app::state::CollectionSampleView,
    ) {
        response.context_menu(|ui| {
            let mut close_menu = false;
            ui.label(RichText::new(sample.label.clone()).color(style::palette().text_primary));
            self.sample_tag_menu(ui, &mut close_menu, |app, tag| {
                app.controller.tag_collection_sample(row, tag).is_ok()
            });
            if ui
                .button("Normalize (overwrite)")
                .on_hover_text("Scale to full range and overwrite the wav")
                .clicked()
                && self.controller.normalize_collection_sample(row).is_ok()
            {
                close_menu = true;
            }
            ui.separator();
            let default_name = view_model::sample_display_label(&sample.path);
            let rename_id = ui.make_persistent_id(format!(
                "rename:sample:{}:{}",
                sample.source_id,
                sample.path.display()
            ));
            if self.sample_rename_controls(ui, rename_id, default_name.as_str(), |app, value| {
                app.controller.rename_collection_sample(row, value).is_ok()
            }) {
                close_menu = true;
            }
            let delete_btn = egui::Button::new(
                RichText::new("Delete from collection").color(style::destructive_text()),
            );
            if ui.add(delete_btn).clicked() && self.controller.delete_collection_sample(row).is_ok()
            {
                close_menu = true;
            }
            if close_menu {
                ui.close();
            }
        });
    }

    pub(super) fn collection_row_menu(
        &mut self,
        response: &egui::Response,
        collection: &CollectionRowView,
    ) {
        response.context_menu(|ui| {
            if ui.button("Set export folder…").clicked() {
                self.controller.pick_collection_export_path(&collection.id);
                ui.close();
            }
            if ui.button("Clear export folder").clicked() {
                self.controller.clear_collection_export_path(&collection.id);
                ui.close();
            }
            let refresh_enabled = collection.export_path.is_some();
            if ui
                .add_enabled(refresh_enabled, egui::Button::new("Sync export"))
                .clicked()
            {
                self.controller.sync_collection_export(&collection.id);
                ui.close();
            }
            let export_dir = collection_export_dir(collection);
            if ui
                .add_enabled(
                    export_dir.is_some(),
                    egui::Button::new("Open export folder"),
                )
                .clicked()
            {
                self.controller
                    .open_collection_export_folder(&collection.id);
                ui.close();
            }
            if let Some(path) = export_dir {
                ui.small(format!("Current export: {}", path.display()));
            } else {
                ui.small("No export folder set");
            }
            ui.separator();
            ui.label("Rename collection");
            let rename_id = ui.make_persistent_id(format!("rename:{}", collection.id.as_str()));
            let mut rename_value = ui.ctx().data_mut(|data| {
                let value = data.get_temp_mut_or_default::<String>(rename_id);
                if value.is_empty() {
                    *value = collection.name.clone();
                }
                value.clone()
            });
            let edit = ui.text_edit_singleline(&mut rename_value);
            ui.ctx()
                .data_mut(|data| data.insert_temp(rename_id, rename_value.clone()));
            let rename_requested =
                edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Apply rename").clicked() || rename_requested {
                self.controller
                    .rename_collection(&collection.id, rename_value.clone());
                ui.ctx()
                    .data_mut(|data| data.insert_temp(rename_id, rename_value));
                ui.close();
            }
            ui.separator();
            if ui
                .button(RichText::new("Delete collection").color(style::destructive_text()))
                .clicked()
            {
                let _ = self.controller.delete_collection(&collection.id);
                ui.close();
            }
        });
    }
}

fn collection_export_dir(collection: &CollectionRowView) -> Option<PathBuf> {
    collection.export_path.clone()
}
