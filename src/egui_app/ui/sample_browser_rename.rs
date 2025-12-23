use super::helpers::{InlineTextEditAction, render_inline_text_edit};
use super::*;
use crate::egui_app::state::SampleBrowserActionPrompt;
use eframe::egui::{self, Ui};

impl EguiApp {
    pub(super) fn render_browser_rename_editor(
        &mut self,
        ui: &mut Ui,
        row_response: &egui::Response,
        padding: f32,
        number_width: f32,
        number_gap: f32,
        trailing_space: f32,
    ) {
        let Some(prompt) = self.controller.ui.browser.pending_action.as_mut() else {
            return;
        };
        let name = match prompt {
            SampleBrowserActionPrompt::Rename { name, .. } => name,
        };
        let mut edit_rect = row_response.rect;
        edit_rect.min.x += number_width + number_gap + padding;
        edit_rect.max.x -= padding + trailing_space;
        edit_rect.min.y += 2.0;
        edit_rect.max.y -= 2.0;
        match render_inline_text_edit(
            ui,
            edit_rect,
            name,
            "Rename sample",
            &mut self.controller.ui.browser.rename_focus_requested,
        ) {
            InlineTextEditAction::Submit => self.controller.apply_pending_browser_rename(),
            InlineTextEditAction::Cancel => self.controller.cancel_browser_rename(),
            InlineTextEditAction::None => {}
        }
    }
}

