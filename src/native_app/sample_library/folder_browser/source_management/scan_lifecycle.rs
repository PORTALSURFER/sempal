use std::{collections::HashSet, path::PathBuf};

#[cfg(test)]
use super::super::scan_types::FolderScanDiscovery;
use super::super::{
    FolderBrowserState, FolderEntry, SourceEntry,
    path_helpers::{folder_label, path_id},
    scan_types::{
        FolderScanDiscoveryBatch, FolderScanRequest, FolderScanResult, FolderTreeRefreshResult,
        RatingHydrationStatus,
    },
    scanning::{merge_scan_discovery, placeholder_folder},
};
use crate::native_app::app::{BrowserProjectionDelta, PreparedFolderProjection};

impl FolderBrowserState {
    pub(in crate::native_app) fn source_projection_revision(&self, source_id: &str) -> Option<u64> {
        self.source
            .sources
            .iter()
            .find(|source| source.id == source_id)
            .and_then(|source| source.projection_revision)
    }

    #[cfg(test)]
    pub(in crate::native_app) fn apply_committed_projection_delta(
        &mut self,
        source_id: &str,
        delta: BrowserProjectionDelta,
    ) -> bool {
        self.apply_committed_projection(source_id, Some(delta), Vec::new())
    }

    pub(in crate::native_app) fn apply_committed_projection(
        &mut self,
        source_id: &str,
        delta: Option<BrowserProjectionDelta>,
        folder_projections: Vec<PreparedFolderProjection>,
    ) -> bool {
        let Some(source_index) = self
            .source
            .sources
            .iter()
            .position(|source| source.id == source_id)
        else {
            return false;
        };
        let Some(current_revision) = self.source.sources[source_index].projection_revision else {
            return false;
        };
        let source_root = self.source.sources[source_index].root.clone();
        let target_revision = delta
            .as_ref()
            .map(|delta| delta.manifest_revision)
            .or_else(|| {
                folder_projections
                    .first()
                    .map(|projection| projection.snapshot_revision)
            });
        let Some(target_revision) = target_revision else {
            return false;
        };
        let apply_manifest_delta = if let Some(delta) = &delta {
            if delta.manifest_revision < current_revision {
                return false;
            }
            if delta.manifest_revision == current_revision {
                false
            } else {
                let folder_snapshot_covers_delta =
                    folder_projections_cover_delta(&source_root, delta, &folder_projections);
                let revision_is_contiguous =
                    delta.manifest_revision == current_revision.saturating_add(1);
                if (!revision_is_contiguous && !folder_snapshot_covers_delta)
                    || delta.snapshot_revision != delta.manifest_revision
                {
                    tracing::info!(
                        source_id,
                        current_revision,
                        incoming_revision = delta.manifest_revision,
                        "Browser projection revision gap requires a full snapshot refresh"
                    );
                    return false;
                }
                true
            }
        } else if target_revision != current_revision {
            tracing::info!(
                source_id,
                current_revision,
                snapshot_revision = target_revision,
                "Folder projection snapshot is not at the current committed revision"
            );
            return false;
        } else {
            false
        };
        if folder_projections
            .iter()
            .any(|projection| projection.snapshot_revision != target_revision)
        {
            return false;
        }
        let selected = self.source.selected_source == source_id && self.source.selected_tree_loaded;
        let changed = {
            let root = if selected {
                self.tree.folders.first_mut()
            } else {
                self.source.sources[source_index].root_folder.as_mut()
            };
            let Some(root) = root else {
                return false;
            };
            let mut changed = false;
            for projection in &folder_projections {
                let folder_path = PathBuf::from(&root.id).join(&projection.relative_path);
                let folder_id = path_id(&folder_path);
                match &projection.folder {
                    Some(folder) => {
                        if root.id == folder_id {
                            changed |= *root != *folder;
                            *root = folder.clone();
                        } else {
                            let Some(parent) = folder_path.parent() else {
                                return false;
                            };
                            let Some(parent_folder) = root.find_mut(&path_id(parent)) else {
                                return false;
                            };
                            changed |= super::super::scanning::upsert_folder(
                                &mut parent_folder.children,
                                folder.clone(),
                            );
                        }
                    }
                    None => {
                        changed |= root.take_child_by_id(&folder_id).is_some();
                    }
                }
            }
            if apply_manifest_delta && let Some(delta) = &delta {
                let removed = delta
                    .removed_file_ids
                    .iter()
                    .cloned()
                    .collect::<HashSet<_>>();
                changed |= root.remove_files_by_ids(&removed);
                for folder in &delta.folders {
                    if root.ensure_folder_path(folder).is_none() {
                        return false;
                    }
                }
                for file in &delta.upserted_files {
                    let Some(parent) = PathBuf::from(&file.id).parent().map(PathBuf::from) else {
                        return false;
                    };
                    let Some(folder) = root.ensure_folder_path(&parent) else {
                        return false;
                    };
                    changed |= folder.upsert_projected_file(file.clone());
                }
            }
            changed
        };
        if apply_manifest_delta {
            self.source.sources[source_index].projection_revision = Some(target_revision);
        }
        if changed {
            if selected {
                self.retain_tree_state_after_selected_source_refresh();
            }
            self.bump_file_content_revision();
            self.mark_scan_content_refresh_pending();
            self.refresh_missing_collection_state();
        }
        true
    }

