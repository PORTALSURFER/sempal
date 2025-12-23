use super::EguiApp;
use eframe::egui;

impl EguiApp {
    pub(super) fn update_sources_panel_drop_state(
        &mut self,
        ctx: &egui::Context,
        rect: egui::Rect,
    ) -> bool {
        self.sources_panel_drop_hovered = ctx.input(|i| {
            let pointer_pos = i.pointer.hover_pos().or_else(|| i.pointer.interact_pos());
            let pointer_over = pointer_pos.is_none_or(|pos| rect.contains(pos));
            let hovered_has_path = i.raw.hovered_files.iter().any(|file| file.path.is_some());
            hovered_has_path && pointer_over
        });
        if self.sources_panel_drop_hovered {
            self.sources_panel_drop_armed = true;
        } else if ctx.input(|i| {
            i.pointer
                .hover_pos()
                .or_else(|| i.pointer.interact_pos())
                .is_some_and(|pos| !rect.contains(pos))
        }) {
            self.sources_panel_drop_armed = false;
        }
        self.sources_panel_drop_hovered
    }
}
