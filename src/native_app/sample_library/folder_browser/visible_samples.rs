use radiant::prelude as ui;
use std::{
    cell::{Ref, RefCell},
    collections::{BTreeMap, HashMap, HashSet},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use super::{
    FileColumn, FileEntry, FileRenameView, FolderBrowserState, SimilarityAspectStrengths,
    SimilarityBrowserState, default_file_columns, listing::BrowserListingRevealState,
    starmap::StarmapLayoutCache,
};
use wavecrate::sample_sources::{HarvestState, config::SimilarityAspectSettings};

const COPY_FLASH_FRAMES: u8 = 12;
const PROTECTED_SOURCE_ERROR_FLASH_FRAMES: u8 = 24;
const PRIMARY_SOURCE_ACCEPTANCE_FLASH_DURATION: Duration = Duration::from_secs(1);
const SLOW_SAMPLE_PROJECTION_CACHE_FILL: Duration = Duration::from_millis(4);

#[derive(Clone, Copy)]
pub(in crate::native_app) struct VisibleSampleQuery<'a> {
    pub(in crate::native_app) tags_by_file: &'a HashMap<String, Vec<String>>,
    pub(in crate::native_app) cached_sample_paths: &'a HashSet<String>,
}

#[derive(Clone, Copy)]
pub(in crate::native_app) struct VisibleSampleWindowPolicy<'a> {
    pub(in crate::native_app) tags_by_file: &'a HashMap<String, Vec<String>>,
    pub(in crate::native_app) viewport_rows: usize,
    pub(in crate::native_app) overscan_rows: usize,
    pub(in crate::native_app) guard_rows: usize,
}

pub(in crate::native_app) struct VisibleSampleList<'a> {
    pub(in crate::native_app) total_count: usize,
    pub(in crate::native_app) window: ui::VirtualListWindow,
    pub(in crate::native_app) rows: Vec<VisibleSampleRow<'a>>,
    pub(in crate::native_app) columns: Vec<&'a FileColumn>,
    pub(in crate::native_app) sort: &'a radiant::application::DetailsSort,
    pub(in crate::native_app) similarity_mode_active: bool,
    pub(in crate::native_app) similarity_controls: &'a SimilarityAspectSettings,
}

pub(super) struct VisibleSampleWindowFiles<'a> {
    pub(super) total_count: usize,
    pub(super) rows: Vec<&'a FileEntry>,
}

pub(in crate::native_app) struct VisibleSampleRow<'a> {
    pub(in crate::native_app) file: &'a FileEntry,
    pub(in crate::native_app) selected: bool,
    pub(in crate::native_app) focused: bool,
    pub(in crate::native_app) focus_alpha: u8,
    pub(in crate::native_app) selection_flash: bool,
    pub(in crate::native_app) copy_flash: bool,
    pub(in crate::native_app) protected_source_error_flash: bool,
    pub(in crate::native_app) drag_active: bool,
    pub(in crate::native_app) drag_source: bool,
    pub(in crate::native_app) cached: bool,
    pub(in crate::native_app) missing: bool,
    pub(in crate::native_app) rename: Option<FileRenameView>,
    pub(in crate::native_app) similarity_anchor: bool,
    pub(in crate::native_app) similarity_strength: Option<f32>,
    pub(in crate::native_app) similarity_aspect_strengths: SimilarityAspectStrengths,
    pub(in crate::native_app) collection_colors: Vec<ui::Rgba8>,
    pub(in crate::native_app) source_folder_path: String,
    pub(in crate::native_app) harvest_badges: Vec<String>,
    pub(in crate::native_app) harvest_completed: bool,
    pub(in crate::native_app) curation_badges: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct SampleListState {
    pub(super) file_columns: Vec<FileColumn>,
    pub(super) file_sort: radiant::application::DetailsSort,
    pub(super) file_column_resize: Option<radiant::application::DetailsColumnResizeDrag>,
    pub(super) file_column_reorder: Option<radiant::application::DetailsColumnReorderDrag>,
    pub(super) similarity_controls: SimilarityAspectSettings,
    pub(super) similarity: Option<SimilarityBrowserState>,
    pub(super) random_navigation: RandomNavigationState,
    pub(super) include_subfolders: bool,
    pub(super) view_controller: ui::VirtualListController,
    pub(super) follow_selection: ui::VirtualListFollowState<String>,
    pub(super) prepared_window: ui::VirtualListWindow,
    pub(super) prepared_content_revision: u64,
    pub(super) content_revision: u64,
    pub(super) missing_collection_files: Vec<FileEntry>,
    pub(super) missing_collection_counts: BTreeMap<u8, usize>,
    pub(super) projection_cache: VisibleSampleProjectionCache,
    pub(super) starmap_layout: StarmapLayoutCache,
    pub(super) listing_reveals: BrowserListingRevealState,
    pub(super) refollow_selected_after_content_change: bool,
    copy_flash_file_ids: HashSet<String>,
    copy_flash_frames: u8,
    protected_source_error_flash_file_ids: HashSet<String>,
    protected_source_error_flash_source_ids: HashSet<String>,
    protected_source_error_flash_frames: u8,
    primary_source_acceptance_flash_deadline: Option<Instant>,
}

