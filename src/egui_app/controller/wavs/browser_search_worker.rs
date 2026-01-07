use crate::egui_app::controller::jobs::{SearchJob, SearchResult};
use crate::egui_app::state::{SampleBrowserSort, TriageFlagFilter, VisibleRows};
use crate::sample_sources::{SampleTag, WavEntry};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::cmp::Ordering;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;

pub(in super::super) fn spawn_search_worker() -> (Sender<SearchJob>, Receiver<SearchResult>) {
    let (tx, rx) = std::sync::mpsc::channel::<SearchJob>();
    let (result_tx, result_rx) = std::sync::mpsc::channel::<SearchResult>();
    thread::spawn(move || {
        let matcher = SkimMatcherV2::default();
        while let Ok(job) = rx.recv() {
            let result = process_search_job(job, &matcher);
            let _ = result_tx.send(result);
        }
    });
    (tx, result_rx)
}

fn process_search_job(job: SearchJob, matcher: &SkimMatcherV2) -> SearchResult {
    let db = match crate::sample_sources::SourceDatabase::open(&job.source_root) {
        Ok(db) => db,
        Err(_) => {
            return SearchResult {
                source_id: job.source_id,
                query: job.query,
                visible: VisibleRows::List(Vec::new()),
                trash: Vec::new(),
                neutral: Vec::new(),
                keep: Vec::new(),
                scores: Vec::new(),
            };
        }
    };

    let entries = match db.list_files() {
        Ok(entries) => entries,
        Err(_) => {
            return SearchResult {
                source_id: job.source_id,
                query: job.query,
                visible: VisibleRows::List(Vec::new()),
                trash: Vec::new(),
                neutral: Vec::new(),
                keep: Vec::new(),
                scores: Vec::new(),
            };
        }
    };

    let filter_accepts = |tag: SampleTag| match job.filter {
        TriageFlagFilter::All => true,
        TriageFlagFilter::Keep => matches!(tag, SampleTag::Keep),
        TriageFlagFilter::Trash => matches!(tag, SampleTag::Trash),
        TriageFlagFilter::Untagged => matches!(tag, SampleTag::Neutral),
    };

    let folder_accepts = |relative_path: &std::path::Path| {
        crate::egui_app::controller::source_folders::folder_filter_accepts(
            relative_path,
            job.folder_selection.as_ref(),
            job.folder_negated.as_ref(),
        )
    };

    let mut scores = vec![None; entries.len()];
    let has_query = !job.query.is_empty();

    if has_query {
        for (index, entry) in entries.iter().enumerate() {
            let label = crate::egui_app::view_model::sample_display_label(&entry.relative_path);
            scores[index] = matcher.fuzzy_match(&label, &job.query);
        }
    }

    let mut visible = Vec::new();

    if let Some(similar) = &job.similar_query {
        for index in similar.indices.iter().copied() {
            if let Some(entry) = entries.get(index) {
                if filter_accepts(entry.tag) && folder_accepts(&entry.relative_path) {
                    visible.push(index);
                }
            }
        }

        if job.sort == SampleBrowserSort::Similarity {
            let mut score_lookup = vec![None; entries.len()];
            for (&index, &score) in similar.indices.iter().zip(similar.scores.iter()) {
                if index < score_lookup.len() {
                    score_lookup[index] = Some(score);
                }
            }
            visible.sort_by(|a: &usize, b: &usize| {
                let a_score = score_lookup.get(*a).and_then(|s| *s).unwrap_or(f32::NEG_INFINITY);
                let b_score = score_lookup.get(*b).and_then(|s| *s).unwrap_or(f32::NEG_INFINITY);
                b_score.partial_cmp(&a_score).unwrap_or(Ordering::Equal).then_with(|| a.cmp(b))
            });

            if let Some(anchor) = similar.anchor_index {
                if let Some(entry) = entries.get(anchor) {
                    if filter_accepts(entry.tag) && folder_accepts(&entry.relative_path) {
                        if let Some(pos) = visible.iter().position(|i| *i == anchor) {
                            visible.remove(pos);
                        }
                        visible.insert(0, anchor);
                    }
                }
            }
        } else {
            visible.sort_unstable();
        }
    }

    let mut scratch = Vec::with_capacity(entries.len().min(1024));
    let mut trash = Vec::new();
    let mut neutral = Vec::new();
    let mut keep = Vec::new();

    for (index, entry) in entries.iter().enumerate() {
        match entry.tag {
            SampleTag::Trash => trash.push(index),
            SampleTag::Neutral => neutral.push(index),
            SampleTag::Keep => keep.push(index),
        }

        if job.similar_query.is_none() && filter_accepts(entry.tag) && folder_accepts(&entry.relative_path) {
            if has_query {
                if let Some(score) = scores[index] {
                    scratch.push((index, score));
                }
            } else {
                visible.push(index);
            }
        }
    }

    if has_query && job.similar_query.is_none() {
        scratch.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        visible = scratch.into_iter().map(|(index, _)| index).collect();
    }

    let has_folder_filters = job.folder_selection.as_ref().is_some_and(|s: &std::collections::BTreeSet<std::path::PathBuf>| !s.is_empty())
        || job.folder_negated.as_ref().is_some_and(|n: &std::collections::BTreeSet<std::path::PathBuf>| !n.is_empty());
    if !has_query && !has_folder_filters && job.filter == TriageFlagFilter::All && job.similar_query.is_none() {
        return SearchResult {
            source_id: job.source_id,
            query: job.query,
            visible: VisibleRows::All {
                total: entries.len(),
            },
            trash,
            neutral,
            keep,
            scores,
        };
    }

    SearchResult {
        source_id: job.source_id,
        query: job.query,
        visible: VisibleRows::List(visible),
        trash,
        neutral,
        keep,
        scores,
    }
}
