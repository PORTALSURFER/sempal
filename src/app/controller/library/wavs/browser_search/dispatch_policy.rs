//! Async/offload policy and worker-dispatch helpers for browser search.

use super::*;
use crate::app::state::ProgressTaskKind;
use std::collections::BTreeMap;
#[cfg(not(test))]
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(test)]
use std::{cell::Cell, thread_local};

/// Environment override for the browser-search offload threshold.
const SEARCH_OFFLOAD_THRESHOLD_ENV: &str = "WAVECRATE_BROWSER_SEARCH_OFFLOAD_THRESHOLD";
/// Environment override for enabling/disabling async browser search for UI interactions.
const SEARCH_ASYNC_PIPELINE_ENV: &str = "WAVECRATE_BROWSER_ASYNC_PIPELINE";
/// Default wav-entry count threshold above which search work offloads to jobs.
const DEFAULT_SEARCH_OFFLOAD_THRESHOLD: usize = 5_000;

impl AppController {
    /// Return `true` when browser search should run through the async job path.
    pub(crate) fn should_offload_search(&self) -> bool {
        self.wav_entries_len() > browser_search_offload_threshold()
    }

    /// Return `true` when runtime browser interactions should use the async worker pipeline.
    pub(crate) fn should_dispatch_browser_search_async(&self) -> bool {
        browser_async_pipeline_enabled()
    }

    /// Return `true` when browser list rebuilds should defer to the async worker pipeline.
    ///
    /// Runtime builds treat the worker pipeline as authoritative. Tests can still
    /// force the synchronous retained pipeline off for deterministic immediate
    /// assertions, while oversized lists continue to offload through jobs even
    /// when the explicit async toggle is disabled.
    pub(crate) fn should_rebuild_browser_lists_async(&self) -> bool {
        self.should_dispatch_browser_search_async() || self.should_offload_search()
    }

    /// Enqueue the authoritative browser-search worker job for the current browser state.
    pub(crate) fn dispatch_search_job(&mut self) {
        self.dispatch_search_job_with_metadata_delta(Vec::new());
    }

    /// Retire any queued async browser-search result before applying a local projection.
    pub(crate) fn invalidate_async_browser_search_for_local_projection(&mut self) {
        self.ui.browser.search.latest_search_request_id = self
            .ui
            .browser
            .search
            .latest_search_request_id
            .wrapping_add(1);
        self.ui.browser.search.latest_applied_search_request_id =
            self.ui.browser.search.latest_search_request_id;
        self.runtime
            .browser
            .pending_search_metadata_delta_paths
            .clear();
    }

    /// Enqueue one authoritative browser-search worker job plus optional metadata-only row deltas.
    pub(crate) fn dispatch_search_job_with_metadata_delta(
        &mut self,
        metadata_delta_paths: Vec<PathBuf>,
    ) {
        let Some(source) = self.current_source() else {
            self.mark_browser_search_projection_revision_dirty();
            self.ui.browser.search.search_busy = false;
            self.clear_progress_task(ProgressTaskKind::Search);
            return;
        };
        let source_id = source.id.clone();
        let source_root = source.root.clone();
        self.ui.browser.search.latest_search_request_id = self
            .ui
            .browser
            .search
            .latest_search_request_id
            .wrapping_add(1);
        let request_id = self.ui.browser.search.latest_search_request_id;
        let query = self.ui.browser.search.search_query.clone();
        let filter = self.ui.browser.search.filter;
        let rating_filter = self.ui.browser.search.rating_filter.clone();
        let playback_age_filter = self.ui.browser.search.playback_age_filter.clone();
        let sidebar_filters = self.ui.browser.search.sidebar_filters.clone();
        let sidebar_bpm_values = self.sidebar_bpm_values_for_search_job(&sidebar_filters);
        let tag_named_filter = self.ui.browser.search.tag_named_filter;
        let sort = self.ui.browser.search.sort;
        let similar_query = self.ui.browser.search.similar_query.clone();
        let duplicate_cleanup = self.ui.browser.duplicate_cleanup.clone();
        let folder_selection = self.folder_selection_for_filter().cloned();
        let folder_negated = self.folder_negation_for_filter().cloned();
        let file_scope_mode = self.folder_file_scope_mode_for_filter().unwrap_or_default();
        let playback_age_now_unix_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.mark_browser_search_projection_revision_dirty();
        self.ui.browser.search.search_busy = true;
        self.show_status_progress(ProgressTaskKind::Search, "Filtering samples", 0, false);
        if query.trim().is_empty() {
            self.update_progress_detail_for_task(
                ProgressTaskKind::Search,
                format!("Rebuilding browser rows for {}", source_root.display()),
            );
        } else {
            self.update_progress_detail_for_task(
                ProgressTaskKind::Search,
                format!("Filtering '{query}' in {}", source_root.display()),
            );
        }
        if !metadata_delta_paths.is_empty() {
            self.runtime
                .browser
                .pending_search_metadata_delta_paths
                .extend(metadata_delta_paths);
        }
        let metadata_delta_paths = self
            .runtime
            .browser
            .pending_search_metadata_delta_paths
            .iter()
            .cloned()
            .collect();
        self.runtime
            .jobs
            .send_search_job(crate::app::controller::jobs::SearchJob {
                request_id,
                source_id,
                source_root,
                query,
                filter,
                rating_filter,
                playback_age_filter,
                tag_named_filter,
                sidebar_filters,
                sidebar_bpm_values,
                sort,
                similar_query,
                duplicate_cleanup,
                folder_selection,
                folder_negated,
                file_scope_mode,
                metadata_delta_paths,
                playback_age_now_unix_secs,
            });
    }

