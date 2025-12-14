use super::*;

impl EguiController {
    pub(in crate::egui_app::controller) fn rebuild_browser_lists(&mut self) {
        if self.ui.collections.selected_sample.is_some() {
            self.ui.browser.autoscroll = false;
        }
        self.prune_browser_selection();
        let allow_highlight = matches!(
            self.ui.focus.context,
            FocusContext::SampleBrowser | FocusContext::Waveform | FocusContext::None
        );
        let highlight_selection = self.ui.collections.selected_sample.is_none() && allow_highlight;
        let focused_index = highlight_selection
            .then_some(self.selected_row_index())
            .flatten();
        let loaded_index = highlight_selection
            .then_some(self.loaded_row_index())
            .flatten();
        self.reset_browser_ui();

        for i in 0..self.wav_entries.len() {
            let tag = self.wav_entries[i].tag;
            let flags = RowFlags {
                focused: Some(i) == focused_index,
                loaded: Some(i) == loaded_index,
            };
            self.push_browser_row(i, tag, flags);
        }
        let (visible, selected_visible, loaded_visible) =
            self.build_visible_rows(focused_index, loaded_index);
        self.ui.browser.visible = visible;
        self.ui.browser.selected_visible = selected_visible;
        self.ui.browser.loaded_visible = loaded_visible;
        let visible_len = self.ui.browser.visible.len();
        if let Some(anchor) = self.ui.browser.selection_anchor_visible
            && anchor >= visible_len
        {
            self.ui.browser.selection_anchor_visible = self.ui.browser.selected_visible;
        }
    }

    pub(in crate::egui_app::controller) fn selected_row_index(&self) -> Option<usize> {
        self.wav_selection
            .selected_wav
            .as_ref()
            .and_then(|path| self.wav_lookup.get(path).copied())
    }

    pub(in crate::egui_app::controller) fn loaded_row_index(&self) -> Option<usize> {
        self.wav_selection
            .loaded_wav
            .as_ref()
            .and_then(|path| self.wav_lookup.get(path).copied())
    }

    fn reset_browser_ui(&mut self) {
        let autoscroll = self.ui.browser.autoscroll;
        let collections_selected = self.ui.collections.selected_sample.is_some();
        self.ui.browser.trash.clear();
        self.ui.browser.neutral.clear();
        self.ui.browser.keep.clear();
        self.ui.browser.visible.clear();
        self.ui.browser.selected_visible = None;
        if collections_selected {
            self.ui.browser.selected = None;
        }
        self.ui.browser.loaded = None;
        self.ui.browser.loaded_visible = None;
        self.ui.browser.autoscroll = autoscroll && !collections_selected;
        self.ui.loaded_wav = None;
    }

    fn push_browser_row(&mut self, entry_index: usize, tag: SampleTag, flags: RowFlags) {
        let target = match tag {
            SampleTag::Trash => &mut self.ui.browser.trash,
            SampleTag::Neutral => &mut self.ui.browser.neutral,
            SampleTag::Keep => &mut self.ui.browser.keep,
        };
        let row_index = target.len();
        target.push(entry_index);
        if flags.focused {
            self.ui.browser.selected = Some(view_model::sample_browser_index_for(tag, row_index));
        }
        if flags.loaded {
            self.ui.browser.loaded = Some(view_model::sample_browser_index_for(tag, row_index));
            if let Some(path) = self.wav_entries.get(entry_index) {
                self.ui.loaded_wav = Some(path.relative_path.clone());
            }
        }
    }

    fn prune_browser_selection(&mut self) {
        self.ui
            .browser
            .selected_paths
            .retain(|path| self.wav_lookup.contains_key(path));
        if let Some(path) = self.wav_selection.selected_wav.clone()
            && !self.wav_lookup.contains_key(&path)
        {
            self.wav_selection.selected_wav = None;
            self.ui.browser.selected = None;
            self.ui.browser.selected_visible = None;
            self.clear_waveform_view();
        }
    }

    pub(in crate::egui_app::controller) fn focused_browser_row(&self) -> Option<usize> {
        self.ui.browser.selected_visible
    }

    pub(in crate::egui_app::controller) fn focused_browser_path(&self) -> Option<PathBuf> {
        let row = self.focused_browser_row()?;
        self.browser_path_for_visible(row)
    }

    pub(super) fn browser_path_for_visible(&self, visible_row: usize) -> Option<PathBuf> {
        let index = self.ui.browser.visible.get(visible_row).copied()?;
        self.wav_entries
            .get(index)
            .map(|entry| entry.relative_path.clone())
    }
}