impl SampleListState {
    pub(super) fn new() -> Self {
        Self {
            file_columns: default_file_columns(),
            file_sort: radiant::application::DetailsSort::new(
                "name",
                radiant::application::SortDirection::Ascending,
            ),
            file_column_resize: None,
            file_column_reorder: None,
            similarity_controls: SimilarityAspectSettings::default(),
            similarity: None,
            random_navigation: RandomNavigationState::default(),
            include_subfolders: false,
            view_controller: ui::VirtualListController::default(),
            follow_selection: ui::VirtualListFollowState::default(),
            prepared_window: ui::VirtualListWindow::default(),
            prepared_content_revision: 0,
            content_revision: 0,
            missing_collection_files: Vec::new(),
            missing_collection_counts: BTreeMap::new(),
            projection_cache: VisibleSampleProjectionCache::default(),
            starmap_layout: StarmapLayoutCache::default(),
            listing_reveals: BrowserListingRevealState::default(),
            refollow_selected_after_content_change: false,
            copy_flash_file_ids: HashSet::new(),
            copy_flash_frames: 0,
            protected_source_error_flash_file_ids: HashSet::new(),
            protected_source_error_flash_source_ids: HashSet::new(),
            protected_source_error_flash_frames: 0,
            primary_source_acceptance_flash_deadline: None,
        }
    }

    pub(super) fn reset_view(&mut self) {
        self.view_controller = ui::VirtualListController::default();
        self.follow_selection.clear();
        self.prepared_window = ui::VirtualListWindow::default();
        self.prepared_content_revision = self.content_revision;
        self.refollow_selected_after_content_change = false;
    }

    pub(super) fn bump_content_revision(&mut self) {
        self.content_revision = self.content_revision.saturating_add(1);
        self.projection_cache
            .invalidate_for_content_revision(self.content_revision);
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct RandomNavigationState {
    pub(super) enabled: bool,
    result_ids: Vec<String>,
    visited: HashSet<String>,
    history: Vec<String>,
}

impl RandomNavigationState {
    pub(super) fn set_enabled(
        &mut self,
        enabled: bool,
        selected_file: Option<&str>,
        ids: &[String],
    ) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        self.reset_for_selection(selected_file, ids);
    }

    pub(super) fn reconcile(&mut self, selected_file: Option<&str>, ids: &[String]) {
        if self.result_ids == ids {
            return;
        }
        self.reset_for_selection(selected_file, ids);
    }

    pub(super) fn previous(&mut self) -> Option<String> {
        if self.history.len() <= 1 {
            return None;
        }
        self.history.pop();
        self.history.last().cloned()
    }

    pub(super) fn next(&mut self, selected_file: Option<&str>, ids: &[String]) -> Option<String> {
        self.reconcile(selected_file, ids);
        if ids.len() <= 1 {
            return None;
        }
        if let Some(selected) = selected_file.filter(|selected| ids.iter().any(|id| id == selected))
        {
            self.record_selected(selected);
        }

        let target = self.random_unvisited(ids).or_else(|| {
            self.reset_cycle(selected_file, ids);
            self.random_unvisited(ids)
        })?;
        self.record_selected(&target);
        Some(target)
    }

    fn reset_for_selection(&mut self, selected_file: Option<&str>, ids: &[String]) {
        self.result_ids = ids.to_vec();
        self.visited.clear();
        self.history.clear();
        if let Some(selected) = selected_file.filter(|selected| ids.iter().any(|id| id == selected))
        {
            self.record_selected(selected);
        }
    }

    fn reset_cycle(&mut self, selected_file: Option<&str>, ids: &[String]) {
        self.visited.clear();
        self.history.clear();
        if let Some(selected) = selected_file.filter(|selected| ids.iter().any(|id| id == selected))
        {
            self.record_selected(selected);
        }
    }

    fn random_unvisited(&self, ids: &[String]) -> Option<String> {
        use rand::Rng;

        let candidates = ids
            .iter()
            .filter(|id| !self.visited.contains(*id))
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return None;
        }
        let index = rand::rng().random_range(0..candidates.len());
        Some(candidates[index].to_string())
    }