    /// Collect BPM values needed by sidebar BPM facets for the worker job snapshot.
    fn sidebar_bpm_values_for_search_job(
        &mut self,
        sidebar_filters: &crate::app::state::BrowserSidebarFilterState,
    ) -> BTreeMap<PathBuf, Option<f32>> {
        if !sidebar_filters.needs_bpm_metadata() {
            return BTreeMap::new();
        }
        let Some(snapshot) = self.current_browser_feature_cache_snapshot() else {
            return BTreeMap::new();
        };
        let paths = snapshot.entry_paths.to_vec();
        self.preload_bpm_values_for_paths(&paths);
        let Some(source_id) = self.current_source().map(|source| source.id.clone()) else {
            return BTreeMap::new();
        };
        let Some(cache) = self.ui_cache.browser.bpm_values.get(&source_id) else {
            return BTreeMap::new();
        };
        paths
            .into_iter()
            .map(|path| {
                let bpm = cache.get(&path).copied().flatten();
                (path, bpm)
            })
            .collect()
    }
}

/// Resolve the wav-entry threshold for switching browser search to async jobs.
fn browser_search_offload_threshold() -> usize {
    #[cfg(test)]
    {
        DEFAULT_SEARCH_OFFLOAD_THRESHOLD
    }
    #[cfg(not(test))]
    {
        /// Cached parsed offload threshold for browser search jobs.
        static OFFLOAD_THRESHOLD: OnceLock<usize> = OnceLock::new();
        *OFFLOAD_THRESHOLD.get_or_init(|| {
            std::env::var(SEARCH_OFFLOAD_THRESHOLD_ENV)
                .ok()
                .and_then(|value| value.trim().parse::<usize>().ok())
                .filter(|threshold| *threshold > 0)
                .unwrap_or(DEFAULT_SEARCH_OFFLOAD_THRESHOLD)
        })
    }
}

/// Resolve whether browser interaction paths should use the async worker pipeline.
///
/// This defaults to `true` for runtime builds and `false` under libtest so
/// tests keep deterministic immediate list updates unless they explicitly opt in.
fn browser_async_pipeline_enabled() -> bool {
    #[cfg(test)]
    {
        browser_async_pipeline_override_for_tests().unwrap_or(false)
    }
    #[cfg(not(test))]
    {
        /// Cached parsed async pipeline override for browser interactions.
        static ASYNC_PIPELINE_ENABLED: OnceLock<bool> = OnceLock::new();
        *ASYNC_PIPELINE_ENABLED.get_or_init(|| {
            std::env::var(SEARCH_ASYNC_PIPELINE_ENV)
                .ok()
                .as_deref()
                .and_then(crate::env_flags::parse_env_bool)
                .unwrap_or(true)
        })
    }
}

#[cfg(test)]
thread_local! {
    /// Per-test-thread override for forcing the browser async dispatch path.
    static BROWSER_ASYNC_PIPELINE_OVERRIDE: Cell<Option<bool>> = const { Cell::new(None) };
}

#[cfg(test)]
fn browser_async_pipeline_override_for_tests() -> Option<bool> {
    BROWSER_ASYNC_PIPELINE_OVERRIDE.with(|value| value.get())
}

#[cfg(test)]
/// Run one test closure with the browser async pipeline forced on or off.
pub(crate) fn with_browser_async_pipeline_enabled_for_tests<T>(
    enabled: bool,
    run: impl FnOnce() -> T,
) -> T {
    struct Reset<'a> {
        cell: &'a Cell<Option<bool>>,
        previous: Option<bool>,
    }

    impl Drop for Reset<'_> {
        fn drop(&mut self) {
            self.cell.set(self.previous);
        }
    }

    BROWSER_ASYNC_PIPELINE_OVERRIDE.with(|value| {
        let previous = value.replace(Some(enabled));
        let _reset = Reset {
            cell: value,
            previous,
        };
        run()
    })
}
