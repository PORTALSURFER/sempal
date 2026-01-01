use super::*;
use crate::egui_app::state::SampleBrowserSort;
use crate::egui_app::view_model;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use std::cmp::Ordering;
use std::path::Path;

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
    ) -> (crate::egui_app::state::VisibleRows, Option<usize>, Option<usize>) {
        let filter = self.ui.browser.filter;
        let filter_accepts = |tag: SampleTag| match filter {
            TriageFlagFilter::All => true,
            TriageFlagFilter::Keep => matches!(tag, SampleTag::Keep),
            TriageFlagFilter::Trash => matches!(tag, SampleTag::Trash),
            TriageFlagFilter::Untagged => matches!(tag, SampleTag::Neutral),
        };
        let folder_selection = self.folder_selection_for_filter().cloned();
        let folder_negated = self.folder_negation_for_filter().cloned();
        let has_folder_filters = folder_selection
            .as_ref()
            .is_some_and(|selection| !selection.is_empty())
            || folder_negated
                .as_ref()
                .is_some_and(|negated| !negated.is_empty());
        let folder_accepts = |relative_path: &Path| {
            crate::egui_app::controller::source_folders::folder_filter_accepts(
                relative_path,
                folder_selection.as_ref(),
                folder_negated.as_ref(),
            )
        };
        if let Some(similar) = self.ui.browser.similar_query.clone() {
            let sort_mode = self.ui.browser.sort;
            let mut visible: Vec<usize> = Vec::new();
            for index in similar.indices.iter().copied() {
                let Some(entry) = self.wav_entry(index) else {
                    continue;
                };
                let tag = entry.tag;
                let path = entry.relative_path.clone();
                if filter_accepts(tag) && folder_accepts(&path) {
                    visible.push(index);
                }
            }
            match sort_mode {
                SampleBrowserSort::ListOrder => {
                    visible.sort_unstable();
                }
                SampleBrowserSort::Similarity => {
                    visible.sort_by(|a, b| {
                        let a_score = similar.score_for_index(*a).unwrap_or(f32::NEG_INFINITY);
                        let b_score = similar.score_for_index(*b).unwrap_or(f32::NEG_INFINITY);
                        b_score
                            .partial_cmp(&a_score)
                            .unwrap_or(Ordering::Equal)
                            .then_with(|| a.cmp(b))
                    });
                    if let Some(anchor) = similar.anchor_index {
                        if let Some(entry) = self.wav_entry(anchor) {
                            let tag = entry.tag;
                            let path = entry.relative_path.clone();
                            if filter_accepts(tag) && folder_accepts(&path) {
                                if let Some(pos) = visible.iter().position(|i| *i == anchor) {
                                    visible.remove(pos);
                                }
                                visible.insert(0, anchor);
                            }
                        }
                    }
                }
            }
            let selected_visible =
                focused_index.and_then(|idx| visible.iter().position(|i| *i == idx));
            let loaded_visible =
                loaded_index.and_then(|idx| visible.iter().position(|i| *i == idx));
            return (
                crate::egui_app::state::VisibleRows::List(visible),
                selected_visible,
                loaded_visible,
            );
        }
        let Some(query) = self.active_search_query().map(str::to_string) else {
            if !has_folder_filters
                && self.ui.browser.filter == TriageFlagFilter::All
                && self.ui.browser.similar_query.is_none()
            {
                let total = self.wav_entries_len();
                return (
                    crate::egui_app::state::VisibleRows::All { total },
                    focused_index,
                    loaded_index,
                );
            }
            let mut visible = Vec::new();
            let _ = self.for_each_wav_entry(|index, entry| {
                if filter_accepts(entry.tag) && folder_accepts(&entry.relative_path) {
                    visible.push(index);
                }
            });
            let selected_visible =
                focused_index.and_then(|idx| visible.iter().position(|i| *i == idx));
            let loaded_visible =
                loaded_index.and_then(|idx| visible.iter().position(|i| *i == idx));
            return (
                crate::egui_app::state::VisibleRows::List(visible),
                selected_visible,
                loaded_visible,
            );
        };
        self.ensure_search_scores(&query);
        let scores = std::mem::take(&mut self.ui_cache.browser.search.scores);
        let mut scratch = std::mem::take(&mut self.ui_cache.browser.search.scratch);
        scratch.clear();
        scratch.reserve(self.wav_entries_len().min(1024));
        let _ = self.for_each_wav_entry(|index, entry| {
            if !filter_accepts(entry.tag) || !folder_accepts(&entry.relative_path) {
                return;
            }
            if let Some(score) = scores.get(index).and_then(|s| *s) {
                scratch.push((index, score));
            }
        });
        scratch.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        self.ui_cache.browser.search.scores = scores;
        self.ui_cache.browser.search.scratch = scratch;
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
        (
            crate::egui_app::state::VisibleRows::List(visible),
            selected_visible,
            loaded_visible,
        )
    }

    #[allow(dead_code)]
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
            || self.ui_cache.browser.search.scores.len() != self.wav_entries_len()
        {
            self.ui_cache.browser.search.source_id = source_id;
            self.ui_cache.browser.search.query.clear();
            self.ui_cache.browser.search.query.push_str(query);
            self.ui_cache.browser.search.scores.clear();
            self.ui_cache
                .browser
                .search
                .scores
                .resize(self.wav_entries_len(), None);

            let Some(source_id) = self.selection_state.ctx.selected_source.clone() else {
                return;
            };
            let needs_labels = self
                .ui_cache
                .browser
                .labels
                .get(&source_id)
                .map(|cached| cached.len() != self.wav_entries_len())
                .unwrap_or(true);
            if needs_labels {
                self.ui_cache.browser.labels.insert(source_id.clone(), Vec::new());
            }
            for index in 0..self.wav_entries_len() {
                let label = self.label_for_ref(index).map(str::to_string);
                if let Some(label) = label {
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
            .map(|cached| cached.len() != self.wav_entries_len())
            .unwrap_or(true);
        if needs_labels {
            self.ui_cache
                .browser
                .labels
                .insert(source_id.clone(), vec![String::new(); self.wav_entries_len()]);
        }
        let needs_fill = self
            .ui_cache
            .browser
            .labels
            .get(&source_id)
            .and_then(|labels| labels.get(index))
            .is_some_and(|label| label.is_empty());
        if needs_fill {
            let entry = self.wav_entry(index)?;
            let label = view_model::sample_display_label(&entry.relative_path);
            if let Some(labels) = self.ui_cache.browser.labels.get_mut(&source_id)
                && index < labels.len()
            {
                labels[index] = label;
            }
        }
        self.ui_cache
            .browser
            .labels
            .get(&source_id)
            .and_then(|labels| labels.get(index))
            .map(|label| label.as_str())
    }

}

pub(super) fn set_browser_filter(controller: &mut EguiController, filter: TriageFlagFilter) {
    if controller.ui.browser.filter != filter {
        controller.ui.browser.filter = filter;
        controller.rebuild_browser_lists();
    }
}

pub(super) fn set_browser_sort(controller: &mut EguiController, sort: SampleBrowserSort) {
    if controller.ui.browser.sort != sort {
        controller.ui.browser.sort = sort;
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
    controller.ui.browser.sort = SampleBrowserSort::ListOrder;
    controller.rebuild_browser_lists();
}