    fn record_selected(&mut self, selected: &str) {
        self.visited.insert(selected.to_owned());
        if self.history.last().is_none_or(|last| last != selected) {
            self.history.push(selected.to_owned());
        }
    }

    #[cfg(test)]
    pub(super) fn seed_for_tests(
        &mut self,
        result_ids: Vec<String>,
        visited: HashSet<String>,
        history: Vec<String>,
    ) {
        self.enabled = true;
        self.result_ids = result_ids;
        self.visited = visited;
        self.history = history;
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct VisibleSampleProjectionCache {
    entries: RefCell<HashMap<VisibleSampleProjectionKey, Vec<usize>>>,
    id_entries: RefCell<HashMap<VisibleSampleProjectionKey, Vec<String>>>,
}

impl VisibleSampleProjectionCache {
    pub(super) fn audio_indices(
        &self,
        request: VisibleSampleProjectionRequest<'_>,
        build: impl FnOnce() -> Vec<usize>,
    ) -> Ref<'_, Vec<usize>> {
        let key = request.key();
        if !self.entries.borrow().contains_key(&key) {
            let started_at = Instant::now();
            let value = build();
            log_projection_cache_fill("indices", &key, value.len(), started_at);
            self.entries.borrow_mut().insert(key.clone(), value);
        }
        Ref::map(self.entries.borrow(), |entries| {
            entries
                .get(&key)
                .expect("visible sample projection cache should contain computed key")
        })
    }

    pub(super) fn cached_audio_indices(
        &self,
        request: VisibleSampleProjectionRequest<'_>,
    ) -> Option<Ref<'_, Vec<usize>>> {
        let key = request.key();
        if !self.entries.borrow().contains_key(&key) {
            return None;
        }
        Some(Ref::map(self.entries.borrow(), |entries| {
            entries
                .get(&key)
                .expect("visible sample projection cache should contain computed key")
        }))
    }

    pub(super) fn audio_ids(
        &self,
        request: VisibleSampleProjectionRequest<'_>,
        build: impl FnOnce() -> Vec<String>,
    ) -> Ref<'_, Vec<String>> {
        let key = request.key();
        if !self.id_entries.borrow().contains_key(&key) {
            let started_at = Instant::now();
            let value = build();
            log_projection_cache_fill("ids", &key, value.len(), started_at);
            self.id_entries.borrow_mut().insert(key.clone(), value);
        }
        Ref::map(self.id_entries.borrow(), |entries| {
            entries
                .get(&key)
                .expect("visible sample id projection cache should contain computed key")
        })
    }

    pub(super) fn cached_audio_ids(
        &self,
        request: VisibleSampleProjectionRequest<'_>,
    ) -> Option<Ref<'_, Vec<String>>> {
        let key = request.key();
        if !self.id_entries.borrow().contains_key(&key) {
            return None;
        }
        Some(Ref::map(self.id_entries.borrow(), |entries| {
            entries
                .get(&key)
                .expect("visible sample id projection cache should contain computed key")
        }))
    }

    pub(super) fn invalidate_for_content_revision(&mut self, content_revision: u64) {
        self.entries
            .get_mut()
            .retain(|key, _| key.content_revision == content_revision);
        self.id_entries
            .get_mut()
            .retain(|key, _| key.content_revision == content_revision);
    }

    pub(super) fn clear(&self) {
        self.entries.borrow_mut().clear();
        self.id_entries.borrow_mut().clear();
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.borrow().len() + self.id_entries.borrow().len()
    }
}

