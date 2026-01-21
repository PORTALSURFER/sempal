use crate::egui_app::controller::jobs::{SearchJob, SearchResult};
use crate::egui_app::state::{SampleBrowserSort, TriageFlagFilter, VisibleRows};
use crate::sample_sources::Rating;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::SystemTime;
use tracing::warn;

struct CompactSearchEntry {
    display_label: Box<str>,
    relative_path: Box<str>,
    tag: Rating,
    last_played_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DbFileStamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl DbFileStamp {
    fn from_path(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        let modified = metadata.modified().ok();
        Some(Self {
            modified,
            len: metadata.len(),
        })
    }
}

struct SearchWorkerCache {
    db: Option<crate::sample_sources::SourceDatabase>,
    entries: Option<Vec<CompactSearchEntry>>,
    source_id: Option<String>,
    source_root: Option<PathBuf>,
    revision: u64,
    db_stamp: Option<DbFileStamp>,
}

impl Default for SearchWorkerCache {
    fn default() -> Self {
        Self {
            db: None,
            entries: None,
            source_id: None,
            source_root: None,
            revision: 0,
            db_stamp: None,
        }
    }
}

#[derive(Default)]
struct SearchJobQueueState {
    pending: Option<SearchJob>,
    poisoned_recovered: bool,
}

/// Latest-only queue for browser search jobs.
struct SearchJobQueue {
    state: Mutex<SearchJobQueueState>,
    ready: Condvar,
}

impl SearchJobQueue {
    fn new() -> Self {
        Self {
            state: Mutex::new(SearchJobQueueState::default()),
            ready: Condvar::new(),
        }
    }

    fn send(&self, job: SearchJob) {
        let mut state = self.lock_state();
        state.pending = Some(job);
        self.ready.notify_one();
    }

    fn take_blocking(&self) -> SearchJob {
        let mut state = self.lock_state();
        loop {
            if let Some(job) = state.pending.take() {
                return job;
            }
            state = self.wait_ready(state);
        }
    }

    #[cfg(test)]
    fn try_take(&self) -> Option<SearchJob> {
        let mut state = self.lock_state();
        state.pending.take()
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, SearchJobQueueState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => self.recover_state("lock", poisoned),
        }
    }

    fn wait_ready<'a>(
        &self,
        guard: std::sync::MutexGuard<'a, SearchJobQueueState>,
    ) -> std::sync::MutexGuard<'a, SearchJobQueueState> {
        self.ready
            .wait(guard)
            .unwrap_or_else(|poisoned| self.recover_state("condvar", poisoned))
    }

    fn recover_state<'a>(
        &self,
        context: &'static str,
        poisoned: std::sync::PoisonError<std::sync::MutexGuard<'a, SearchJobQueueState>>,
    ) -> std::sync::MutexGuard<'a, SearchJobQueueState> {
        let mut guard = poisoned.into_inner();
        if !guard.poisoned_recovered {
            warn!(
                "Search job queue {context} poisoned; recovering and clearing pending job."
            );
            guard.pending = None;
            guard.poisoned_recovered = true;
        }
        guard
    }
}

/// Sender handle for coalesced search jobs.
#[derive(Clone)]
pub(crate) struct SearchJobSender {
    queue: Arc<SearchJobQueue>,
}

impl SearchJobSender {
    /// Replace any pending search job with the latest request.
    pub(crate) fn send(&self, job: SearchJob) {
        self.queue.send(job);
    }
}

/// Spawn a background worker that processes the latest pending search job.
pub(crate) fn spawn_search_worker() -> (SearchJobSender, Receiver<SearchResult>) {
    let queue = Arc::new(SearchJobQueue::new());
    let sender = SearchJobSender {
        queue: Arc::clone(&queue),
    };
    let (result_tx, result_rx) = std::sync::mpsc::channel::<SearchResult>();
    thread::spawn(move || {
        let matcher = SkimMatcherV2::default();
        let mut cache = SearchWorkerCache::default();
        loop {
            let job = queue.take_blocking();
            let result = process_search_job(job, &matcher, &mut cache);
            let _ = result_tx.send(result);
        }
    });
    (sender, result_rx)
}

