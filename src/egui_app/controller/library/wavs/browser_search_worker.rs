use crate::egui_app::controller::jobs::{SearchJob, SearchResult};
use crate::egui_app::state::{SampleBrowserSort, TriageFlagFilter, VisibleRows};
use crate::sample_sources::{Rating, WavEntry};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::thread;

struct CompactSearchEntry {
    display_label: Box<str>,
    relative_path: Box<str>,
    tag: Rating,
    last_played_at: Option<i64>,
}

struct SearchWorkerCache {
    entries: Option<Vec<CompactSearchEntry>>,
    source_id: Option<String>,
    revision: u64,
}

impl Default for SearchWorkerCache {
    fn default() -> Self {
        Self {
            entries: None,
            source_id: None,
            revision: 0,
        }
    }
}

pub(crate) fn spawn_search_worker() -> (Sender<SearchJob>, Receiver<SearchResult>) {
    let (tx, rx) = std::sync::mpsc::channel::<SearchJob>();
    let (result_tx, result_rx) = std::sync::mpsc::channel::<SearchResult>();
    thread::spawn(move || {
        let matcher = SkimMatcherV2::default();
        let mut cache = SearchWorkerCache::default();
        while let Ok(job) = rx.recv() {
            let result = process_search_job(job, &matcher, &mut cache);
            let _ = result_tx.send(result);
        }
    });
    (tx, result_rx)
}