fn log_projection_cache_fill(
    kind: &'static str,
    key: &VisibleSampleProjectionKey,
    rows: usize,
    started_at: Instant,
) {
    let elapsed = started_at.elapsed();
    if elapsed < SLOW_SAMPLE_PROJECTION_CACHE_FILL {
        return;
    }
    tracing::warn!(
        target: "wavecrate::debug::sample_load",
        event = "browser.sample_projection.cache_fill",
        kind,
        elapsed_ms = elapsed.as_secs_f64() * 1_000.0,
        rows,
        folder_id = key.folder_id.as_str(),
        name_filter_active = !key.name_filter.is_empty(),
        rating_filter_active = !key.rating_filter.is_empty(),
        curation_filter_active = !key.curation_filter.is_empty(),
        listing_reveal_active = key.listing_reveal_id.is_some(),
        sort_column = key.sort_column_id.as_str(),
        sort_descending = key.sort_descending,
        similarity_active = key.similarity_anchor_id.is_some(),
        playback_type_tag_sort = key.playback_type_tag_sort,
        content_revision = key.content_revision,
        "Visible sample projection cache fill was slow"
    );
}

pub(super) struct VisibleSampleProjectionRequest<'a> {
    folder_id: &'a str,
    name_filter: &'a str,
    rating_filter: &'a str,
    curation_filter: &'a str,
    listing_reveal_id: Option<&'a str>,
    sort: &'a radiant::application::DetailsSort,
    similarity_anchor_id: Option<&'a str>,
    content_revision: u64,
    playback_type_tag_sort: bool,
}

impl<'a> VisibleSampleProjectionRequest<'a> {
    pub(super) fn new(
        folder_id: &'a str,
        name_filter: &'a str,
        rating_filter: &'a str,
        curation_filter: &'a str,
        sort: &'a radiant::application::DetailsSort,
        similarity_anchor_id: Option<&'a str>,
        content_revision: u64,
    ) -> Self {
        Self {
            folder_id,
            name_filter,
            rating_filter,
            curation_filter,
            listing_reveal_id: None,
            sort,
            similarity_anchor_id,
            content_revision,
            playback_type_tag_sort: false,
        }
    }

    pub(super) fn with_playback_type_tag_sort(mut self, enabled: bool) -> Self {
        self.playback_type_tag_sort = enabled;
        self
    }

    pub(super) fn with_listing_reveal(mut self, file_id: Option<&'a str>) -> Self {
        self.listing_reveal_id = file_id;
        self
    }

    fn key(&self) -> VisibleSampleProjectionKey {
        VisibleSampleProjectionKey::new(
            self.folder_id.to_owned(),
            self.name_filter.to_owned(),
            self.rating_filter.to_owned(),
            self.curation_filter.to_owned(),
            self.listing_reveal_id.map(str::to_owned),
            self.sort.column_id.clone(),
            self.sort.direction == radiant::application::SortDirection::Descending,
            self.similarity_anchor_id.map(str::to_owned),
            self.content_revision,
            self.playback_type_tag_sort,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct VisibleSampleProjectionKey {
    folder_id: String,
    name_filter: String,
    rating_filter: String,
    curation_filter: String,
    listing_reveal_id: Option<String>,
    sort_column_id: String,
    sort_descending: bool,
    similarity_anchor_id: Option<String>,
    content_revision: u64,
    playback_type_tag_sort: bool,
}

impl VisibleSampleProjectionKey {
    fn new(
        folder_id: String,
        name_filter: String,
        rating_filter: String,
        curation_filter: String,
        listing_reveal_id: Option<String>,
        sort_column_id: String,
        sort_descending: bool,
        similarity_anchor_id: Option<String>,
        content_revision: u64,
        playback_type_tag_sort: bool,
    ) -> Self {
        Self {
            folder_id,
            name_filter,
            rating_filter,
            curation_filter,
            listing_reveal_id,
            sort_column_id,
            sort_descending,
            similarity_anchor_id,
            content_revision,
            playback_type_tag_sort,
        }
    }
}

impl Hash for VisibleSampleProjectionKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.folder_id.hash(state);
        self.name_filter.hash(state);
        self.rating_filter.hash(state);
        self.curation_filter.hash(state);
        self.listing_reveal_id.hash(state);
        self.sort_column_id.hash(state);
        self.sort_descending.hash(state);
        self.similarity_anchor_id.hash(state);
        self.content_revision.hash(state);
        self.playback_type_tag_sort.hash(state);
    }
}

impl FolderBrowserState {
    pub(in crate::native_app) fn flash_copied_file_paths<I, P>(&mut self, paths: I)
    where
        I: IntoIterator<Item = P>,
        P: AsRef<std::path::Path>,
    {
        self.sample_list.copy_flash_file_ids.clear();
        self.sample_list
            .copy_flash_file_ids
            .extend(paths.into_iter().filter_map(copy_flash_file_id));
        self.sample_list.copy_flash_frames = if self.sample_list.copy_flash_file_ids.is_empty() {
            0
        } else {
            COPY_FLASH_FRAMES
        };
    }