    pub(in crate::native_app) fn defer_add_source_path(
        &mut self,
        root: PathBuf,
        select_source: bool,
    ) -> Option<String> {
        if let Some(source) = self
            .source
            .sources
            .iter()
            .find(|source| source.root == root)
        {
            return Some(source.id.clone());
        }
        let source = retained_source_entry(&root)?;
        let id = source.id.clone();
        self.source.sources.push(source);
        if select_source {
            self.park_selected_source_tree();
            self.select_pending_source(id.clone(), placeholder_folder(&root));
        }
        Some(id)
    }

    pub(in crate::native_app) fn source_exists(&self, source_id: &str) -> bool {
        self.source
            .sources
            .iter()
            .any(|source| source.id == source_id)
    }

    pub(in crate::native_app) fn begin_add_source_path(
        &mut self,
        root: PathBuf,
        task_id: u64,
    ) -> Option<FolderScanRequest> {
        self.begin_add_source_path_with_selection(root, task_id, true)
    }

    pub(in crate::native_app) fn begin_add_source_path_preserving_selection(
        &mut self,
        root: PathBuf,
        task_id: u64,
    ) -> Option<FolderScanRequest> {
        self.begin_add_source_path_with_selection(root, task_id, false)
    }

    fn begin_add_source_path_with_selection(
        &mut self,
        root: PathBuf,
        task_id: u64,
        select_source: bool,
    ) -> Option<FolderScanRequest> {
        if let Some(index) = self
            .source
            .sources
            .iter()
            .position(|source| source.root == root)
        {
            let id = self.source.sources[index].id.clone();
            return if select_source {
                self.begin_select_source(id, task_id)
            } else {
                self.begin_source_scan_without_selection(index, task_id)
            };
        }
        let mut source = retained_source_entry(&root)?;
        let id = source.id.clone();
        let label = source.label.clone();
        source.loading_task = Some(task_id);
        let database_root = source.database_root.clone();
        if select_source {
            self.park_selected_source_tree();
        }
        self.source.sources.push(source);
        if select_source {
            self.select_pending_source(id.clone(), placeholder_folder(&root));
        }
        Some(FolderScanRequest {
            task_id,
            source_id: id,
            label,
            root,
            database_root,
            rating_decay_weeks: FolderScanRequest::default_rating_decay_weeks(),
        })
    }

    fn begin_source_scan_without_selection(
        &mut self,
        source_index: usize,
        task_id: u64,
    ) -> Option<FolderScanRequest> {
        if self.source.sources[source_index]
            .refresh_availability_from_disk()
            .is_missing()
        {
            return None;
        }
        if self.source.sources[source_index].root_folder.is_some()
            || self.source.sources[source_index].loading_task.is_some()
        {
            return None;
        }
        self.source.sources[source_index].loading_task = Some(task_id);
        let source = self.source.sources[source_index].clone();
        Some(FolderScanRequest {
            task_id,
            source_id: source.id,
            label: source.label,
            root: source.root,
            database_root: source.database_root,
            rating_decay_weeks: FolderScanRequest::default_rating_decay_weeks(),
        })
    }

