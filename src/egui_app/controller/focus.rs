use super::*;
use crate::egui_app::state::FocusContext;

impl EguiController {
    /// Mark the sample browser as the active focus surface.
    pub(super) fn focus_browser_context(&mut self) {
        self.set_focus_context(FocusContext::SampleBrowser);
    }

    /// Focus the sample browser, selecting a row if none is active.
    pub(super) fn focus_browser_list(&mut self) {
        let Some(target_row) =
            self.ui
                .browser
                .selected_visible
                .or(self.ui.browser.visible.first().copied())
        else {
            self.set_status_message(StatusMessage::AddSourceWithSamplesFirst);
            return;
        };
        // Entering via the focus hotkey should not autoplay; suppress it for this selection.
        self.selection_state.suppress_autoplay_once = true;
        self.focus_browser_row_only(target_row);
    }

    /// Mark the waveform viewer as the active focus surface.
    pub(crate) fn focus_waveform_context(&mut self) {
        self.set_focus_context(FocusContext::Waveform);
    }

    /// Mark the collections sample list as the active focus surface.
    pub(super) fn focus_collection_context(&mut self) {
        self.set_focus_context(FocusContext::CollectionSample);
    }

    /// Focus the collection samples list, selecting the current row or first row.
    pub(super) fn focus_collection_samples_list(&mut self) {
        let Some(collection) = self.current_collection() else {
            self.set_status_message(StatusMessage::SelectCollectionFirst {
                tone: StatusTone::Info,
            });
            return;
        };
        if collection.members.is_empty() {
            self.set_status_message(StatusMessage::CollectionEmpty);
            return;
        }
        let target = self
            .ui
            .collections
            .selected_sample
            .unwrap_or(0)
            .min(collection.members.len().saturating_sub(1));
        self.select_collection_sample(target);
    }

    /// Mark the sources list as the active focus surface.
    pub(super) fn focus_sources_context(&mut self) {
        self.set_focus_context(FocusContext::SourcesList);
    }

    /// Mark the source folder browser as the active focus surface.
    pub(super) fn focus_folder_context(&mut self) {
        self.set_focus_context(FocusContext::SourceFolders);
    }

    /// Focus the sources list, selecting the current row or the first available source.
    pub(super) fn focus_sources_list(&mut self) {
        if self.library.sources.is_empty() {
            self.set_status_message(StatusMessage::AddSourceFirst {
                tone: StatusTone::Info,
            });
            return;
        }
        let target = self
            .ui
            .sources
            .selected
            .unwrap_or(0)
            .min(self.library.sources.len() - 1);
        self.select_source_by_index(target);
        self.focus_sources_context();
    }

    /// Mark the collections list as the active focus surface.
    pub(super) fn focus_collections_list_context(&mut self) {
        self.set_focus_context(FocusContext::CollectionsList);
    }

    /// Focus the collections list, selecting the active row or the first entry.
    pub(super) fn focus_collections_list(&mut self) {
        if self.library.collections.is_empty() {
            self.set_status_message(StatusMessage::CreateCollectionFirst);
            return;
        }
        let target = self
            .ui
            .collections
            .selected
            .unwrap_or(0)
            .min(self.library.collections.len() - 1);
        self.select_collection_by_index(Some(target));
        self.focus_collections_list_context();
    }

    /// Clear focus when no interactive surface should process shortcuts.
    pub(super) fn clear_focus_context(&mut self) {
        self.set_focus_context(FocusContext::None);
    }

    /// Clear focus when it currently belongs to the collections list.
    pub(super) fn clear_collection_focus_context(&mut self) {
        if matches!(self.ui.focus.context, FocusContext::CollectionSample) {
            self.clear_focus_context();
        }
    }

    /// Focus a UI surface from UI-driven navigation (e.g. alt+arrow switching).
    pub(crate) fn focus_context_from_ui(&mut self, context: FocusContext) {
        match context {
            FocusContext::SampleBrowser => self.focus_browser_list(),
            FocusContext::Waveform => self.focus_waveform_context(),
            FocusContext::SourceFolders => self.focus_folder_context(),
            FocusContext::CollectionSample => self.focus_collection_samples_list(),
            FocusContext::SourcesList => self.focus_sources_list(),
            FocusContext::CollectionsList => self.focus_collections_list(),
            FocusContext::None => self.clear_focus_context(),
        }
    }

    fn set_focus_context(&mut self, context: FocusContext) {
        let previous = self.ui.focus.context;
        if previous == context {
            return;
        }
        if matches!(previous, FocusContext::SourceFolders)
            && !matches!(context, FocusContext::SourceFolders)
        {
            self.drop_folder_focus();
        }
        self.ui.focus.set_context(context);
    }
}