    pub(in crate::native_app) fn copy_flash_active(&self) -> bool {
        self.sample_list.copy_flash_frames > 0
    }

    pub(in crate::native_app) fn copy_flash_frames(&self) -> u8 {
        self.sample_list.copy_flash_frames
    }

    pub(in crate::native_app) fn flash_primary_source_acceptance(&mut self) {
        self.flash_primary_source_acceptance_at(Instant::now());
    }

    pub(in crate::native_app) fn primary_source_acceptance_flash_active(&self) -> bool {
        self.primary_source_acceptance_flash_active_at(Instant::now())
    }

    pub(in crate::native_app) fn advance_primary_source_acceptance_flash_frame(&mut self) {
        self.advance_primary_source_acceptance_flash_at(Instant::now());
    }

    fn flash_primary_source_acceptance_at(&mut self, now: Instant) {
        self.sample_list.primary_source_acceptance_flash_deadline =
            Some(now + PRIMARY_SOURCE_ACCEPTANCE_FLASH_DURATION);
    }

    fn primary_source_acceptance_flash_active_at(&self, now: Instant) -> bool {
        self.sample_list
            .primary_source_acceptance_flash_deadline
            .is_some_and(|deadline| now < deadline)
    }

    fn advance_primary_source_acceptance_flash_at(&mut self, now: Instant) {
        if !self.primary_source_acceptance_flash_active_at(now) {
            self.sample_list.primary_source_acceptance_flash_deadline = None;
        }
    }

    #[cfg(test)]
    pub(in crate::native_app) fn set_primary_source_acceptance_flash_time_for_tests(
        &mut self,
        now: Instant,
    ) {
        self.flash_primary_source_acceptance_at(now);
    }

    #[cfg(test)]
    pub(in crate::native_app) fn advance_primary_source_acceptance_flash_time_for_tests(
        &mut self,
        now: Instant,
    ) {
        self.advance_primary_source_acceptance_flash_at(now);
    }

    #[cfg(test)]
    pub(in crate::native_app) fn primary_source_acceptance_flash_active_at_for_tests(
        &self,
        now: Instant,
    ) -> bool {
        self.primary_source_acceptance_flash_active_at(now)
    }

    pub(in crate::native_app) fn advance_copy_flash_frame(&mut self) {
        if self.sample_list.copy_flash_frames == 0 {
            return;
        }
        self.sample_list.copy_flash_frames = self.sample_list.copy_flash_frames.saturating_sub(1);
        if self.sample_list.copy_flash_frames == 0 {
            self.sample_list.copy_flash_file_ids.clear();
        }
    }

    pub(in crate::native_app) fn flash_protected_source_error_paths<I, P>(&mut self, paths: I)
    where
        I: IntoIterator<Item = P>,
        P: AsRef<std::path::Path>,
    {
        let mut file_ids = HashSet::new();
        let mut source_ids = HashSet::new();
        for path in paths {
            let path = path.as_ref();
            if let Some(file_id) = copy_flash_file_id(path) {
                file_ids.insert(file_id);
            }
            if let Some(source_id) = self.protected_source_id_for_path(path) {
                source_ids.insert(source_id);
            }
        }
        self.sample_list.protected_source_error_flash_file_ids = file_ids;
        self.sample_list.protected_source_error_flash_source_ids = source_ids;
        self.sample_list.protected_source_error_flash_frames = if self
            .sample_list
            .protected_source_error_flash_file_ids
            .is_empty()
            && self
                .sample_list
                .protected_source_error_flash_source_ids
                .is_empty()
        {
            0
        } else {
            PROTECTED_SOURCE_ERROR_FLASH_FRAMES
        };
    }

    pub(in crate::native_app) fn protected_source_error_flash_frames(&self) -> u8 {
        self.sample_list.protected_source_error_flash_frames
    }

