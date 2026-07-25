use std::{
    cell::Ref,
    collections::{BTreeSet, HashMap},
    path::PathBuf,
};

use super::{
    FileColumnKind, FileEntry, FolderBrowserState, FolderEntry, curation,
    file_columns::sort_kind_for_details_sort,
    harvest_filter,
    listing::{BrowserListingRevealReason, BrowserListingSnapshot},
    path_helpers::path_id_matches,
    playback_type_filter, rating_filter,
    visible_samples::{VisibleSampleProjectionRequest, VisibleSampleWindowFiles},
};

mod adjacent;
mod collection;
mod filters;
mod ordering;
mod subtree;
pub(super) mod traversal;

impl FolderBrowserState {
    pub(super) fn active_name_filter(&self) -> &str {
        if self.filters.name_enabled {
            self.filters.name_filter.as_str()
        } else {
            ""
        }
    }

    pub(super) fn active_required_tags(&self) -> Vec<String> {
        if self.filters.tags_enabled {
            filters::parsed_tag_filter(&self.filters.tag_filter)
        } else {
            Vec::new()
        }
    }

    pub(super) fn active_playback_type_filters(
        &self,
    ) -> BTreeSet<playback_type_filter::PlaybackTypeFilter> {
        if self.filters.playback_type_enabled {
            self.filters.playback_type_filter.clone()
        } else {
            BTreeSet::new()
        }
    }

    pub(super) fn active_rating_filter(&self) -> BTreeSet<i8> {
        if self.filters.rating_enabled {
            self.filters.rating_filter.clone()
        } else {
            BTreeSet::new()
        }
    }

    pub(super) fn active_harvest_filter(&self) -> Option<harvest_filter::HarvestFilter> {
        self.filters
            .harvest_enabled
            .then_some(self.selected_harvest_filter())
    }

    pub(in crate::native_app) fn harvest_context_menu_actions_active(&self) -> bool {
        self.harvest_mode_active()
    }

    /// Returns true when the Harvest filter family has a selected active filter.
    pub(in crate::native_app) fn harvest_mode_active(&self) -> bool {
        self.active_harvest_filter().is_some()
    }

    pub(in crate::native_app) fn selected_files(&self) -> &[FileEntry] {
        self.selected_folder()
            .map(|folder| folder.files.as_slice())
            .unwrap_or(&[])
    }

    pub(in crate::native_app) fn context_file_is_keep_locked(
        &self,
        path: &std::path::Path,
    ) -> bool {
        self.loaded_source_audio_files()
            .into_iter()
            .find(|file| path_id_matches(&file.id, path))
            .is_some_and(|file| {
                file.rating == wavecrate::sample_sources::Rating::KEEP_3 && file.rating_locked
            })
    }

    pub(in crate::native_app) fn selected_audio_files(&self) -> Vec<&FileEntry> {
        self.selected_audio_files_with_sort_tags(None)
    }

    fn selected_audio_files_with_sort_tags(
        &self,
        sort_tags: Option<&HashMap<String, Vec<String>>>,
    ) -> Vec<&FileEntry> {
        if self.active_harvest_filter().is_some() {
            let empty_tags = HashMap::new();
            let tags_by_file = sort_tags.unwrap_or(&empty_tags);
            return self.browser_listing_snapshot(tags_by_file).rows().to_vec();
        }
        if let Some(collection) = self.selection.selected_collection {
            return self.selected_collection_audio_files_with_sort_tags(collection, sort_tags);
        }

        let Some(folder) = self.selected_folder() else {
            return Vec::new();
        };
        if self.folder_subtree_listing_enabled() {
            return self.selected_folder_recursive_audio_files_with_sort_tags(folder, sort_tags);
        }
        self.selected_folder_audio_files_with_sort_tags(folder, sort_tags)
    }

