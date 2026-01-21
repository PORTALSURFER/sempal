use super::*;
use crate::egui_app::ui::style::StatusTone;
use crate::egui_app::state::FocusContext;

impl EguiController {
    /// Mark the sample browser as the active focus surface.
    pub(crate) fn focus_browser_context(&mut self) {
        self.set_focus_context(FocusContext::SampleBrowser);
    }

    /// Focus the sample browser, selecting a row if none is active.
    pub(crate) fn focus_browser_list(&mut self) {
        let visible_len = self.ui.browser.visible.len();
        let anchor = self
            .ui
            .browser
            .selection_anchor_visible
            .filter(|row| *row < visible_len);
        let selected = self
            .ui
            .browser
            .selected_visible
            .filter(|row| *row < visible_len);
        let last_focused_path = self.ui.browser.last_focused_path.clone();
        let selected_paths = self.ui.browser.selected_paths.clone();
        let selected_wav = self.sample_view.wav.selected_wav.clone();
        let target_row = anchor
            .or(selected)
            .or_else(|| {
                last_focused_path
                    .as_ref()
                    .and_then(|path| self.visible_row_for_path(path))
            })
            .or_else(|| {
                selected_paths
                    .iter()
                    .find_map(|path| self.visible_row_for_path(path))
            })
            .or_else(|| {
                selected_wav
                    .as_ref()
                    .and_then(|path| self.visible_row_for_path(path))
            })
            .or_else(|| self.ui.browser.visible.get(0));
        let Some(target_row) = target_row else {
            self.set_status_message(StatusMessage::AddSourceWithSamplesFirst);
            return;
        };
        // Entering via the focus hotkey should not autoplay; suppress it for this selection.
        self.selection_state.suppress_autoplay_once = true;
        self.focus_browser_row_only(target_row);
    }

    /// Focus the currently loaded sample in the browser if it is visible.
    pub(crate) fn focus_loaded_sample_in_browser(&mut self) {
        let loaded_path = self
            .sample_view
            .wav
            .loaded_wav
            .clone()
            .or_else(|| self.ui.loaded_wav.clone());
        let Some(loaded_path) = loaded_path else {
            self.set_status("Load a sample to focus it", StatusTone::Info);
            return;
        };
        let Some(target_row) = self.visible_row_for_path(&loaded_path) else {
            self.focus_browser_context();
            self.set_status(
                "Loaded sample is not visible in the browser",
                StatusTone::Info,
            );
            return;
        };
        self.selection_state.suppress_autoplay_once = true;
        self.focus_browser_row_only(target_row);
    }

    /// Mark the waveform viewer as the active focus surface.
    pub(crate) fn focus_waveform_context(&mut self) {
        self.set_focus_context(FocusContext::Waveform);
    }

    /// Mark the sources list as the active focus surface.
    pub(crate) fn focus_sources_context(&mut self) {
        self.set_focus_context(FocusContext::SourcesList);
    }

    /// Mark the source folder browser as the active focus surface.
    pub(crate) fn focus_folder_context(&mut self) {
        self.set_focus_context(FocusContext::SourceFolders);
    }

    /// Focus the sources list, selecting the current row or the first available source.
    pub(crate) fn focus_sources_list(&mut self) {
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

    /// Clear focus when no interactive surface should process shortcuts.
    pub(crate) fn clear_focus_context(&mut self) {
        self.set_focus_context(FocusContext::None);
    }

    /// Focus a UI surface from UI-driven navigation.
    pub(crate) fn focus_context_from_ui(&mut self, context: FocusContext) {
        match context {
            FocusContext::SampleBrowser => self.focus_browser_list(),
            FocusContext::Waveform => self.focus_waveform_context(),
            FocusContext::SourceFolders => {
                if self.ui.sources.folders.focused.is_none() {
                    if let Some(path) = self.ui.sources.folders.last_focused_path.clone() {
                        if let Some(index) = self
                            .ui
                            .sources
                            .folders
                            .rows
                            .iter()
                            .position(|row| row.path == path)
                        {
                            self.focus_folder_row(index);
                            return;
                        }
                    }
                    if !self.ui.sources.folders.rows.is_empty() {
                        self.focus_folder_row(0);
                        return;
                    }
                }
                self.focus_folder_context();
            }
            FocusContext::SourcesList => self.focus_sources_list(),
            FocusContext::None => self.clear_focus_context(),
        }
    }

    fn set_focus_context(&mut self, context: FocusContext) {
        let previous = self.ui.focus.context;
        if previous == context {
            return;
        }
        if matches!(previous, FocusContext::Waveform)
            && !matches!(context, FocusContext::Waveform)
        {
            let _ = self.commit_edit_selection_fades();
        }
        if matches!(previous, FocusContext::SourceFolders)
            && !matches!(context, FocusContext::SourceFolders)
        {
            self.drop_folder_focus();
        }
        self.ui.focus.set_context(context);
    }
}