    pub(in crate::native_app) fn advance_protected_source_error_flash_frame(&mut self) {
        if self.sample_list.protected_source_error_flash_frames == 0 {
            return;
        }
        self.sample_list.protected_source_error_flash_frames = self
            .sample_list
            .protected_source_error_flash_frames
            .saturating_sub(1);
        if self.sample_list.protected_source_error_flash_frames == 0 {
            self.sample_list
                .protected_source_error_flash_file_ids
                .clear();
            self.sample_list
                .protected_source_error_flash_source_ids
                .clear();
        }
    }

    pub(in crate::native_app) fn source_protected_error_flash_active(
        &self,
        source_id: &str,
    ) -> bool {
        self.sample_list.protected_source_error_flash_frames > 0
            && self
                .sample_list
                .protected_source_error_flash_source_ids
                .contains(source_id)
    }

    pub(in crate::native_app) fn prepare_visible_sample_window(
        &mut self,
        policy: VisibleSampleWindowPolicy<'_>,
    ) -> ui::VirtualListWindow {
        let window = self.follow_selected_file_view_matching_tags(
            policy.viewport_rows,
            policy.overscan_rows,
            policy.guard_rows,
            policy.tags_by_file,
        );
        self.sample_list.prepared_content_revision = self.sample_list.content_revision;
        window
    }

    pub(in crate::native_app) fn visible_samples<'a>(
        &'a self,
        query: VisibleSampleQuery<'a>,
    ) -> VisibleSampleList<'a> {
        let (window, window_files) = self.visible_sample_window_files(query.tags_by_file);
        let harvest_lookup =
            super::harvest_filter::HarvestFileFactsLookup::load(self, &window_files.rows);
        let show_new_harvest_badges = self.filters.harvest.is_some();
        let rows = window_files
            .rows
            .into_iter()
            .map(|file| {
                self.visible_sample_row_for_file(
                    file,
                    query,
                    &harvest_lookup,
                    show_new_harvest_badges,
                )
            })
            .collect();