    pub(in crate::native_app) fn selected_cache_candidate_paths(
        &self,
        max_files: usize,
    ) -> Vec<PathBuf> {
        if max_files == 0 {
            return Vec::new();
        }

        let name_query = filters::normalized_name_filter(self.active_name_filter());
        if let Some(collection) = self.selection.selected_collection {
            let mut paths = Vec::new();
            for folder in self.loaded_source_root_folders() {
                collect_collection_cache_candidate_paths(
                    folder,
                    collection,
                    &name_query,
                    max_files,
                    &mut paths,
                );
                if paths.len() >= max_files {
                    break;
                }
            }
            return paths;
        }

        let Some(folder) = self.selected_folder() else {
            return Vec::new();
        };
        if self.folder_subtree_listing_enabled() {
            return subtree::collect_recursive_cache_candidate_paths(
                folder,
                &name_query,
                max_files,
            );
        }
        collect_local_cache_candidate_paths(folder, &name_query, max_files)
    }

    pub(in crate::native_app) fn selected_source_cache_warm_request(
        &self,
    ) -> Option<(String, Vec<PathBuf>)> {
        self.source_cache_warm_request(self.source.selected_source.as_str())
    }

    pub(in crate::native_app) fn source_cache_warm_request(
        &self,
        source_id: &str,
    ) -> Option<(String, Vec<PathBuf>)> {
        let folder = self.source_root_folder(source_id)?;
        let mut paths = Vec::new();
        collect_source_cache_candidate_paths(folder, &mut paths);
        Some((folder.id.clone(), paths))
    }

    pub(in crate::native_app) fn selected_audio_files_matching_tags(
        &self,
        tags_by_file: &HashMap<String, Vec<String>>,
    ) -> Vec<&FileEntry> {
        self.browser_listing_snapshot(tags_by_file).rows().to_vec()
    }