fn process_search_job(
    job: SearchJob,
    matcher: &SkimMatcherV2,
    cache: &mut SearchWorkerCache,
) -> SearchResult {
    let job_source_id_str = job.source_id.as_str().to_string();
    let db_path = crate::sample_sources::database_path_for(&job.source_root);
    let db_stamp = DbFileStamp::from_path(&db_path);

    let must_reopen = cache.db.is_none()
        || cache.source_id.as_ref() != Some(&job_source_id_str)
        || cache.source_root.as_ref() != Some(&job.source_root)
        || cache.db_stamp.as_ref() != db_stamp.as_ref();

    if must_reopen {
        match crate::sample_sources::SourceDatabase::open_read_only(&job.source_root) {
            Ok(db) => {
                cache.db = Some(db);
                cache.entries = None;
                cache.revision = 0;
                cache.source_id = Some(job_source_id_str.clone());
                cache.source_root = Some(job.source_root.clone());
                cache.db_stamp = db_stamp;
            }
            Err(_) => {
                cache.db = None;
                cache.entries = None;
                cache.revision = 0;
                cache.source_id = Some(job_source_id_str);
                cache.source_root = Some(job.source_root.clone());
                cache.db_stamp = db_stamp;
                return empty_search_result(job);
            }
        }
    }

    let db = match cache.db.as_ref() {
        Some(db) => db,
        None => return empty_search_result(job),
    };

    let revision = db.get_revision().unwrap_or(0);
    let must_reload = cache.entries.is_none() || cache.revision != revision;

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
                cache.revision = revision;
            }
            Err(_) => {
                cache.entries = None;
                return empty_search_result(job);
            }
        }
    }

    let entries = cache.entries.as_ref().unwrap();

    let filter_accepts = |tag: Rating| {
        let triage_ok = match job.filter {
            TriageFlagFilter::All => true,
            TriageFlagFilter::Keep => tag.is_keep(),
            TriageFlagFilter::Trash => tag.is_trash(),
            TriageFlagFilter::Untagged => tag.is_neutral(),
        };
        let rating_ok = job.rating_filter.is_empty()
            || job.rating_filter.contains(&tag.val());
        triage_ok && rating_ok
    };

    let folder_accepts = |entry: &CompactSearchEntry| {
        let path = std::path::Path::new(entry.relative_path.as_ref());
        crate::egui_app::controller::library::source_folders::folder_filter_accepts(
            path,
            job.folder_selection.as_ref(),
            job.folder_negated.as_ref(),
            job.root_mode,
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

    let has_folder_filters =
        crate::egui_app::controller::library::source_folders::folder_filters_active(
            job.folder_selection.as_ref(),
            job.folder_negated.as_ref(),
            job.root_mode,
        );
    if !has_query
        && !has_folder_filters
        && job.filter == TriageFlagFilter::All
        && job.similar_query.is_none()
        && job.sort == SampleBrowserSort::ListOrder
        && job.rating_filter.is_empty()
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

fn empty_search_result(job: SearchJob) -> SearchResult {
    SearchResult {
        source_id: job.source_id,
        query: job.query,
        visible: VisibleRows::List(Vec::new()),
        trash: Vec::new(),
        neutral: Vec::new(),
        keep: Vec::new(),
        scores: Vec::new(),
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
    use crate::sample_sources::WavEntry;
    use crate::sample_sources::SourceId;
    use std::collections::BTreeSet;
    use std::sync::mpsc;
    use std::time::Duration;

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

    #[test]
    fn latest_search_job_replaces_pending() {
        let queue = Arc::new(SearchJobQueue::new());
        let sender = SearchJobSender {
            queue: Arc::clone(&queue),
        };

        let first = make_search_job("first", "one");
        let second = make_search_job("second", "two");

        sender.send(first);
        sender.send(second);

        let pending = queue.try_take().expect("expected pending search job");
        assert_eq!(pending.query, "second");
        assert!(queue.try_take().is_none());
    }

    #[test]
    fn search_queue_recovers_from_poison() {
        let queue = Arc::new(SearchJobQueue::new());
        let queue_for_panic = Arc::clone(&queue);
        let _ = std::panic::catch_unwind(move || {
            let _guard = queue_for_panic.state.lock().expect("queue lock failed");
            panic!("poison search job queue");
        });

        let (tx, rx) = mpsc::channel();
        let queue_for_worker = Arc::clone(&queue);
        let handle = thread::spawn(move || {
            let job = queue_for_worker.take_blocking();
            tx.send(job.query).expect("send result");
        });

        queue.send(make_search_job("recovered", "root"));

        let received = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("search job never received");
        assert_eq!(received, "recovered");
        handle.join().expect("worker thread panicked");
    }

    fn make_search_job(query: &str, root: &str) -> SearchJob {
        SearchJob {
            source_id: SourceId::new(),
            source_root: std::path::PathBuf::from(root),
            query: query.to_string(),
            filter: TriageFlagFilter::All,
            rating_filter: BTreeSet::new(),
            sort: SampleBrowserSort::ListOrder,
            similar_query: None,
            folder_selection: None,
            folder_negated: None,
            root_mode: crate::egui_app::state::RootFolderFilterMode::AllDescendants,
        }
    }
}