    pub(in crate::native_app) fn begin_select_source(
        &mut self,
        id: String,
        task_id: u64,
    ) -> Option<FolderScanRequest> {
        let index = self
            .source
            .sources
            .iter()
            .position(|source| source.id == id)?;
        if self.source.sources[index]
            .refresh_availability_from_disk()
            .is_missing()
        {
            self.select_cached_or_placeholder_source(index);
            return None;
        }
        let selected_loaded = self.source.selected_source == id && self.selected_source_loaded();
        if selected_loaded {
            if self.source.sources[index].loading_task.is_some() {
                return None;
            }
            self.source.sources[index].loading_task = Some(task_id);
            let source = self.source.sources[index].clone();
            return Some(FolderScanRequest {
                task_id,
                source_id: source.id,
                label: source.label,
                root: source.root,
                database_root: source.database_root,
                rating_decay_weeks: FolderScanRequest::default_rating_decay_weeks(),
            });
        }
        if self.source.selected_source != id {
            self.park_selected_source_tree();
        }
        if let Some(root_folder) = self.source.sources[index].root_folder.take() {
            self.select_loaded_source(id, root_folder);
            if self.source.sources[index].loading_task.is_some() {
                return None;
            }
            self.source.sources[index].loading_task = Some(task_id);
            let source = self.source.sources[index].clone();
            return Some(FolderScanRequest {
                task_id,
                source_id: source.id,
                label: source.label,
                root: source.root,
                database_root: source.database_root,
                rating_decay_weeks: FolderScanRequest::default_rating_decay_weeks(),
            });
        }
        if self.source.sources[index].loading_task.is_some() {
            let root = self.source.sources[index].root.clone();
            self.select_pending_source(id, placeholder_folder(&root));
            return None;
        }
        self.source.sources[index].loading_task = Some(task_id);
        let source = self.source.sources[index].clone();
        self.select_pending_source(source.id.clone(), placeholder_folder(&source.root));
        Some(FolderScanRequest {
            task_id,
            source_id: source.id,
            label: source.label,
            root: source.root,
            database_root: source.database_root,
            rating_decay_weeks: FolderScanRequest::default_rating_decay_weeks(),
        })
    }

    pub(in crate::native_app) fn select_source_without_scan(&mut self, id: String) -> bool {
        let Some(index) = self
            .source
            .sources
            .iter()
            .position(|source| source.id == id)
        else {
            return false;
        };
        let source_missing = self.source.sources[index]
            .refresh_availability_from_disk()
            .is_missing();
        if self.source.selected_source == id && self.selected_source_loaded() && !source_missing {
            return true;
        }
        self.select_cached_or_placeholder_source(index);
        true
    }

    pub(in crate::native_app) fn begin_selected_source_scan(
        &mut self,
        task_id: u64,
    ) -> Option<FolderScanRequest> {
        self.begin_source_scan(self.source.selected_source.clone(), task_id)
    }

    pub(in crate::native_app) fn begin_source_scan(
        &mut self,
        id: String,
        task_id: u64,
    ) -> Option<FolderScanRequest> {
        let index = self
            .source
            .sources
            .iter()
            .position(|source| source.id == id)?;
        if self.source.sources[index]
            .refresh_availability_from_disk()
            .is_missing()
        {
            self.refresh_selected_source_from_cache_or_placeholder(index);
            return None;
        }
        if self.source.sources[index].loading_task.is_some() {
            return None;
        }
        self.source.sources[index].loading_task = Some(task_id);
        let source = self.source.sources[index].clone();
        Some(FolderScanRequest {
            task_id,
            source_id: source.id,
            label: source.label,
            root: source.root,
            database_root: source.database_root,
            rating_decay_weeks: FolderScanRequest::default_rating_decay_weeks(),
        })
    }

    pub(in crate::native_app) fn selected_source_loaded(&self) -> bool {
        self.source
            .sources
            .iter()
            .find(|source| source.id == self.source.selected_source)
            .is_some_and(|source| source.root_folder.is_some())
            || (self.source.selected_tree_loaded && self.tree.folders.first().is_some())
    }