        VisibleSampleList {
            total_count: window_files.total_count,
            window,
            rows,
            columns: self.visible_file_columns(),
            sort: self.file_sort(),
            similarity_mode_active: self.similarity_mode_active(),
            similarity_controls: self.similarity_controls(),
        }
    }

    pub(in crate::native_app) fn prepared_visible_sample_file_ids_matching_tags(
        &self,
        tags_by_file: &HashMap<String, Vec<String>>,
        max_window_len: usize,
    ) -> Option<Vec<String>> {
        if self.sample_list.prepared_content_revision != self.sample_list.content_revision {
            return None;
        }
        let window = preview_warm_window(self.sample_list.prepared_window, max_window_len);
        let window_files =
            self.selected_audio_file_window_matching_tags_if_cached(window, tags_by_file)?;
        window_files_complete(window, &window_files).then(|| {
            window_files
                .rows
                .into_iter()
                .filter(|file| !file.is_missing())
                .map(|file| file.id.clone())
                .collect()
        })
    }

    fn visible_sample_window_files(
        &self,
        tags_by_file: &HashMap<String, Vec<String>>,
    ) -> (ui::VirtualListWindow, VisibleSampleWindowFiles<'_>) {
        let prepared_window = self.sample_list.prepared_window;
        let mut window_files =
            self.selected_audio_file_window_matching_tags(prepared_window, tags_by_file);
        let mut window = prepared_window.reconcile_total_items(window_files.total_count);
        if window != prepared_window {
            window_files = self.selected_audio_file_window_matching_tags(window, tags_by_file);
        }
        if !window_files_complete(window, &window_files) {
            self.sample_list.projection_cache.clear();
            window_files = self.selected_audio_file_window_matching_tags(window, tags_by_file);
        }
        if !window_files_complete(window, &window_files) {
            window_files =
                self.uncached_selected_audio_file_window_matching_tags(window, tags_by_file);
            let repaired_window = window.reconcile_total_items(window_files.total_count);
            if repaired_window != window {
                window = repaired_window;
                window_files =
                    self.uncached_selected_audio_file_window_matching_tags(window, tags_by_file);
            }
        }
        (window, window_files)
    }

    fn visible_sample_row_for_file<'a>(
        &'a self,
        file: &'a FileEntry,
        query: VisibleSampleQuery<'a>,
        harvest_lookup: &super::harvest_filter::HarvestFileFactsLookup,
        show_new_harvest_badges: bool,
    ) -> VisibleSampleRow<'a> {
        let harvest_facts = harvest_lookup.facts_for_file(self, file);
        let selected = self.is_file_selected(&file.id);
        let focused = self.pointer_focused_folder_id.is_none()
            && !self.source_keyboard_focus_active()
            && self.keyboard_focus_visible()
            && self.selected_file_id() == Some(file.id.as_str());
        VisibleSampleRow {
            file,
            selected,
            focused,
            focus_alpha: if focused {
                self.keyboard_focus_alpha()
            } else {
                0
            },
            selection_flash: self.marked_item_flash_active(&file.id),
            copy_flash: self.copied_file_flash_active(&file.id),
            protected_source_error_flash: self.protected_source_error_file_flash_active(&file.id),
            drag_active: self.file_drag_active(),
            drag_source: self.file_drag_source(&file.id),
            cached: query.cached_sample_paths.contains(&file.id),
            missing: file.is_missing(),
            rename: self.file_rename_view(&file.id),
            similarity_anchor: self.file_is_similarity_anchor(&file.id),
            similarity_strength: self.similarity_display_strength_for_file(&file.id),
            similarity_aspect_strengths: self
                .similarity_aspect_display_strengths_for_file(&file.id),
            collection_colors: file
                .collection_memberships()
                .into_iter()
                .filter_map(|collection| self.collection_color(collection))
                .collect(),
            source_folder_path: self.visible_source_folder_path_for_file(file),
            harvest_badges: super::harvest_filter::harvest_badges_for_facts(
                harvest_facts,
                show_new_harvest_badges,
            ),
            harvest_completed: harvest_facts.is_some_and(|facts| {
                matches!(facts.state, HarvestState::Done | HarvestState::Ignored)
            }),
            curation_badges: super::curation::curation_badges_for_file(
                file,
                query.tags_by_file.get(&file.id).map(Vec::as_slice),
                &self.filters.curation,
                super::curation::now_epoch_seconds(),
            ),
        }
    }

    fn visible_source_folder_path_for_file(&self, file: &FileEntry) -> String {
        if self.collection_focus_active() {
            self.source_folder_path_for_file(file)
        } else {
            String::new()
        }
    }

    fn source_folder_path_for_file(&self, file: &FileEntry) -> String {
        let file_path = Path::new(&file.id);
        let folder_path = self
            .source_relative_file_path(file_path)
            .and_then(|(_, relative)| relative.parent().map(Path::to_path_buf))
            .or_else(|| file_path.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        folder_path_display(folder_path)
    }

    pub(super) fn copied_file_flash_active(&self, file_id: &str) -> bool {
        self.copy_flash_active() && self.sample_list.copy_flash_file_ids.contains(file_id)
    }

    pub(super) fn protected_source_error_file_flash_active(&self, file_id: &str) -> bool {
        self.sample_list.protected_source_error_flash_frames > 0
            && self
                .sample_list
                .protected_source_error_flash_file_ids
                .contains(file_id)
    }

    fn protected_source_id_for_path(&self, path: &std::path::Path) -> Option<String> {
        self.source
            .sources
            .iter()
            .filter(|source| path.starts_with(&source.root))
            .max_by_key(|source| source.root.components().count())
            .filter(|source| source.is_protected())
            .map(|source| source.id.clone())
    }
}

fn folder_path_display(path: PathBuf) -> String {
    if path.as_os_str().is_empty() {
        String::from(".")
    } else {
        path.to_string_lossy().into_owned()
    }
}

