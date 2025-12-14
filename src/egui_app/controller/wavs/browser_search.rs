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
        let Some(query) = self.active_search_query().map(str::to_string) else {
            let visible: Vec<usize> = self
                .wav_entries
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
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
        self.browser_search_cache.scratch.clear();
        self.browser_search_cache
            .scratch
            .reserve(self.wav_entries.entries.len().min(1024));

        for (index, entry) in self.wav_entries.entries.iter().enumerate() {
            if !self.browser_filter_accepts(entry.tag)
                || !self.folder_filter_accepts(&entry.relative_path)
            {
                continue;
            }
            if let Some(score) = self.browser_search_cache.scores.get(index).and_then(|s| *s) {
                self.browser_search_cache.scratch.push((index, score));
            }
        }
        self.browser_search_cache
            .scratch
            .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let visible: Vec<usize> = self
            .browser_search_cache
            .scratch
            .iter()
            .map(|(index, _)| *index)
            .collect();
        let selected_visible =
            focused_index.and_then(|idx| visible.iter().position(|i| *i == idx));
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
        let source_id = self.selection_ctx.selected_source.clone();
        if self.browser_search_cache.source_id != source_id
            || self.browser_search_cache.query != query
            || self.browser_search_cache.scores.len() != self.wav_entries.entries.len()
        {
            self.browser_search_cache.source_id = source_id;
            self.browser_search_cache.query.clear();
            self.browser_search_cache.query.push_str(query);
            self.browser_search_cache.scores.clear();
            self.browser_search_cache
                .scores
                .resize(self.wav_entries.entries.len(), None);

            let Some(source_id) = self.selection_ctx.selected_source.clone() else {
                return;
            };
            let needs_labels = self
                .label_cache
                .get(&source_id)
                .map(|cached| cached.len() != self.wav_entries.entries.len())
                .unwrap_or(true);
            if needs_labels {
                self.label_cache
                    .insert(source_id.clone(), self.build_label_cache(&self.wav_entries.entries));
            }
            let Some(labels) = self.label_cache.get(&source_id) else {
                return;
            };
            for index in 0..self.wav_entries.entries.len() {
                if let Some(label) = labels.get(index) {
                    self.browser_search_cache.scores[index] = self
                        .browser_search_cache
                        .matcher
                        .fuzzy_match(label.as_str(), query);
                }
            }
        }
    }

    pub(super) fn label_for_ref(&mut self, index: usize) -> Option<&str> {
        let source_id = self.selection_ctx.selected_source.clone()?;
        let needs_labels = self
            .label_cache
            .get(&source_id)
            .map(|cached| cached.len() != self.wav_entries.entries.len())
            .unwrap_or(true);
        if needs_labels {
            self.label_cache
                .insert(source_id.clone(), self.build_label_cache(&self.wav_entries.entries));
        }
        self.label_cache
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