    pub(in crate::native_app) fn source_tree_loaded(&self, source_id: &str) -> bool {
        let Some(source) = self
            .source
            .sources
            .iter()
            .find(|source| source.id == source_id)
        else {
            return false;
        };
        if self.source.selected_source == source_id {
            return self.source.selected_tree_loaded && self.tree.folders.first().is_some();
        }
        source.parked_tree_loaded && source.root_folder.is_some()
    }

    pub(in crate::native_app) fn apply_scan_finished(&mut self, result: FolderScanResult) -> bool {
        let Some(source_index) = self
            .source
            .sources
            .iter()
            .position(|source| source.id == result.source_id)
        else {
            return false;
        };
        if self.source.sources[source_index].loading_task != Some(result.task_id) {
            return false;
        }
        let source_id = self.source.sources[source_index].id.clone();
        let should_select = self.source.selected_source == source_id;
        let refreshing_selected_loaded_source = should_select && self.selected_source_loaded();
        self.source.sources[source_index].loading_task = None;
        if !result.source_root_available {
            self.source.sources[source_index].mark_missing();
            self.refresh_selected_source_from_cache_or_placeholder(source_index);
            self.mark_scan_content_refresh_pending();
            return true;
        }
        self.source.sources[source_index].mark_available();
        let Some(projection_revision) = result.metadata_hydration.revision() else {
            tracing::warn!(
                source_id = %source_id,
                error = result.metadata_hydration.error().unwrap_or("projection unavailable"),
                "Keeping the last good browser projection after snapshot hydration failed"
            );
            return true;
        };
        self.source.sources[source_index].projection_revision = Some(projection_revision);
        self.source.sources[source_index].missing_collection_snapshot =
            result.missing_collection_snapshot.clone();
        if should_select {
            if refreshing_selected_loaded_source {
                self.refresh_selected_source_tree(source_id, result.folder, true);
            } else {
                self.source.sources[source_index].root_folder = None;
                self.select_loaded_source(source_id, result.folder);
                self.refresh_missing_collection_state();
            }
            self.mark_scan_content_refresh_pending();
        } else {
            self.source.sources[source_index].root_folder = Some(result.folder);
            self.source.sources[source_index].parked_tree_loaded = true;
            self.bump_file_content_revision();
            self.mark_scan_content_refresh_pending();
            self.refresh_missing_collection_state();
        }
        true
    }

    pub(in crate::native_app) fn cancel_scan(&mut self, source_id: &str, task_id: u64) -> bool {
        let Some(source_index) = self
            .source
            .sources
            .iter()
            .position(|source| source.id == source_id)
        else {
            return false;
        };
        if self.source.sources[source_index].loading_task != Some(task_id) {
            return false;
        }
        let preserve_loaded_tree = self.source.selected_tree_loaded
            && self.source.selected_source == source_id
            || self.source.sources[source_index].parked_tree_loaded;
        self.source.sources[source_index].loading_task = None;
        if !preserve_loaded_tree {
            self.source.sources[source_index].root_folder = None;
            self.source.sources[source_index].parked_tree_loaded = false;
            if self.source.selected_source == source_id {
                let root = self.source.sources[source_index].root.clone();
                self.tree.folders = vec![placeholder_folder(&root)];
            }
        }
        true
    }

    pub(in crate::native_app) fn apply_folder_tree_refresh_result(
        &mut self,
        result: FolderTreeRefreshResult,
    ) -> bool {
        if self.source.selected_source != result.source_id {
            return false;
        }
        let Some(source_index) = self
            .source
            .sources
            .iter()
            .position(|source| source.id == result.source_id)
        else {
            return false;
        };
        if !result.source_root_available {
            self.source.sources[source_index].mark_missing();
            self.refresh_selected_source_from_cache_or_placeholder(source_index);
            return true;
        }
        self.source.sources[source_index].mark_available();
        self.source.sources[source_index].root_folder = None;
        self.source.selected_tree_loaded = true;
        let Some(root_folder) = self.tree.folders.first_mut() else {
            return false;
        };
        if root_folder.id != result.folder.id {
            return false;
        }
        let structure_changed = root_folder.replace_folder_structure(result.folder);
        let ratings_changed = match result.rating_hydration {
            RatingHydrationStatus::Complete { snapshot } => root_folder
                .apply_rating_snapshot(&self.source.sources[source_index].root, &snapshot),
            RatingHydrationStatus::Failed { error } => {
                tracing::warn!(source_id = %result.source_id, %error, "Keeping cached browser ratings after folder refresh hydration failure");
                false
            }
        };
        let changed = structure_changed || ratings_changed;
        if changed {
            self.retain_tree_state_after_selected_source_refresh();
            self.bump_file_content_revision();
            self.mark_scan_content_refresh_pending();
            self.refresh_missing_collection_state();
        }
        changed
    }