fn copy_flash_file_id(path: impl AsRef<std::path::Path>) -> Option<String> {
    path.as_ref()
        .to_str()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

fn preview_warm_window(
    window: ui::VirtualListWindow,
    max_window_len: usize,
) -> ui::VirtualListWindow {
    if max_window_len == 0 || window.window_len() <= max_window_len {
        return window;
    }

    let viewport_start = window
        .viewport_start
        .clamp(window.window_start, window.window_end);
    let viewport_end = window.viewport_end.clamp(viewport_start, window.window_end);
    let viewport_len = viewport_end.saturating_sub(viewport_start);
    let center = if viewport_len == 0 {
        window.window_start
    } else {
        viewport_start.saturating_add(viewport_len / 2)
    };

    let half = max_window_len / 2;
    let mut window_start = center
        .saturating_sub(half)
        .clamp(window.window_start, window.window_end);
    let window_end = window_start
        .saturating_add(max_window_len)
        .min(window.window_end);
    if window_end.saturating_sub(window_start) < max_window_len {
        window_start = window_end
            .saturating_sub(max_window_len)
            .max(window.window_start);
    }

    let viewport_start = viewport_start.clamp(window_start, window_end);
    let viewport_end = viewport_end.clamp(viewport_start, window_end);
    ui::VirtualListWindow {
        total_items: window.total_items,
        viewport_start,
        viewport_end,
        window_start,
        window_end,
    }
}

fn window_files_complete(
    window: ui::VirtualListWindow,
    window_files: &VisibleSampleWindowFiles<'_>,
) -> bool {
    window_files.rows.len() == window.window_len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_warm_window_limits_large_materialized_ranges_around_viewport() {
        let window = ui::VirtualListWindow {
            total_items: 1_000,
            viewport_start: 500,
            viewport_end: 520,
            window_start: 0,
            window_end: 1_000,
        };

        let limited = preview_warm_window(window, 48);

        assert_eq!(limited.window_len(), 48);
        assert!(limited.window_start > 0);
        assert!(limited.window_end < 1_000);
        assert!(limited.contains(500));
        assert!(limited.contains(519));
    }

    #[test]
    fn preview_warm_window_keeps_small_materialized_ranges_unchanged() {
        let window = ui::VirtualListWindow {
            total_items: 32,
            viewport_start: 0,
            viewport_end: 16,
            window_start: 0,
            window_end: 32,
        };

        assert_eq!(preview_warm_window(window, 48), window);
    }

    #[test]
    fn projection_request_key_tracks_query_and_revision_inputs() {
        let ascending = radiant::application::DetailsSort::new(
            "name",
            radiant::application::SortDirection::Ascending,
        );
        let descending = radiant::application::DetailsSort::new(
            "size",
            radiant::application::SortDirection::Descending,
        );

        assert_ne!(
            VisibleSampleProjectionRequest::new("folder", "kick", "", "", &ascending, None, 4)
                .key(),
            VisibleSampleProjectionRequest::new("folder", "kick", "", "", &descending, None, 4)
                .key()
        );
        assert_ne!(
            VisibleSampleProjectionRequest::new("folder", "kick", "", "", &ascending, None, 4)
                .key(),
            VisibleSampleProjectionRequest::new("folder", "snare", "", "", &ascending, None, 4)
                .key()
        );
        assert_ne!(
            VisibleSampleProjectionRequest::new(
                "folder",
                "kick",
                "",
                "",
                &ascending,
                Some("a.wav"),
                4
            )
            .key(),
            VisibleSampleProjectionRequest::new(
                "folder",
                "kick",
                "",
                "",
                &ascending,
                Some("b.wav"),
                4
            )
            .key()
        );
        assert_ne!(
            VisibleSampleProjectionRequest::new("folder", "kick", "", "", &ascending, None, 4)
                .key(),
            VisibleSampleProjectionRequest::new("folder", "kick", "", "", &ascending, None, 5)
                .key()
        );
        assert_ne!(
            VisibleSampleProjectionRequest::new("folder", "kick", "-1,2", "", &ascending, None, 4)
                .key(),
            VisibleSampleProjectionRequest::new("folder", "kick", "2", "", &ascending, None, 4)
                .key()
        );
        assert_ne!(
            VisibleSampleProjectionRequest::new(
                "folder",
                "kick",
                "",
                "curation:all:14:0:0",
                &ascending,
                None,
                4
            )
            .key(),
            VisibleSampleProjectionRequest::new("folder", "kick", "", "", &ascending, None, 4)
                .key()
        );
        assert_ne!(
            VisibleSampleProjectionRequest::new("folder", "kick", "", "", &ascending, None, 4)
                .key(),
            VisibleSampleProjectionRequest::new("folder", "kick", "", "", &ascending, None, 4)
                .with_playback_type_tag_sort(true)
                .key()
        );
    }

    #[test]
    fn stale_prepared_window_is_clamped_to_current_total() {
        let stale = ui::VirtualListWindow {
            total_items: 10_000,
            viewport_start: 9_990,
            viewport_end: 10_000,
            window_start: 9_986,
            window_end: 10_000,
        };

        let reconciled = stale.reconcile_total_items(24);

        assert_eq!(reconciled.total_items, 24);
        assert_eq!(reconciled.viewport_start, 14);
        assert_eq!(reconciled.viewport_end, 24);
        assert_eq!(reconciled.window_start, 10);
        assert_eq!(reconciled.window_end, 24);
    }
}