fn process_search_job(
    job: SearchJob,
    matcher: &SkimMatcherV2,
    cache: &mut SearchWorkerCache,
) -> SearchResult {
    let db_result = crate::sample_sources::SourceDatabase::open(&job.source_root);

    match db_result {
        Ok(db) => {
            let revision = db.get_revision().unwrap_or(0);
            let job_source_id_str = job.source_id.as_str().to_string();

            let must_reload = cache.entries.is_none()
                || cache.source_id.as_ref() != Some(&job_source_id_str)
                || cache.revision != revision;

            if must_reload {
                match db.list_files() {
                    Ok(loaded_entries) => {
                        let compact_entries: Vec<CompactSearchEntry> = loaded_entries
                            .into_iter()
                            .map(|e| {
                                let relative_path = e.relative_path.to_string_lossy().to_string();
                                let display_label =
                                    crate::egui_app::view_model::sample_display_label(&e.relative_path);

                                CompactSearchEntry {
                                    display_label: display_label.into_boxed_str(),
                                    relative_path: relative_path.into_boxed_str(),
                                    tag: e.tag,
                                    last_played_at: e.last_played_at,
                                }
                            })
                            .collect();
                        cache.entries = Some(compact_entries);
                        cache.source_id = Some(job_source_id_str);
                        cache.revision = revision;
                    }
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
                }
            }
        }
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

    let entries = cache.entries.as_ref().unwrap();

    let filter_accepts = |tag: Rating| match job.filter {
        TriageFlagFilter::All => true,
        TriageFlagFilter::Keep => tag.is_keep(),
        TriageFlagFilter::Trash => tag.is_trash(),
        TriageFlagFilter::Untagged => tag.is_neutral(),
    };

    let folder_accepts = |entry: &CompactSearchEntry| {
        let path = std::path::Path::new(entry.relative_path.as_ref());
        crate::egui_app::controller::library::source_folders::folder_filter_accepts(
            path,
            job.folder_selection.as_ref(),
            job.folder_negated.as_ref(),
        )
    };

    let mut scores = vec![None; entries.len()];
    let has_query = !job.query.is_empty();

    if has_query {
        for (index, entry) in entries.iter().enumerate() {
            scores[index] = matcher.fuzzy_match(&entry.display_label, &job.query);
        }
    }

    let mut visible = Vec::new();

    if let Some(similar) = &job.similar_query {
        for index in similar.indices.iter().copied() {
            if let Some(entry) = entries.get(index) {
                if filter_accepts(entry.tag) && folder_accepts(entry) {
                    visible.push(index);
                }
            }
        }

        match job.sort {
            SampleBrowserSort::Similarity => {
                let mut score_lookup = vec![None; entries.len()];
                for (&index, &score) in similar.indices.iter().zip(similar.scores.iter()) {
                    if index < score_lookup.len() {
                        score_lookup[index] = Some(score);
                    }
                }
                visible.sort_by(|a: &usize, b: &usize| {
                    let a_score = score_lookup
                        .get(*a)
                        .and_then(|s| *s)
                        .unwrap_or(f32::NEG_INFINITY);
                    let b_score = score_lookup
                        .get(*b)
                        .and_then(|s| *s)
                        .unwrap_or(f32::NEG_INFINITY);
                    b_score
                        .partial_cmp(&a_score)
                        .unwrap_or(Ordering::Equal)
                        .then_with(|| a.cmp(b))
                });

                if let Some(anchor) = similar.anchor_index {
                    if let Some(entry) = entries.get(anchor) {
                        if filter_accepts(entry.tag) && folder_accepts(entry) {
                            if let Some(pos) = visible.iter().position(|i| *i == anchor) {
                                visible.remove(pos);
                            }
                            visible.insert(0, anchor);
                        }
                    }
                }
            }
            SampleBrowserSort::PlaybackAgeAsc => {
                sort_visible_by_playback_age(entries, &mut visible, true);
            }
            SampleBrowserSort::PlaybackAgeDesc => {
                sort_visible_by_playback_age(entries, &mut visible, false);
            }
            SampleBrowserSort::ListOrder => {
                visible.sort_unstable();
            }
        }
    }

    let mut scratch = Vec::with_capacity(entries.len().min(1024));
    let mut trash = Vec::new();
    let mut neutral = Vec::new();
    let mut keep = Vec::new();

    for (index, entry) in entries.iter().enumerate() {
        if entry.tag.is_trash() {
            trash.push(index);
        } else if entry.tag.is_keep() {
            keep.push(index);
        } else {
            neutral.push(index);
        }

        if job.similar_query.is_none() && filter_accepts(entry.tag) && folder_accepts(entry) {
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
    if !has_query
        && !has_folder_filters
        && job.filter == TriageFlagFilter::All
        && job.similar_query.is_none()
        && job.sort == SampleBrowserSort::ListOrder
    {
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

    if job.similar_query.is_none() {
        match job.sort {
            SampleBrowserSort::PlaybackAgeAsc => {
                sort_visible_by_playback_age(entries, &mut visible, true);
            }
            SampleBrowserSort::PlaybackAgeDesc => {
                sort_visible_by_playback_age(entries, &mut visible, false);
            }
            _ => {}
        }
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

fn sort_visible_by_playback_age(
    entries: &[CompactSearchEntry],
    visible: &mut Vec<usize>,
    ascending: bool,
) {
    visible.sort_by(|a, b| {
        let a_key = entries
            .get(*a)
            .and_then(|entry| entry.last_played_at)
            .unwrap_or(i64::MIN);
        let b_key = entries
            .get(*b)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compact_search_entry() {
        let entries = vec![
            WavEntry {
                relative_path: std::path::PathBuf::from("kits/drums/kick.wav"),
                file_size: 100,
                modified_ns: 1,
                content_hash: None,
                tag: Rating::NEUTRAL,
                looped: false,
                missing: false,
                last_played_at: None,
            },
            WavEntry {
                relative_path: std::path::PathBuf::from("kits/drums/snare.wav"),
                file_size: 110,
                modified_ns: 2,
                content_hash: None,
                tag: Rating::NEUTRAL,
                looped: false,
                missing: false,
                last_played_at: None,
            },
        ];

        let compacts: Vec<CompactSearchEntry> = entries
            .into_iter()
            .map(|e| {
                let relative_path = e.relative_path.to_string_lossy().to_string();
                let display_label = crate::egui_app::view_model::sample_display_label(&e.relative_path);
                CompactSearchEntry {
                    display_label: display_label.into_boxed_str(),
                    relative_path: relative_path.into_boxed_str(),
                    tag: e.tag,
                    last_played_at: e.last_played_at,
                }
            })
            .collect();

        assert_eq!(compacts.len(), 2);
        assert_eq!(compacts[0].display_label.as_ref(), "kick");
        assert_eq!(compacts[1].display_label.as_ref(), "snare");
        assert_eq!(compacts[0].relative_path.as_ref(), "kits/drums/kick.wav");
    }
}