    pub(in crate::native_app) fn folder_tree_refresh_result_is_current(
        &self,
        result: &FolderTreeRefreshResult,
    ) -> bool {
        if self.source.selected_source != result.source_id {
            return false;
        }
        self.source
            .sources
            .iter()
            .find(|source| source.id == result.source_id)
            .is_some_and(|source| path_id(&source.root) == result.folder.id)
    }

    #[cfg(test)]
    pub(in crate::native_app::sample_library::folder_browser) fn apply_scan_discovered(
        &mut self,
        event: FolderScanDiscovery,
    ) -> bool {
        self.apply_scan_discovered_batch(FolderScanDiscoveryBatch {
            task_id: event.task_id,
            source_id: event.source_id.clone(),
            committed_revision: event.committed_revision,
            lifecycle_generation: None,
            sequence: 0,
            events: vec![event],
        })
    }

    pub(in crate::native_app) fn apply_scan_discovered_batch(
        &mut self,
        batch: FolderScanDiscoveryBatch,
    ) -> bool {
        if batch.events.is_empty() || batch.events.len() > 64 {
            return false;
        }
        let selected_source = self.source.selected_source == batch.source_id;
        let preserve_selected_tree = selected_source && self.source.selected_tree_loaded;
        let Some(source) = self
            .source
            .sources
            .iter_mut()
            .find(|source| source.id == batch.source_id)
        else {
            return false;
        };
        if source.loading_task != Some(batch.task_id) {
            return false;
        }
        if batch.events.iter().any(|event| {
            event.task_id != batch.task_id
                || event.source_id != batch.source_id
                || event.committed_revision != batch.committed_revision
        }) {
            return false;
        }
        if let (Some(current), Some(incoming)) =
            (source.projection_revision, batch.committed_revision)
            && incoming < current
        {
            return false;
        }
        if preserve_selected_tree || (!selected_source && source.root_folder.is_some()) {
            return false;
        }

        if self.source.selected_source == batch.source_id {
            let Some(root_folder) = self.tree.folders.first_mut() else {
                return false;
            };
            let mut changed = false;
            for event in &batch.events {
                changed |= merge_scan_discovery(root_folder, event);
            }
            if changed {
                self.bump_file_content_revision();
                self.mark_scan_content_refresh_pending();
            }
            return changed;
        }

        let root_folder = source
            .root_folder
            .get_or_insert_with(|| placeholder_folder(&source.root));
        let mut changed = false;
        for event in &batch.events {
            changed |= merge_scan_discovery(root_folder, event);
        }
        if changed && self.source.selected_source == batch.source_id {
            self.tree.folders = vec![root_folder.clone()];
            self.bump_file_content_revision();
            self.mark_scan_content_refresh_pending();
        }
        changed
    }
}

fn folder_projections_cover_delta(
    source_root: &std::path::Path,
    delta: &BrowserProjectionDelta,
    folder_projections: &[PreparedFolderProjection],
) -> bool {
    if folder_projections.is_empty() {
        return false;
    }
    let covered = |path: &std::path::Path| {
        folder_projections.iter().any(|projection| {
            let projection_path = source_root.join(&projection.relative_path);
            path == projection_path || path.starts_with(&projection_path)
        })
    };
    delta
        .upserted_files
        .iter()
        .all(|file| covered(std::path::Path::new(&file.id)))
        && delta
            .removed_file_ids
            .iter()
            .all(|file_id| covered(std::path::Path::new(file_id)))
        && delta.folders.iter().all(|folder| covered(folder))
}