    pub(in crate::native_app) fn browser_listing_snapshot<'a>(
        &'a self,
        tags_by_file: &HashMap<String, Vec<String>>,
    ) -> BrowserListingSnapshot<'a> {
        let reveal_id = self.active_listing_reveal_id(Some(tags_by_file));
        let name_query = filters::normalized_name_filter(self.active_name_filter());
        let required_tags = self.active_required_tags();
        let playback_type_filters = self.active_playback_type_filters();
        let rating_filter = self.active_rating_filter();
        let curation_now = curation::now_epoch_seconds();
        let mut files = self.scoped_audio_files_for_listing();
        files.retain(|file| {
            if self.scan_file_deferred(&file.id) {
                return false;
            }
            if reveal_id == Some(file.id.as_str()) {
                return true;
            }
            filters::audio_file_matches_name_query(file, &name_query)
                && filters::audio_file_matches_parsed_tags(file, tags_by_file, &required_tags)
                && playback_type_filter::playback_type_filter_matches(
                    file,
                    tags_by_file,
                    &playback_type_filters,
                )
                && rating_filter::rating_filter_matches(file, &rating_filter)
                && curation_filter_allows_file(
                    file,
                    Some(tags_by_file),
                    &self.filters.curation,
                    curation_now,
                    None,
                )
        });
        self.retain_harvest_filter_matches(&mut files, reveal_id);
        self.sort_files_matching_tags(&mut files, tags_by_file);
        BrowserListingSnapshot::new(files)
    }

    fn window_from_browser_listing_snapshot(
        &self,
        window: radiant::prelude::VirtualListWindow,
        tags_by_file: &HashMap<String, Vec<String>>,
    ) -> VisibleSampleWindowFiles<'_> {
        let snapshot = self.browser_listing_snapshot(tags_by_file);
        let total_count = snapshot.len();
        VisibleSampleWindowFiles {
            total_count,
            rows: (window.window_start.min(total_count)..window.window_end.min(total_count))
                .filter_map(|index| snapshot.rows().get(index).copied())
                .collect(),
        }
    }

    fn scoped_audio_files_for_listing(&self) -> Vec<&FileEntry> {
        if let Some(collection) = self.selection.selected_collection {
            self.scoped_collection_audio_files_for_listing(collection)
        } else if let Some(folder) = self.selected_folder() {
            if self.folder_subtree_listing_enabled() {
                scoped_recursive_audio_files(folder)
            } else {
                folder.files.iter().filter(|file| file.is_audio()).collect()
            }
        } else {
            Vec::new()
        }
    }

    fn scoped_collection_audio_files_for_listing(
        &self,
        collection: wavecrate::sample_sources::SampleCollection,
    ) -> Vec<&FileEntry> {
        let mut files = Vec::new();
        for folder in self.loaded_source_root_folders() {
            traversal::collect_collection_audio_files(folder, collection, &mut files);
        }
        files.extend(
            self.sample_list
                .missing_collection_files
                .iter()
                .filter(|file| file.belongs_to_collection(collection)),
        );
        files
    }

    pub(in crate::native_app) fn clear_listing_reveals(&mut self) -> bool {
        let cleared = self.sample_list.listing_reveals.clear();
        if cleared {
            self.sample_list.projection_cache.clear();
        }
        cleared
    }

    pub(in crate::native_app) fn clear_curation_focus_override(&mut self) -> bool {
        self.clear_listing_reveals()
    }

    pub(in crate::native_app) fn reveal_selected_file_if_hidden(
        &mut self,
        tags_by_file: &HashMap<String, Vec<String>>,
        reason: BrowserListingRevealReason,
    ) -> bool {
        let Some(selected) = self.selection.selected_file.clone() else {
            self.clear_listing_reveals();
            return false;
        };

        self.clear_listing_reveals();
        if self
            .browser_listing_snapshot(tags_by_file)
            .contains(&selected)
        {
            return false;
        }

        self.sample_list
            .listing_reveals
            .set(selected.clone(), reason);
        self.sample_list.projection_cache.clear();
        if self
            .browser_listing_snapshot(tags_by_file)
            .contains(&selected)
        {
            return true;
        }

        self.clear_listing_reveals();
        false
    }

    #[cfg(test)]
    pub(in crate::native_app) fn reveal_selected_curation_focus_if_hidden(
        &mut self,
        tags_by_file: &HashMap<String, Vec<String>>,
    ) -> bool {
        self.reveal_selected_file_if_hidden(
            tags_by_file,
            BrowserListingRevealReason::LoadedFileFocus,
        )
    }

    pub(in crate::native_app) fn selected_audio_file_count_matching_tags(
        &self,
        tags_by_file: &HashMap<String, Vec<String>>,
    ) -> usize {
        if self.active_listing_reveal_id(Some(tags_by_file)).is_some() {
            return self.browser_listing_snapshot(tags_by_file).len();
        }
        let name_query = filters::normalized_name_filter(self.active_name_filter());
        let required_tags = self.active_required_tags();
        let playback_type_filter = self.active_playback_type_filters();
        let rating_filter = self.active_rating_filter();
        let harvest_active = self.active_harvest_filter().is_some();
        if required_tags.is_empty()
            && playback_type_filter.is_empty()
            && self.selection.selected_collection.is_none()
            && !self.filters.curation.enabled
            && !harvest_active
        {
            return self.selected_folder_audio_file_count();
        }
        if let Some(collection) = self.selection.selected_collection {
            if required_tags.is_empty()
                && playback_type_filter.is_empty()
                && !self.filters.curation.enabled
                && !harvest_active
            {
                return self
                    .selected_collection_audio_file_ids_ref(collection)
                    .len();
            }
            return self.selected_audio_files_matching_tags(tags_by_file).len();
        }
        if self.filters.curation.enabled || harvest_active {
            return self.selected_audio_files_matching_tags(tags_by_file).len();
        }
        if self.folder_subtree_listing_enabled() {
            return self
                .selected_folder()
                .map(|folder| {
                    traversal::count_matching_audio_files_in_folder(
                        folder,
                        &name_query,
                        &required_tags,
                        tags_by_file,
                        &rating_filter,
                        &playback_type_filter,
                        None,
                    )
                })
                .unwrap_or_default();
        }

        self.selected_files()
            .iter()
            .filter(|file| {
                file.is_audio()
                    && filters::audio_file_matches_name_query(file, &name_query)
                    && filters::audio_file_matches_parsed_tags(file, tags_by_file, &required_tags)
                    && playback_type_filter::playback_type_filter_matches(
                        file,
                        tags_by_file,
                        &playback_type_filter,
                    )
                    && rating_filter::rating_filter_matches(file, &rating_filter)
            })
            .count()
    }

    pub(in crate::native_app) fn selected_folder_audio_file_count(&self) -> usize {
        if self.selection.selected_collection.is_some() {
            return self.selected_audio_files().len();
        }
        let Some(folder) = self.selected_folder() else {
            return 0;
        };
        if self.folder_subtree_listing_enabled() {
            return self
                .selected_folder_recursive_audio_file_ids_ref(folder)
                .len();
        }
        self.selected_folder_audio_file_indices_ref(folder).len()
    }

    pub(super) fn selected_audio_file_window_matching_tags(
        &self,
        window: radiant::prelude::VirtualListWindow,
        tags_by_file: &HashMap<String, Vec<String>>,
    ) -> VisibleSampleWindowFiles<'_> {
        if self.active_listing_reveal_id(Some(tags_by_file)).is_some() {
            return self.window_from_browser_listing_snapshot(window, tags_by_file);
        }
        if self.active_harvest_filter().is_some() {
            return self.window_from_browser_listing_snapshot(window, tags_by_file);
        }
        if let Some(collection) = self.selection.selected_collection {
            return self.selected_collection_audio_file_window_matching_tags(
                collection,
                window,
                tags_by_file,
            );
        }

        let Some(folder) = self.selected_folder() else {
            return VisibleSampleWindowFiles {
                total_count: 0,
                rows: Vec::new(),
            };
        };
        if self.folder_subtree_listing_enabled() {
            return self.selected_folder_recursive_audio_file_window_matching_tags(
                folder,
                window,
                tags_by_file,
            );
        }

        let required_tags = self.active_required_tags();
        let playback_type_filter = self.active_playback_type_filters();
        let indices =
            self.selected_folder_audio_file_indices_ref_with_sort_tags(folder, Some(tags_by_file));
        if required_tags.is_empty()
            && playback_type_filter.is_empty()
            && !self.filters.curation.enabled
        {
            let total_count = indices.len();
            return VisibleSampleWindowFiles {
                total_count,
                rows: (window.window_start.min(total_count)..window.window_end.min(total_count))
                    .filter_map(|index| {
                        indices
                            .get(index)
                            .and_then(|file_index| folder.files.get(*file_index))
                    })
                    .collect(),
            };
        }

        let mut total_count = 0;
        let rows = indices
            .iter()
            .filter_map(|file_index| folder.files.get(*file_index))
            .filter(|file| {
                filters::audio_file_matches_parsed_tags(file, tags_by_file, &required_tags)
                    && playback_type_filter::playback_type_filter_matches(
                        file,
                        tags_by_file,
                        &playback_type_filter,
                    )
            })
            .filter_map(|file| {
                let row = (total_count >= window.window_start && total_count < window.window_end)
                    .then_some(file);
                total_count += 1;
                row
            })
            .collect();

        VisibleSampleWindowFiles { total_count, rows }
    }

    pub(super) fn selected_audio_file_window_matching_tags_if_cached(
        &self,
        window: radiant::prelude::VirtualListWindow,
        tags_by_file: &HashMap<String, Vec<String>>,
    ) -> Option<VisibleSampleWindowFiles<'_>> {
        if self.active_listing_reveal_id(Some(tags_by_file)).is_some()
            || self.active_harvest_filter().is_some()
        {
            return None;
        }
        if let Some(collection) = self.selection.selected_collection {
            return self.selected_collection_audio_file_window_matching_tags_if_cached(
                collection,
                window,
                tags_by_file,
            );
        }

        let folder = self.selected_folder()?;
        if self.folder_subtree_listing_enabled() {
            return self.selected_folder_recursive_audio_file_window_matching_tags_if_cached(
                folder,
                window,
                tags_by_file,
            );
        }

        let required_tags = self.active_required_tags();
        let playback_type_filter = self.active_playback_type_filters();
        let indices = self.selected_folder_audio_file_indices_ref_with_sort_tags_if_cached(
            folder,
            Some(tags_by_file),
        )?;
        if required_tags.is_empty()
            && playback_type_filter.is_empty()
            && !self.filters.curation.enabled
        {
            let total_count = indices.len();
            return Some(VisibleSampleWindowFiles {
                total_count,
                rows: (window.window_start.min(total_count)..window.window_end.min(total_count))
                    .filter_map(|index| {
                        indices
                            .get(index)
                            .and_then(|file_index| folder.files.get(*file_index))
                    })
                    .collect(),
            });
        }

        let mut total_count = 0;
        let rows = indices
            .iter()
            .filter_map(|file_index| folder.files.get(*file_index))
            .filter(|file| {
                filters::audio_file_matches_parsed_tags(file, tags_by_file, &required_tags)
                    && playback_type_filter::playback_type_filter_matches(
                        file,
                        tags_by_file,
                        &playback_type_filter,
                    )
            })
            .filter_map(|file| {
                let row = (total_count >= window.window_start && total_count < window.window_end)
                    .then_some(file);
                total_count += 1;
                row
            })
            .collect();

        Some(VisibleSampleWindowFiles { total_count, rows })
    }

    pub(super) fn uncached_selected_audio_file_window_matching_tags(
        &self,
        window: radiant::prelude::VirtualListWindow,
        tags_by_file: &HashMap<String, Vec<String>>,
    ) -> VisibleSampleWindowFiles<'_> {
        self.window_from_browser_listing_snapshot(window, tags_by_file)
    }

    pub(in crate::native_app) fn selected_audio_file_index_matching_tags(
        &self,
        tags_by_file: &HashMap<String, Vec<String>>,
    ) -> Option<usize> {
        let selected = self.selection.selected_file.as_deref()?;
        if self.active_listing_reveal_id(Some(tags_by_file)).is_some() {
            return self
                .browser_listing_snapshot(tags_by_file)
                .index_of(selected);
        }
        if self.active_harvest_filter().is_some() {
            return self
                .browser_listing_snapshot(tags_by_file)
                .index_of(selected);
        }
        let required_tags = self.active_required_tags();
        let playback_type_filter = self.active_playback_type_filters();
        if let Some(collection) = self.selection.selected_collection {
            if required_tags.is_empty() && playback_type_filter.is_empty() {
                return self
                    .selected_collection_audio_file_ids_ref_with_sort_tags(
                        collection,
                        Some(tags_by_file),
                    )
                    .iter()
                    .position(|id| id == selected);
            }
            return self
                .selected_audio_files_matching_tags(tags_by_file)
                .iter()
                .position(|file| file.id == selected);
        }
        if self.folder_subtree_listing_enabled() {
            return self.selected_folder().and_then(|folder| {
                self.selected_folder_recursive_audio_file_index_matching_tags(
                    folder,
                    selected,
                    tags_by_file,
                )
            });
        }
        let folder = self.selected_folder()?;
        self.selected_folder_audio_file_indices_ref_with_sort_tags(folder, Some(tags_by_file))
            .iter()
            .filter_map(|file_index| folder.files.get(*file_index))
            .filter(|file| {
                filters::audio_file_matches_parsed_tags(file, tags_by_file, &required_tags)
                    && playback_type_filter::playback_type_filter_matches(
                        file,
                        tags_by_file,
                        &playback_type_filter,
                    )
            })
            .position(|file| file.id == selected)
    }

    pub(in crate::native_app) fn selected_source_audio_files(&self) -> Vec<&FileEntry> {
        let mut files = Vec::new();
        if let Some(folder) = self.selected_source_root_folder() {
            traversal::collect_audio_files(folder, &mut files);
        }
        self.sort_files(&mut files);
        files
    }

    pub(in crate::native_app) fn loaded_source_audio_files(&self) -> Vec<&FileEntry> {
        let mut files = Vec::new();
        for folder in self.loaded_source_root_folders() {
            traversal::collect_audio_files(folder, &mut files);
        }
        self.sort_files(&mut files);
        files
    }

    pub(super) fn loaded_source_root_folders(&self) -> Vec<&FolderEntry> {
        let mut folders = self
            .source
            .sources
            .iter()
            .filter_map(|source| source.root_folder.as_ref())
            .collect::<Vec<_>>();
        for folder in &self.tree.folders {
            if !folders
                .iter()
                .any(|source_root| source_root.id == folder.id)
            {
                folders.push(folder);
            }
        }
        folders
    }

    pub(super) fn selected_source_root_folder(&self) -> Option<&FolderEntry> {
        self.tree.folders.first().or_else(|| {
            self.source
                .sources
                .iter()
                .find(|source| source.id == self.source.selected_source)
                .and_then(|source| source.root_folder.as_ref())
        })
    }

    fn source_root_folder(&self, source_id: &str) -> Option<&FolderEntry> {
        if source_id == self.source.selected_source
            && let Some(folder) = self.tree.folders.first()
        {
            return Some(folder);
        }
        self.source
            .sources
            .iter()
            .find(|source| source.id == source_id)
            .and_then(|source| source.root_folder.as_ref())
    }

    fn selected_folder_audio_files_with_sort_tags<'a>(
        &self,
        folder: &'a FolderEntry,
        sort_tags: Option<&HashMap<String, Vec<String>>>,
    ) -> Vec<&'a FileEntry> {
        self.selected_folder_audio_file_indices_ref_with_sort_tags(folder, sort_tags)
            .iter()
            .filter_map(|index| folder.files.get(*index))
            .collect()
    }

    fn selected_folder_audio_file_indices_ref(&self, folder: &FolderEntry) -> Ref<'_, Vec<usize>> {
        self.selected_folder_audio_file_indices_ref_with_sort_tags(folder, None)
    }

    pub(super) fn selected_folder_audio_file_indices_ref_with_sort_tags(
        &self,
        folder: &FolderEntry,
        sort_tags: Option<&HashMap<String, Vec<String>>>,
    ) -> Ref<'_, Vec<usize>> {
        let name_filter = filters::normalized_name_filter(self.active_name_filter());
        let active_rating_filter = self.active_rating_filter();
        let rating_filter_key = rating_filter::rating_filter_key(&active_rating_filter);
        let listing_reveal_id = self.active_listing_reveal_id(sort_tags);
        let curation_key = if sort_tags.is_some() {
            self.filters.curation.cache_key()
        } else {
            String::new()
        };
        let request = VisibleSampleProjectionRequest::new(
            folder.id.as_str(),
            name_filter.as_str(),
            rating_filter_key.as_str(),
            curation_key.as_str(),
            &self.sample_list.file_sort,
            self.similarity_anchor_id(),
            self.sample_list.content_revision,
        )
        .with_listing_reveal(listing_reveal_id)
        .with_playback_type_tag_sort(self.playback_type_tag_sort_enabled(sort_tags));
        self.sample_list
            .projection_cache
            .audio_indices(request, || {
                let curation_now = curation::now_epoch_seconds();
                let mut indices = folder
                    .files
                    .iter()
                    .enumerate()
                    .filter(|(_, file)| {
                        file.is_audio()
                            && filters::audio_file_matches_name_query(file, &name_filter)
                            && rating_filter_allows_file(
                                file,
                                &active_rating_filter,
                                listing_reveal_id,
                            )
                            && curation_filter_allows_file(
                                file,
                                sort_tags,
                                &self.filters.curation,
                                curation_now,
                                listing_reveal_id,
                            )
                    })
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                if let Some(tags_by_file) = sort_tags {
                    ordering::sort_file_indices_matching_tags(
                        self,
                        folder,
                        &mut indices,
                        tags_by_file,
                    );
                } else {
                    ordering::sort_file_indices(self, folder, &mut indices);
                }
                ordering::sort_file_indices_by_similarity(self, folder, &mut indices);
                indices
            })
    }

    fn selected_folder_audio_file_indices_ref_with_sort_tags_if_cached(
        &self,
        folder: &FolderEntry,
        sort_tags: Option<&HashMap<String, Vec<String>>>,
    ) -> Option<Ref<'_, Vec<usize>>> {
        let name_filter = filters::normalized_name_filter(self.active_name_filter());
        let active_rating_filter = self.active_rating_filter();
        let rating_filter_key = rating_filter::rating_filter_key(&active_rating_filter);
        let listing_reveal_id = self.active_listing_reveal_id(sort_tags);
        let curation_key = if sort_tags.is_some() {
            self.filters.curation.cache_key()
        } else {
            String::new()
        };
        let request = VisibleSampleProjectionRequest::new(
            folder.id.as_str(),
            name_filter.as_str(),
            rating_filter_key.as_str(),
            curation_key.as_str(),
            &self.sample_list.file_sort,
            self.similarity_anchor_id(),
            self.sample_list.content_revision,
        )
        .with_listing_reveal(listing_reveal_id)
        .with_playback_type_tag_sort(self.playback_type_tag_sort_enabled(sort_tags));
        self.sample_list
            .projection_cache
            .cached_audio_indices(request)
    }

    pub(super) fn active_listing_reveal_id(
        &self,
        sort_tags: Option<&HashMap<String, Vec<String>>>,
    ) -> Option<&str> {
        if sort_tags.is_none() {
            return None;
        }
        let focused = self.selection.selected_file.as_deref()?;
        self.sample_list
            .listing_reveals
            .active_file_id_for_focus(Some(focused))
    }

    pub(super) fn playback_type_tag_sort_enabled(
        &self,
        sort_tags: Option<&HashMap<String, Vec<String>>>,
    ) -> bool {
        sort_tags.is_some()
            && sort_kind_for_details_sort(&self.sample_list.file_sort)
                == FileColumnKind::PlaybackType
    }

    #[cfg(test)]
    pub(in crate::native_app) fn selected_audio_projection_cache_len_for_tests(&self) -> usize {
        self.sample_list.projection_cache.len()
    }
}

