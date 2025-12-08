use super::*;
use crate::egui_app::state::FocusContext;

impl EguiController {
    /// Mark the sample browser as the active focus surface.
    pub(super) fn focus_browser_context(&mut self) {
        self.ui.focus.set_context(FocusContext::SampleBrowser);
    }

    /// Mark the collections sample list as the active focus surface.
    pub(super) fn focus_collection_context(&mut self) {
        self.ui.focus.set_context(FocusContext::CollectionSample);
    }

    /// Clear focus when no interactive surface should process shortcuts.
    pub(super) fn clear_focus_context(&mut self) {
        self.ui.focus.set_context(FocusContext::None);
    }

    /// Clear focus when it currently belongs to the collections list.
    pub(super) fn clear_collection_focus_context(&mut self) {
        if matches!(self.ui.focus.context, FocusContext::CollectionSample) {
            self.clear_focus_context();
        }
    }
}