fn retained_source_entry(root: &std::path::Path) -> Option<SourceEntry> {
    match wavecrate::sample_sources::library::lookup_retained_source_for_root(root) {
        Ok(Some(source)) => Some(SourceEntry::from_sample_source(&source)),
        Ok(None) => Some(SourceEntry::new(
            path_id(root),
            folder_label(root),
            root.to_path_buf(),
        )),
        Err(error) => {
            tracing::warn!(
                root = %root.display(),
                error = %error,
                "Could not restore retained source descriptor; source add was rejected"
            );
            None
        }
    }
}

impl FolderBrowserState {
    fn select_cached_or_placeholder_source(&mut self, source_index: usize) {
        let source_id = self.source.sources[source_index].id.clone();
        let source_root = self.source.sources[source_index].root.clone();
        if self.source.selected_source != source_id {
            self.park_selected_source_tree();
        }
        if let Some(root_folder) = self.source.sources[source_index].root_folder.take() {
            let parked_tree_loaded = self.source.sources[source_index].parked_tree_loaded;
            self.source.sources[source_index].parked_tree_loaded = false;
            if self.source.sources[source_index].loading_task.is_some() && !parked_tree_loaded {
                self.select_pending_source(source_id, root_folder);
            } else {
                self.select_loaded_source(source_id, root_folder);
            }
        } else {
            self.select_pending_source(source_id, placeholder_folder(&source_root));
        }
    }

    fn refresh_selected_source_from_cache_or_placeholder(&mut self, source_index: usize) {
        let source_id = self.source.sources[source_index].id.clone();
        if self.source.selected_source != source_id {
            return;
        }
        let source_root = self.source.sources[source_index].root.clone();
        let loaded = self.source.sources[source_index].root_folder.is_some();
        let root_folder = self.source.sources[source_index]
            .root_folder
            .take()
            .unwrap_or_else(|| placeholder_folder(&source_root));
        self.refresh_selected_source_tree(source_id, root_folder, loaded);
    }

    fn refresh_selected_source_tree(
        &mut self,
        source_id: String,
        root_folder: FolderEntry,
        loaded: bool,
    ) {
        self.source.selected_source = source_id;
        self.source.selected_tree_loaded = loaded;
        self.tree.folders = vec![root_folder];
        self.retain_tree_state_after_selected_source_refresh();
        self.reset_tree_view();
        self.bump_file_content_revision();
        self.mark_scan_content_refresh_pending();
        self.refresh_missing_collection_state();
    }

    fn retain_tree_state_after_selected_source_refresh(&mut self) {
        let root_id = self
            .tree
            .folders
            .first()
            .map(|folder| folder.id.clone())
            .unwrap_or_default();
        if root_id.is_empty() {
            return;
        }

        let still_available = self
            .tree
            .expanded_folders
            .iter()
            .filter(|id| self.find_folder(id).is_some())
            .cloned()
            .collect();
        self.tree.expanded_folders = still_available;
        self.tree.expanded_folders.insert(root_id.clone());

        if self.find_folder(&self.selection.selected_folder).is_some() {
            let mut existing_folder_ids = HashSet::new();
            for folder in &self.tree.folders {
                folder.collect_folder_ids(&mut existing_folder_ids);
            }
            self.selection
                .retain_existing_folders(&existing_folder_ids, root_id.clone());
            self.expand_selected_folder_ancestors();
            let visible_ids = self
                .selected_audio_files()
                .into_iter()
                .map(|file| file.id.clone())
                .collect();
            self.selection.retain_visible_files(&visible_ids);
            return;
        }

        self.selection.select_folder_after_tree_changed(root_id);
    }

    fn expand_selected_folder_ancestors(&mut self) {
        let selected = std::path::PathBuf::from(&self.selection.selected_folder);
        let mut cursor = selected.parent();
        while let Some(parent) = cursor {
            let id = path_id(parent);
            if self.find_folder(&id).is_some() {
                self.tree.expanded_folders.insert(id);
            }
            cursor = parent.parent();
        }
    }
}
