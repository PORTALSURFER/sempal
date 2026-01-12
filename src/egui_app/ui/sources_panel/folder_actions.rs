use super::EguiApp;
use super::helpers::{
    InlineTextEditAction, RowBackground, render_inline_text_edit, render_list_row,
};
use super::style;
use eframe::egui::{self, RichText, Ui};
use std::path::Path;

impl EguiApp {
    pub(super) fn render_folder_rename_editor(
        &mut self,
        ui: &mut Ui,
        row_response: &egui::Response,
        row: &crate::egui_app::state::FolderRowView,
    ) {
        let Some(prompt) = self.controller.ui.sources.folders.pending_action.as_mut() else {
            return;
        };
        let name = match prompt {
            crate::egui_app::state::FolderActionPrompt::Rename { name, .. } => name,
        };
        let padding = ui.spacing().button_padding.x;
        let indent = row.depth as f32 * 12.0;
        let mut edit_rect = row_response.rect;
        edit_rect.min.x += padding + indent + 14.0;
        edit_rect.max.x -= padding;
        edit_rect.min.y += 2.0;
        edit_rect.max.y -= 2.0;
        match render_inline_text_edit(
            ui,
            edit_rect,
            name,
            "Rename folder",
            &mut self.controller.ui.sources.folders.rename_focus_requested,
        ) {
            InlineTextEditAction::Submit => self.apply_pending_folder_rename(),
            InlineTextEditAction::Cancel => self.controller.cancel_folder_rename(),
            InlineTextEditAction::None => {}
        }
    }

    pub(super) fn render_inline_new_folder_row(
        &mut self,
        ui: &mut Ui,
        depth: usize,
        row_width: f32,
        row_height: f32,
    ) {
        let Some(inline) = self.controller.ui.sources.folders.new_folder.as_mut() else {
            return;
        };
        let response = render_list_row(
            ui,
            super::helpers::ListRow {
                label: "",
                row_width,
                row_height,
                background: RowBackground::Solid(style::row_selected_fill()),
                skip_hover: false,
                text_color: style::high_contrast_text(),
                sense: egui::Sense::hover(),
                number: None,
                marker: None,
                rating: None,
                looped: false,
                bpm_label: None,
            },
        );
        let padding = ui.spacing().button_padding.x;
        let indent = depth as f32 * 12.0;
        let mut edit_rect = response.rect;
        edit_rect.min.x += padding + indent + 14.0;
        edit_rect.max.x -= padding;
        edit_rect.min.y += 2.0;
        edit_rect.max.y -= 2.0;
        match render_inline_text_edit(
            ui,
            edit_rect,
            &mut inline.name,
            "New folder name",
            &mut inline.focus_requested,
        ) {
            InlineTextEditAction::Submit => self.apply_pending_folder_creation(),
            InlineTextEditAction::Cancel => self.controller.cancel_new_folder_creation(),
            InlineTextEditAction::None => {}
        }
    }

    pub(super) fn apply_pending_folder_rename(&mut self) {
        let action = self.controller.ui.sources.folders.pending_action.clone();
        if let Some(crate::egui_app::state::FolderActionPrompt::Rename { target, name }) = action {
            match self.controller.rename_folder(&target, &name) {
                Ok(()) => {
                    self.controller.cancel_folder_rename();
                }
                Err(err) => {
                    self.controller.cancel_folder_rename();
                    self.controller.set_status(err, style::StatusTone::Error);
                }
            }
        }
    }

    pub(super) fn apply_pending_folder_creation(&mut self) {
        let inline = self.controller.ui.sources.folders.new_folder.clone();
        if let Some(state) = inline {
            match self.controller.create_folder(&state.parent, &state.name) {
                Ok(()) => self.controller.ui.sources.folders.new_folder = None,
                Err(err) => self.controller.set_status(err, style::StatusTone::Error),
            }
        }
    }

    pub(super) fn folder_row_menu(
        &mut self,
        response: &egui::Response,
        index: usize,
        row: &crate::egui_app::state::FolderRowView,
    ) {
        response.context_menu(|ui| {
            let palette = style::palette();
            ui.label(RichText::new(row.name.clone()).color(palette.text_primary));
            ui.separator();
            let mut close_menu = false;
            self.folder_hotkey_menu(ui, &row.path, row.hotkey);
            if ui.button("Open in Explorer").clicked() {
                self.controller.open_folder_in_file_explorer(&row.path);
                close_menu = true;
            }
            if ui.button("New subfolder").clicked() {
                self.controller.focus_folder_row(index);
                self.controller.start_new_folder();
                close_menu = true;
            }
            if ui.button("Rename").clicked() {
                self.controller.focus_folder_row(index);
                self.controller.start_folder_rename();
                close_menu = true;
            }
            let delete_button = egui::Button::new(
                RichText::new("Delete")
                    .color(style::destructive_text())
                    .strong(),
            );
            if ui.add(delete_button).clicked() {
                self.controller.focus_folder_row(index);
                self.controller.delete_focused_folder();
                close_menu = true;
            }
            if close_menu {
                ui.close();
            }
        });
    }

    pub(super) fn root_row_menu(&mut self, response: &egui::Response) {
        response.context_menu(|ui| {
            let palette = style::palette();
            ui.label(RichText::new(".").color(palette.text_primary));
            ui.separator();
            let root_hotkey = self
                .controller
                .ui
                .sources
                .folders
                .rows
                .first()
                .and_then(|row| row.hotkey);
            self.folder_hotkey_menu(ui, Path::new(""), root_hotkey);
            if ui.button("Open in Explorer").clicked() {
                self.controller.open_folder_in_file_explorer(Path::new(""));
                ui.close();
                return;
            }
            if ui.button("New folder at root").clicked() {
                self.controller.start_new_folder_at_root();
                ui.close();
            }
        });
    }

    fn folder_hotkey_menu(&mut self, ui: &mut Ui, path: &Path, current: Option<u8>) {
        ui.menu_button("Bind hotkey", |ui| {
            for slot in 0..=9 {
                let bound = current == Some(slot);
                if ui.selectable_label(bound, slot.to_string()).clicked() {
                    self.controller.bind_folder_hotkey(path, Some(slot));
                    ui.close();
                }
            }
            if current.is_some() {
                ui.separator();
                if ui.button("Clear hotkey").clicked() {
                    self.controller.bind_folder_hotkey(path, None);
                    ui.close();
                }
            }
        });
    }
}
