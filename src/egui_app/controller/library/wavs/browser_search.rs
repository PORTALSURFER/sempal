use super::*;
use crate::egui_app::state::SampleBrowserSort;
use crate::egui_app::view_model;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use std::cmp::Ordering;
use std::path::Path;

#[derive(Default)]
pub(crate) struct BrowserSearchCache {
    source_id: Option<SourceId>,
    query: String,
    pub(crate) scores: Vec<Option<i64>>,
    scratch: Vec<(usize, i64)>,
    pub(crate) matcher: SkimMatcherV2,
}

impl BrowserSearchCache {
    pub(crate) fn invalidate(&mut self) {
        self.source_id = None;
        self.query.clear();
        self.scores.clear();
        self.scratch.clear();
    }
}

impl EguiController {
    pub(crate) fn build_visible_rows(
        &mut self,
        focused_index: Option<usize>,
        loaded_index: Option<usize>,
    ) -> (
        crate::egui_app::state::VisibleRows,
        Option<usize>,
        Option<usize>,
    ) {
        let filter = self.ui.browser.filter;
        let filter_accepts = |tag: crate::sample_sources::Rating| match filter {
            TriageFlagFilter::All => true,
            TriageFlagFilter::Keep => tag.is_keep(),
            TriageFlagFilter::Trash => tag.is_trash(),
            TriageFlagFilter::Untagged => tag.is_neutral(),
        };
        let folder_selection = self.folder_selection_for_filter().cloned();
        let folder_negated = self.folder_negation_for_filter().cloned();
        let root_mode = self
            .root_folder_filter_mode_for_filter()
            .unwrap_or_default();
        let has_folder_filters = crate::egui_app::controller::library::source_folders::folder_filters_active(
            folder_selection.as_ref(),
            folder_negated.as_ref(),
            root_mode,
        );
        let folder_accepts = |relative_path: &Path| {
            crate::egui_app::controller::library::source_folders::folder_filter_accepts(
                relative_path,
                folder_selection.as_ref(),
                folder_negated.as_ref(),
                root_mode,
            )
        };
        let sort_mode = self.ui.browser.sort;
        if let Some(similar) = self.ui.browser.similar_query.clone() {
            let mut visible: Vec<usize> = Vec::new();
            for index in similar.indices.iter().copied() {
                let Some(entry) = self.wav_entry(index) else {
                    continue;
                };
                if filter_accepts(entry.tag) && folder_accepts(&entry.relative_path) {
                    visible.push(index);
                }
            }
            match sort_mode {
                SampleBrowserSort::ListOrder => {
                    visible.sort_unstable();
                }
                SampleBrowserSort::Similarity => {
                    let mut score_lookup = vec![None; self.wav_entries_len()];
                    for (&index, &score) in similar.indices.iter().zip(similar.scores.iter()) {
                        if index < score_lookup.len() {
                            score_lookup[index] = Some(score);
                        }
                    }
                    visible.sort_by(|a, b| {
                        let a_score = score_lookup
                            .get(*a)
                            .and_then(|score| *score)
                            .unwrap_or(f32::NEG_INFINITY);
                        let b_score = score_lookup
                            .get(*b)
                            .and_then(|score| *score)
                            .unwrap_or(f32::NEG_INFINITY);
                        b_score
                            .partial_cmp(&a_score)
                            .unwrap_or(Ordering::Equal)
                            .then_with(|| a.cmp(b))
                    });
                    if let Some(anchor) = similar.anchor_index {
                        if let Some(entry) = self.wav_entry(anchor) {
                            if filter_accepts(entry.tag) && folder_accepts(&entry.relative_path) {
                                if let Some(pos) = visible.iter().position(|i| *i == anchor) {
                                    visible.remove(pos);
                                }
                                visible.insert(0, anchor);
                            }
                        }
                    }
                }
                SampleBrowserSort::PlaybackAgeAsc => {
                    sort_visible_by_playback_age(self, &mut visible, true);
                }
                SampleBrowserSort::PlaybackAgeDesc => {
                    sort_visible_by_playback_age(self, &mut visible, false);
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
                && sort_mode == SampleBrowserSort::ListOrder
            {
                let total = self.wav_entries_len();
                return (
                    crate::egui_app::state::VisibleRows::All { total },
                    focused_index,
                    loaded_index,
                );
            }
            let mut visible = Vec::new();
            let mut playback_scratch = Vec::new();
            let _ = self.for_each_wav_entry(|index, entry| {
                if filter_accepts(entry.tag) && folder_accepts(&entry.relative_path) {
                    if matches!(
                        sort_mode,
                        SampleBrowserSort::PlaybackAgeAsc | SampleBrowserSort::PlaybackAgeDesc
                    ) {
                        playback_scratch.push((index, entry.last_played_at.unwrap_or(i64::MIN)));
                    } else {
                        visible.push(index);
                    }
                }
            });
            if matches!(
                sort_mode,
                SampleBrowserSort::PlaybackAgeAsc | SampleBrowserSort::PlaybackAgeDesc
            ) {
                let ascending = sort_mode == SampleBrowserSort::PlaybackAgeAsc;
                playback_scratch.sort_by(|a, b| {
                    let order = if ascending {
                        a.1.cmp(&b.1)
                    } else {
                        b.1.cmp(&a.1)
                    };
                    order.then_with(|| a.0.cmp(&b.0))
                });
                visible = playback_scratch.into_iter().map(|(index, _)| index).collect();
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
        let mut visible = visible;
        if matches!(
            sort_mode,
            SampleBrowserSort::PlaybackAgeAsc | SampleBrowserSort::PlaybackAgeDesc
        ) {
            let ascending = sort_mode == SampleBrowserSort::PlaybackAgeAsc;
            sort_visible_by_playback_age(self, &mut visible, ascending);
        }
        let selected_visible = focused_index.and_then(|idx| visible.iter().position(|i| *i == idx));
        let loaded_visible = loaded_index.and_then(|idx| visible.iter().position(|i| *i == idx));
        (
            crate::egui_app::state::VisibleRows::List(visible),
            selected_visible,
            loaded_visible,
        )
    }

    pub(crate) fn should_offload_search(&self) -> bool {
        self.wav_entries_len() > 5000
    }

    #[allow(dead_code)]
    fn browser_filter_accepts(&self, tag: crate::sample_sources::Rating) -> bool {
        match self.ui.browser.filter {
            TriageFlagFilter::All => true,
            TriageFlagFilter::Keep => tag.is_keep(),
            TriageFlagFilter::Trash => tag.is_trash(),
            TriageFlagFilter::Untagged => tag.is_neutral(),
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
                self.ui_cache
                    .browser
                    .labels
                    .insert(source_id.clone(), Vec::new());
            }
            let mut label_strings: Vec<Option<String>> = Vec::with_capacity(self.wav_entries_len());
            for idx in 0..self.wav_entries_len() {
                let lbl = self.label_for_ref(idx).map(|s| s.to_string());
                label_strings.push(lbl);
            }

            let mut new_scores: Vec<Option<i64>> = Vec::with_capacity(label_strings.len());
            for lbl_opt in label_strings {
                if let Some(lbl_str) = lbl_opt {
                    let score = self.ui_cache.browser.search.matcher.fuzzy_match(&lbl_str, query);
                    new_scores.push(score);
                } else {
                    new_scores.push(None);
                }
            }
            self.ui_cache.browser.search.scores = new_scores;
        }
    }

    pub(crate) fn label_for_ref(&mut self, index: usize) -> Option<&str> {
        let source_id = self.selection_state.ctx.selected_source.clone()?;
        let needs_labels = self
            .ui_cache
            .browser
            .labels
            .get(&source_id)
            .map(|cached| cached.len() != self.wav_entries_len())
            .unwrap_or(true);
        if needs_labels {
            self.ui_cache.browser.labels.insert(
                source_id.clone(),
                vec![String::new(); self.wav_entries_len()],
            );
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

    pub(crate) fn dispatch_search_job(&mut self) {
        let Some(source) = self.current_source() else {
            return;
        };
        let query = self.ui.browser.search_query.clone();
        let filter = self.ui.browser.filter;
        let sort = self.ui.browser.sort;
        let similar_query = self.ui.browser.similar_query.clone();
        let folder_selection = self.folder_selection_for_filter().cloned();
        let folder_negated = self.folder_negation_for_filter().cloned();
        let root_mode = self
            .root_folder_filter_mode_for_filter()
            .unwrap_or_default();

        self.ui.browser.search_busy = true;
        self.runtime.jobs.send_search_job(crate::egui_app::controller::jobs::SearchJob {
            source_id: source.id.clone(),
            source_root: source.root.clone(),
            query,
            filter,
            sort,
            similar_query,
            folder_selection,
            folder_negated,
            root_mode,
        });
    }
}

pub(crate) fn set_browser_filter(controller: &mut EguiController, filter: TriageFlagFilter) {
    if controller.ui.browser.filter != filter {
        controller.ui.browser.filter = filter;
        controller.rebuild_browser_lists();
    }
}

pub(crate) fn set_browser_sort(controller: &mut EguiController, sort: SampleBrowserSort) {
    if controller.ui.browser.sort != sort {
        controller.ui.browser.sort = sort;
        if sort != SampleBrowserSort::Similarity {
            controller.ui.browser.similarity_sort_follow_loaded = false;
        }
        controller.rebuild_browser_lists();
    }
}

pub(crate) fn focus_browser_search(controller: &mut EguiController) {
    controller.ui.browser.search_focus_requested = true;
    controller.focus_browser_context();
}

pub(crate) fn set_browser_search(controller: &mut EguiController, query: impl Into<String>) {
    let query = query.into();
    if controller.ui.browser.search_query == query {
        return;
    }
    controller.ui.browser.search_query = query;
    controller.ui.browser.similar_query = None;
    controller.ui.browser.sort = SampleBrowserSort::ListOrder;
    controller.ui.browser.similarity_sort_follow_loaded = false;
    controller.rebuild_browser_lists();
}

fn sort_visible_by_playback_age(
    controller: &mut EguiController,
    visible: &mut Vec<usize>,
    ascending: bool,
) {
    visible.sort_by(|a, b| {
        let a_key = controller
            .wav_entry(*a)
            .and_then(|entry| entry.last_played_at)
            .unwrap_or(i64::MIN);
        let b_key = controller
            .wav_entry(*b)
            .and_then(|entry| entry.last_played_at)
            .unwrap_or(i64::MIN);
        let order = if ascending {
            a_key.cmp(&b_key)
        } else {
            b_key.cmp(&a_key)
        };
        order.then_with(|| a.cmp(b))
    });
}