pub(super) fn curation_filter_allows_file(
    file: &FileEntry,
    tags_by_file: Option<&HashMap<String, Vec<String>>>,
    mode: &curation::BrowserCurationMode,
    now: i64,
    listing_reveal_id: Option<&str>,
) -> bool {
    if !mode.enabled {
        return true;
    }
    let Some(tags_by_file) = tags_by_file else {
        return true;
    };
    curation::file_matches_curation(file, tags_by_file, mode, now)
        || listing_reveal_id == Some(file.id.as_str())
}

pub(super) fn rating_filter_allows_file(
    file: &FileEntry,
    rating_filter: &std::collections::BTreeSet<i8>,
    listing_reveal_id: Option<&str>,
) -> bool {
    rating_filter::rating_filter_matches(file, rating_filter)
        || listing_reveal_id == Some(file.id.as_str())
}

fn scoped_recursive_audio_files(folder: &FolderEntry) -> Vec<&FileEntry> {
    let mut files = Vec::new();
    traversal::collect_audio_files(folder, &mut files);
    files
}

fn collect_local_cache_candidate_paths(
    folder: &FolderEntry,
    name_query: &str,
    max_files: usize,
) -> Vec<PathBuf> {
    folder
        .files
        .iter()
        .filter(|file| file.is_audio() && filters::audio_file_matches_name_query(file, name_query))
        .take(max_files)
        .map(|file| PathBuf::from(&file.id))
        .collect()
}

fn collect_collection_cache_candidate_paths(
    folder: &FolderEntry,
    collection: wavecrate::sample_sources::SampleCollection,
    name_query: &str,
    max_files: usize,
    paths: &mut Vec<PathBuf>,
) {
    if paths.len() >= max_files {
        return;
    }

    paths.extend(
        folder
            .files
            .iter()
            .filter(|file| {
                file.is_audio()
                    && file.belongs_to_collection(collection)
                    && filters::audio_file_matches_name_query(file, name_query)
            })
            .take(max_files.saturating_sub(paths.len()))
            .map(|file| PathBuf::from(&file.id)),
    );

    for child in &folder.children {
        if paths.len() >= max_files {
            break;
        }
        collect_collection_cache_candidate_paths(child, collection, name_query, max_files, paths);
    }
}

fn collect_source_cache_candidate_paths(folder: &FolderEntry, paths: &mut Vec<PathBuf>) {
    paths.extend(
        folder
            .files
            .iter()
            .filter(|file| file.is_audio())
            .map(|file| PathBuf::from(&file.id)),
    );

    for child in &folder.children {
        collect_source_cache_candidate_paths(child, paths);
    }
}
