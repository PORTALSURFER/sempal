use super::*;
use crate::egui_app::view_model;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

#[derive(Default)]
pub(in super::super) struct BrowserSearchCache {
    source_id: Option<SourceId>,
    query: String,
    scores: Vec<Option<i64>>,
    scratch: Vec<(usize, i64)>,
    matcher: SkimMatcherV2,
}

impl BrowserSearchCache {
    pub(in super::super) fn invalidate(&mut self) {
        self.source_id = None;
        self.query.clear();
        self.scores.clear();
        self.scratch.clear();
    }
}

impl EguiController {
    pub(super) fn build_visible_rows(
        &mut self,
        focused_index: Option<usize>,
        loaded_index: Option<usize>,
    ) -> (Vec<usize>, Option<usize>, Option<usize>) {
        if let Some(similar) = self.ui.browser.similar_query.as_ref() {
            let mut visible: Vec<usize> = similar
                .indices
                .iter()
                .copied()
                .filter(|index| {
                    if let Some(entry) = self.wav_entries.entries.get(*index) {
                        self.browser_filter_accepts(entry.tag)
                            && self.folder_filter_accepts(&entry.relative_path)
                    } else {
                        false
                    }
                })
                .collect();
            if let Some(anchor) = similar.anchor_index {
                if let Some(entry) = self.wav_entries.entries.get(anchor) {
                    if self.browser_filter_accepts(entry.tag)
                        && self.folder_filter_accepts(&entry.relative_path)
                    {
                        if let Some(pos) = visible.iter().position(|i| *i == anchor) {
                            visible.remove(pos);
                        }
                        visible.insert(0, anchor);
                    }
                }
            }
            let selected_visible = similar
                .anchor_index
                .filter(|anchor| visible.get(0).copied() == Some(*anchor))
                .map(|_| 0)
                .or_else(|| focused_index.and_then(|idx| visible.iter().position(|i| *i == idx)));
            let loaded_visible =
                loaded_index.and_then(|idx| visible.iter().position(|i| *i == idx));
            return (visible, selected_visible, loaded_visible);
        }
        let Some(query) = self.active_search_query().map(str::to_string) else {
            let visible: Vec<usize> = self
                .wav_entries
                .entries
                .iter()
                .enumerate()
                .filter(|(_index, entry)| {
                    self.browser_filter_accepts(entry.tag)
                        && self.folder_filter_accepts(&entry.relative_path)
                })
                .map(|(index, _)| index)
                .collect();
            let selected_visible =
                focused_index.and_then(|idx| visible.iter().position(|i| *i == idx));
            let loaded_visible =
                loaded_index.and_then(|idx| visible.iter().position(|i| *i == idx));
            return (visible, selected_visible, loaded_visible);
        };
        self.ensure_search_scores(&query);
        self.ui_cache.browser.search.scratch.clear();
        self.ui_cache
            .browser
            .search
            .scratch
            .reserve(self.wav_entries.entries.len().min(1024));

        for (index, entry) in self.wav_entries.entries.iter().enumerate() {
            if !self.browser_filter_accepts(entry.tag)
                || !self.folder_filter_accepts(&entry.relative_path)
            {
                continue;
            }
            if let Some(score) = self
                .ui_cache
                .browser
                .search
                .scores
                .get(index)
                .and_then(|s| *s)
            {
                self.ui_cache.browser.search.scratch.push((index, score));
            }
        }
        self.ui_cache
            .browser
            .search
            .scratch
            .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let visible: Vec<usize> = self
            .ui_cache
            .browser
            .search
            .scratch
            .iter()
            .map(|(index, _)| *index)
            .collect();
        let selected_visible = focused_index.and_then(|idx| visible.iter().position(|i| *i == idx));
        let loaded_visible = loaded_index.and_then(|idx| visible.iter().position(|i| *i == idx));
        (visible, selected_visible, loaded_visible)
    }

    fn browser_filter_accepts(&self, tag: SampleTag) -> bool {
        match self.ui.browser.filter {
            TriageFlagFilter::All => true,
            TriageFlagFilter::Keep => matches!(tag, SampleTag::Keep),
            TriageFlagFilter::Trash => matches!(tag, SampleTag::Trash),
            TriageFlagFilter::Untagged => matches!(tag, SampleTag::Neutral),
        }
    }

    fn active_search_query(&self) -> Option<&str> {
        let query = self.ui.browser.search_query.trim();
        if query.is_empty() { None } else { Some(query) }
    }

    fn ensure_search_scores(&mut self, query: &str) {
        let source_id = self.selection_state.ctx.selected_source.clone();
        if self.ui_cache.browser.search.source_id != source_id
            || self.ui_cache.browser.search.query != query
            || self.ui_cache.browser.search.scores.len() != self.wav_entries.entries.len()
        {
            self.ui_cache.browser.search.source_id = source_id;
            self.ui_cache.browser.search.query.clear();
            self.ui_cache.browser.search.query.push_str(query);
            self.ui_cache.browser.search.scores.clear();
            self.ui_cache
                .browser
                .search
                .scores
                .resize(self.wav_entries.entries.len(), None);

            let Some(source_id) = self.selection_state.ctx.selected_source.clone() else {
                return;
            };
            let needs_labels = self
                .ui_cache
                .browser
                .labels
                .get(&source_id)
                .map(|cached| cached.len() != self.wav_entries.entries.len())
                .unwrap_or(true);
            if needs_labels {
                self.ui_cache.browser.labels.insert(
                    source_id.clone(),
                    self.build_label_cache(&self.wav_entries.entries),
                );
            }
            let Some(labels) = self.ui_cache.browser.labels.get(&source_id) else {
                return;
            };
            for index in 0..self.wav_entries.entries.len() {
                if let Some(label) = labels.get(index) {
                    self.ui_cache.browser.search.scores[index] = self
                        .ui_cache
                        .browser
                        .search
                        .matcher
                        .fuzzy_match(label.as_str(), query);
                }
            }
        }
    }

    pub(super) fn label_for_ref(&mut self, index: usize) -> Option<&str> {
        let source_id = self.selection_state.ctx.selected_source.clone()?;
        let needs_labels = self
            .ui_cache
            .browser
            .labels
            .get(&source_id)
            .map(|cached| cached.len() != self.wav_entries.entries.len())
            .unwrap_or(true);
        if needs_labels {
            self.ui_cache.browser.labels.insert(
                source_id.clone(),
                self.build_label_cache(&self.wav_entries.entries),
            );
        }
        self.ui_cache
            .browser
            .labels
            .get(&source_id)
            .and_then(|labels| labels.get(index).map(|s| s.as_str()))
    }

    pub(in super::super) fn build_label_cache(&self, entries: &[WavEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| view_model::sample_display_label(&entry.relative_path))
            .collect()
    }
}

pub(super) fn set_browser_filter(controller: &mut EguiController, filter: TriageFlagFilter) {
    if controller.ui.browser.filter != filter {
        controller.ui.browser.filter = filter;
        controller.rebuild_browser_lists();
    }
}

pub(super) fn focus_browser_search(controller: &mut EguiController) {
    controller.ui.browser.search_focus_requested = true;
    controller.focus_browser_context();
}

pub(super) fn set_browser_search(controller: &mut EguiController, query: impl Into<String>) {
    let query = query.into();
    if controller.ui.browser.search_query == query {
        return;
    }
    controller.ui.browser.search_query = query;
    controller.ui.browser.similar_query = None;
    controller.rebuild_browser_lists();
}
