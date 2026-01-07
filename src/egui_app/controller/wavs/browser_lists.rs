use super::*;
use crate::egui_app::state::FocusContext;

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

        if self.should_offload_search() {
            self.dispatch_search_job();
            return;
        }

        self.reset_browser_ui();
        let mut trash = Vec::new();
        let mut neutral = Vec::new();
        let mut keep = Vec::new();
        let mut selected = None;
        let mut loaded = None;
        let mut loaded_wav = None;
        let _ = self.for_each_wav_entry(|i, entry| {
            let flags = RowFlags {
                focused: Some(i) == focused_index,
                loaded: Some(i) == loaded_index,
            };
            let target = match entry.tag {
                SampleTag::Trash => &mut trash,
                SampleTag::Neutral => &mut neutral,
                SampleTag::Keep => &mut keep,
            };
            let row_index = target.len();
            target.push(i);
            if flags.focused {
                selected = Some(view_model::sample_browser_index_for(entry.tag, row_index));
            }
            if flags.loaded {
                loaded = Some(view_model::sample_browser_index_for(entry.tag, row_index));
                loaded_wav = Some(entry.relative_path.clone());
            }
        });
        self.ui.browser.trash = trash;
        self.ui.browser.neutral = neutral;
        self.ui.browser.keep = keep;
        self.ui.browser.selected = selected;
        self.ui.browser.loaded = loaded;
        self.ui.loaded_wav = loaded_wav;
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

    pub(in crate::egui_app::controller) fn selected_row_index(&mut self) -> Option<usize> {
        let selected_wav = self.sample_view.wav.selected_wav.clone();
        selected_wav
            .as_ref()
            .and_then(|path| self.wav_index_for_path(path))
    }

    pub(in crate::egui_app::controller) fn loaded_row_index(&mut self) -> Option<usize> {
        let loaded_wav = self.sample_view.wav.loaded_wav.clone();
        loaded_wav
            .as_ref()
            .and_then(|path| self.wav_index_for_path(path))
    }

    fn reset_browser_ui(&mut self) {
        let autoscroll = self.ui.browser.autoscroll;
        let collections_selected = self.ui.collections.selected_sample.is_some();
        self.ui.browser.trash.clear();
        self.ui.browser.neutral.clear();
        self.ui.browser.keep.clear();
        self.ui.browser.visible.clear_to_list();
        self.ui.browser.selected_visible = None;
        self.ui.browser.search_busy = false;
        if collections_selected {
            self.ui.browser.selected = None;
        }
        self.ui.browser.loaded = None;
        self.ui.browser.loaded_visible = None;
        self.ui.browser.autoscroll = autoscroll && !collections_selected;
        self.ui.loaded_wav = None;
    }

    #[allow(dead_code)]
    fn push_browser_row(&mut self, entry_index: usize, entry: &WavEntry, flags: RowFlags) {
        let target = match entry.tag {
            SampleTag::Trash => &mut self.ui.browser.trash,
            SampleTag::Neutral => &mut self.ui.browser.neutral,
            SampleTag::Keep => &mut self.ui.browser.keep,
        };
        let row_index = target.len();
        target.push(entry_index);
        if flags.focused {
            self.ui.browser.selected =
                Some(view_model::sample_browser_index_for(entry.tag, row_index));
        }
        if flags.loaded {
            self.ui.browser.loaded =
                Some(view_model::sample_browser_index_for(entry.tag, row_index));
            self.ui.loaded_wav = Some(entry.relative_path.clone());
        }
    }

    fn prune_browser_selection(&mut self) {
        let selected_paths = self.ui.browser.selected_paths.clone();
        let mut kept = Vec::new();
        for path in selected_paths.iter() {
            if self.wav_index_for_path(path).is_some() {
                kept.push(path.clone());
            }
        }
        self.ui.browser.selected_paths = kept;
        let selected_wav = self.sample_view.wav.selected_wav.clone();
        if let Some(path) = selected_wav
            && self.wav_index_for_path(&path).is_none()
        {
            if self.runtime.jobs.pending_select_path().as_ref() == Some(&path) {
                return;
            }
            self.sample_view.wav.selected_wav = None;
            self.ui.browser.selected = None;
            self.ui.browser.selected_visible = None;
            self.clear_waveform_view();
        }
    }

    pub(in crate::egui_app::controller) fn focused_browser_row(&self) -> Option<usize> {
        self.ui.browser.selected_visible
    }

    pub(in crate::egui_app::controller) fn focused_browser_path(&mut self) -> Option<PathBuf> {
        let row = self.focused_browser_row()?;
        self.browser_path_for_visible(row)
    }

    pub(in crate::egui_app::controller) fn browser_path_for_visible(
        &mut self,
        visible_row: usize,
    ) -> Option<PathBuf> {
        let index = self.ui.browser.visible.get(visible_row)?;
        self.wav_entry(index)
            .map(|entry| entry.relative_path.clone())
    }
}
