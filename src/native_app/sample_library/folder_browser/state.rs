use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use wavecrate::sample_sources::config::SimilarityAspectSettings;

use super::{
    BrowserDragDropState, BrowserFilterState, BrowserPanelLayoutState, BrowserRenameState,
    BrowserSelectionState, BrowserSourceState, CollectionPanelState,
    EMPTY_SIMILARITY_ASPECT_STRENGTHS, FolderBrowserMessage, FolderEntry,
    FolderSelectionToggleResult, FolderTreeState, SampleListState, SimilarityAspectStrengths,
    SimilarityBrowserState, SourceEntry, path_id_matches, placeholder_folder,
};
#[cfg(test)]
use super::{folder_label, path_id};

#[derive(Clone, Debug)]
pub(in crate::native_app) struct FolderBrowserState {
    pub(super) source: BrowserSourceState,
    pub(super) selection: BrowserSelectionState,
    pub(super) filters: BrowserFilterState,
    pub(super) tree: FolderTreeState,
    pub(super) rename: BrowserRenameState,
    pub(super) drag_drop: BrowserDragDropState,
    pub(super) collection_panel: CollectionPanelState,
    pub(super) panel_layout: BrowserPanelLayoutState,
    pub(super) sample_list: SampleListState,
    /// Most recent keyboard or pointer focus activity. Pointer focus records
    /// its exact folder target when selection state alone is ambiguous.
    keyboard_focus_shown_at: Option<Instant>,
    keyboard_focus_hold: Duration,
    keyboard_focus_alpha: u8,
    pub(super) pointer_focused_folder_id: Option<String>,
    pub(super) selection_flash_item_id: Option<String>,
    pub(super) selection_flash_frames: u8,
}

const KEYBOARD_FOCUS_HOLD: Duration = Duration::from_millis(800);
const KEYBOARD_FOCUS_FADE: Duration = Duration::from_millis(250);

impl FolderBrowserState {
    #[cfg(test)]
    pub(in crate::native_app) fn load_default() -> Self {
        Self::empty()
    }

    #[cfg(test)]
    pub(in crate::native_app) fn from_root(root: PathBuf) -> Self {
        let source_id = path_id(&root);
        let label = folder_label(&root);
        let sources = vec![SourceEntry::new(source_id.clone(), label, root)];
        Self::from_sources(sources, source_id)
    }

    #[cfg(test)]
    pub(super) fn from_sources(sources: Vec<SourceEntry>, selected_source: String) -> Self {
        let mut sources = sources;
        let source_index = selected_source_index(&sources, &selected_source);
        let snapshot = super::load_source_snapshot(
            sources[source_index].root.clone(),
            sources[source_index].database_root.clone(),
        );
        sources[source_index].root_folder = Some(snapshot.folder.clone());
        sources[source_index].missing_collection_snapshot = snapshot.missing_collection_snapshot;
        Self::new(sources, source_index, snapshot.folder, true)
    }

    pub(in crate::native_app) fn from_sources_deferred(
        sources: Vec<SourceEntry>,
        selected_source: String,
    ) -> Self {
        let source_index = selected_source_index(&sources, &selected_source);
        let selected_tree_loaded = sources[source_index].root_folder.is_some();
        let root_folder = sources[source_index]
            .root_folder
            .clone()
            .unwrap_or_else(|| placeholder_folder(&sources[source_index].root));
        Self::new(sources, source_index, root_folder, selected_tree_loaded)
    }

    fn new(
        sources: Vec<SourceEntry>,
        source_index: usize,
        root_folder: FolderEntry,
        selected_tree_loaded: bool,
    ) -> Self {
        let root_id = root_folder.id.clone();
        let selected_source = sources[source_index].id.clone();
        let mut state = Self {
            source: BrowserSourceState::new(sources, selected_source, selected_tree_loaded),
            selection: BrowserSelectionState::new(root_id.clone()),
            filters: BrowserFilterState::default(),
            tree: FolderTreeState::new(root_folder, root_id),
            rename: BrowserRenameState::default(),
            drag_drop: BrowserDragDropState::new(),
            collection_panel: CollectionPanelState::new(),
            panel_layout: BrowserPanelLayoutState::new(),
            sample_list: SampleListState::new(),
            keyboard_focus_shown_at: None,
            keyboard_focus_hold: Duration::ZERO,
            keyboard_focus_alpha: 0,
            pointer_focused_folder_id: None,
            selection_flash_item_id: None,
            selection_flash_frames: 0,
        };
        state.refresh_missing_collection_state();
        state
    }

    pub(in crate::native_app::sample_library::folder_browser) fn empty() -> Self {
        let root_id = String::new();
        Self {
            source: BrowserSourceState::new(Vec::new(), String::new(), false),
            selection: BrowserSelectionState::new(root_id.clone()),
            filters: BrowserFilterState::default(),
            tree: FolderTreeState::new(
                FolderEntry {
                    id: root_id.clone(),
                    name: String::new(),
                    children: Vec::new(),
                    files: Vec::new(),
                },
                root_id,
            ),
            rename: BrowserRenameState::default(),
            drag_drop: BrowserDragDropState::new(),
            collection_panel: CollectionPanelState::new(),
            panel_layout: BrowserPanelLayoutState::new(),
            sample_list: SampleListState::new(),
            keyboard_focus_shown_at: None,
            keyboard_focus_hold: Duration::ZERO,
            keyboard_focus_alpha: 0,
            pointer_focused_folder_id: None,
            selection_flash_item_id: None,
            selection_flash_frames: 0,
        }
    }

    pub(in crate::native_app) fn keyboard_focus_visible(&self) -> bool {
        self.keyboard_focus_alpha() > 0
    }

    pub(in crate::native_app) fn keyboard_focus_alpha(&self) -> u8 {
        self.keyboard_focus_alpha
    }

    pub(in crate::native_app) fn keyboard_focus_alpha_at(&self, now: Instant) -> u8 {
        let Some(shown_at) = self.keyboard_focus_shown_at else {
            return 0;
        };
        let elapsed = now.checked_duration_since(shown_at).unwrap_or_default();
        if elapsed <= self.keyboard_focus_hold {
            return u8::MAX;
        }
        let fade_elapsed = elapsed.saturating_sub(self.keyboard_focus_hold);
        if fade_elapsed >= KEYBOARD_FOCUS_FADE {
            return 0;
        }
        let remaining = 1.0 - fade_elapsed.as_secs_f32() / KEYBOARD_FOCUS_FADE.as_secs_f32();
        (remaining * f32::from(u8::MAX)).round() as u8
    }

    pub(in crate::native_app) fn show_keyboard_focus(&mut self) {
        self.pointer_focused_folder_id = None;
        self.show_focus_at(Instant::now(), KEYBOARD_FOCUS_HOLD);
    }

    pub(in crate::native_app) fn show_pointer_focus(&mut self) {
        self.pointer_focused_folder_id = None;
        self.show_focus_at(Instant::now(), Duration::ZERO);
    }

    pub(in crate::native_app) fn show_pointer_focus_for_folder(&mut self, folder_id: String) {
        self.pointer_focused_folder_id = Some(folder_id);
        self.show_focus_at(Instant::now(), Duration::ZERO);
    }

    fn show_focus_at(&mut self, shown_at: Instant, hold: Duration) {
        self.keyboard_focus_shown_at = Some(shown_at);
        self.keyboard_focus_hold = hold;
        self.keyboard_focus_alpha = u8::MAX;
    }

    pub(in crate::native_app) fn advance_keyboard_focus_fade(&mut self, now: Instant) {
        self.keyboard_focus_alpha = self.keyboard_focus_alpha_at(now);
    }

    pub(in crate::native_app) fn hide_keyboard_focus(&mut self) {
        self.keyboard_focus_shown_at = None;
        self.keyboard_focus_hold = Duration::ZERO;
        self.keyboard_focus_alpha = 0;
        self.pointer_focused_folder_id = None;
    }

    #[cfg(test)]
    pub(in crate::native_app) fn root_path(&self) -> &std::path::Path {
        self.tree
            .folders
            .first()
            .map(|folder| std::path::Path::new(&folder.id))
            .unwrap_or_else(|| std::path::Path::new(""))
    }

    #[cfg(test)]
    pub(in crate::native_app) fn source_labels(&self) -> Vec<String> {
        self.source_labels_for_tests()
    }

    pub(in crate::native_app) fn selected_file_id(&self) -> Option<&str> {
        self.selection.selected_file_id()
    }

    #[cfg(test)]
    pub(in crate::native_app) fn remove_loaded_source_folder_for_tests(
        &mut self,
        folder_id: &str,
    ) -> bool {
        self.source
            .sources
            .iter_mut()
            .find(|source| source.id == self.source.selected_source)
            .and_then(|source| source.root_folder.as_mut())
            .and_then(|root| root.take_child_by_id(folder_id))
            .is_some()
    }

    pub(in crate::native_app) fn toggle_focused_folder_selection(
        &mut self,
    ) -> Option<FolderSelectionToggleResult> {
        self.show_keyboard_focus();
        if self.rename_active() || self.selection.selected_collection.is_some() {
            return None;
        }
        let previous_folder_id = self.selection.selected_folder.clone();
        let visible_ids = self
            .visible_folders()
            .into_iter()
            .map(|folder| folder.id)
            .collect::<Vec<_>>();
        let folder_id = self.selection.selected_folder.clone();
        let selected = self.selection.toggle_focused_folder(&visible_ids)?;
        self.clear_similarity_anchor_after_folder_change(&previous_folder_id);
        Some(FolderSelectionToggleResult {
            folder_id,
            selected,
            selected_count: self.selection.selected_folder_count(),
        })
    }

    pub(in crate::native_app) fn similarity_anchor_id(&self) -> Option<&str> {
        self.sample_list
            .similarity
            .as_ref()
            .map(SimilarityBrowserState::anchor_id)
    }

    pub(in crate::native_app) fn similarity_mode_active(&self) -> bool {
        self.sample_list.similarity.is_some()
    }

    pub(in crate::native_app) fn similarity_controls(&self) -> &SimilarityAspectSettings {
        self.sample_list
            .similarity
            .as_ref()
            .map(SimilarityBrowserState::controls)
            .unwrap_or(&self.sample_list.similarity_controls)
    }

    pub(in crate::native_app) fn set_similarity_controls(
        &mut self,
        controls: SimilarityAspectSettings,
    ) {
        let controls = controls.normalized();
        let changed = self.sample_list.similarity_controls != controls;
        self.sample_list.similarity_controls = controls.clone();
        if let Some(similarity) = self.sample_list.similarity.as_mut() {
            similarity.set_controls(controls);
        }
        if changed {
            self.bump_file_content_revision();
        }
    }

    pub(in crate::native_app) fn random_navigation_enabled(&self) -> bool {
        self.sample_list.random_navigation.enabled
    }

    pub(in crate::native_app) fn toggle_random_navigation(&mut self) -> bool {
        let enabled = !self.sample_list.random_navigation.enabled;
        let file_ids = self.selected_audio_file_ids();
        self.sample_list.random_navigation.set_enabled(
            enabled,
            self.selection.selected_file_id(),
            &file_ids,
        );
        enabled
    }

    pub(in crate::native_app) fn file_is_similarity_anchor(&self, file_id: &str) -> bool {
        self.similarity_anchor_id() == Some(file_id)
    }

    pub(in crate::native_app) fn similarity_display_strength_for_file(
        &self,
        file_id: &str,
    ) -> Option<f32> {
        self.sample_list
            .similarity
            .as_ref()
            .and_then(|similarity| similarity.display_strength_for(file_id))
    }

    pub(in crate::native_app) fn similarity_aspect_display_strengths_for_file(
        &self,
        file_id: &str,
    ) -> SimilarityAspectStrengths {
        self.sample_list
            .similarity
            .as_ref()
            .map(|similarity| similarity.aspect_display_strengths_for(file_id))
            .unwrap_or(EMPTY_SIMILARITY_ASPECT_STRENGTHS)
    }

    pub(in crate::native_app) fn toggle_similarity_anchor(&mut self, file_id: String) {
        if self.file_is_similarity_anchor(&file_id) {
            self.clear_similarity_anchor();
        } else {
            self.sample_list.similarity = Some(SimilarityBrowserState::new(
                file_id,
                self.sample_list.similarity_controls.clone(),
            ));
            self.bump_file_content_revision();
        }
    }

    pub(in crate::native_app) fn clear_similarity_anchor(&mut self) -> bool {
        let had_anchor = self.sample_list.similarity.take().is_some();
        if had_anchor {
            self.bump_file_content_revision();
        }
        had_anchor
    }

    pub(super) fn clear_similarity_anchor_after_folder_change(&mut self, previous_folder_id: &str) {
        if self.selection.selected_folder != previous_folder_id {
            self.clear_similarity_anchor();
        }
    }

    pub(super) fn rewrite_similarity_path_prefix(&mut self, old_path: &Path, new_path: &Path) {
        let Some(similarity) = self.sample_list.similarity.as_mut() else {
            return;
        };
        if similarity.rewrite_path_prefix(old_path, new_path) {
            self.bump_file_content_revision();
        }
    }

    pub(in crate::native_app) fn set_similarity_scores_with_aspects(
        &mut self,
        anchor_id: String,
        scores_by_file: HashMap<String, f32>,
        aspect_scores_by_file: HashMap<String, SimilarityAspectStrengths>,
    ) {
        self.sample_list.similarity = Some(SimilarityBrowserState::with_scores_and_aspects(
            anchor_id,
            self.sample_list.similarity_controls.clone(),
            scores_by_file,
            aspect_scores_by_file,
        ));
        self.bump_file_content_revision();
    }

    #[cfg(test)]
    pub(in crate::native_app) fn set_similarity_scores_for_tests(
        &mut self,
        anchor_id: String,
        scores_by_file: HashMap<String, f32>,
    ) {
        self.set_similarity_scores_with_aspects(anchor_id, scores_by_file, HashMap::new());
    }

    pub(in crate::native_app) fn folder_path(&self, folder_id: &str) -> Option<PathBuf> {
        self.find_folder(folder_id)
            .map(|folder| PathBuf::from(&folder.id))
    }

    pub(in crate::native_app) fn context_sample_path(&self, file_id: &str) -> Option<PathBuf> {
        if self.selection.selected_file_ids.contains(file_id)
            && let Some(focused) = self.selection.selected_file.as_deref()
            && self.selection.selected_file_ids.contains(focused)
        {
            return Some(PathBuf::from(focused));
        }

        let requested_path = Path::new(file_id);
        self.loaded_source_audio_files()
            .into_iter()
            .find(|file| path_id_matches(&file.id, requested_path))
            .map(|file| PathBuf::from(&file.id))
    }

    pub(in crate::native_app) fn is_file_selected(&self, file_id: &str) -> bool {
        self.selection.selected_file_ids_contains(file_id)
    }

    pub(in crate::native_app) fn scan_is_active(&self, source_id: &str, task_id: u64) -> bool {
        self.source
            .sources
            .iter()
            .any(|source| source.id == source_id && source.loading_task == Some(task_id))
    }

    pub(in crate::native_app) fn apply_message(&mut self, message: FolderBrowserMessage) {
        match message {
            FolderBrowserMessage::AddSource
            | FolderBrowserMessage::SelectSource(_)
            | FolderBrowserMessage::NavigateSource(_)
            | FolderBrowserMessage::DragSource(_, _)
            | FolderBrowserMessage::OpenSourceContextMenu(_, _)
            | FolderBrowserMessage::OpenCollectionContextMenu(_, _)
            | FolderBrowserMessage::BeginRenameSelected
            | FolderBrowserMessage::BeginCreateSubfolder
            | FolderBrowserMessage::RenameInput(_)
            | FolderBrowserMessage::DropOnCollection(_)
            | FolderBrowserMessage::DropOnSource(_) => {}
            FolderBrowserMessage::DropOnFolder(_) => {}
            FolderBrowserMessage::ToggleFolderSubtreeListing => {
                self.toggle_folder_subtree_listing();
            }
            FolderBrowserMessage::ToggleEmptyFolderVisibility => {
                self.toggle_empty_folder_visibility();
                self.sync_tree_view_to_selection(
                    super::FOLDER_TREE_PROJECTED_VIEWPORT_ROWS,
                    super::FOLDER_TREE_OVERSCAN_ROWS,
                    super::FOLDER_TREE_EDGE_CONTEXT_ROWS,
                );
            }
            FolderBrowserMessage::NameFilterInput(message) => {
                self.apply_name_filter_input(message);
            }
            FolderBrowserMessage::TagFilterInput(message) => {
                self.apply_tag_filter_input(message);
            }
            FolderBrowserMessage::SetFilterFamilyEnabled(family, enabled) => {
                self.set_filter_family_enabled(family, enabled);
            }
            FolderBrowserMessage::TogglePlaybackTypeFilter(filter, enabled) => {
                self.set_playback_type_filter(filter, enabled);
            }
            FolderBrowserMessage::ToggleRatingFilter(level, enabled) => {
                self.set_rating_filter(level, enabled);
            }
            FolderBrowserMessage::SetCurationScope(scope, enabled) => {
                self.set_curation_scope(scope, enabled);
            }
            FolderBrowserMessage::SetHarvestFilter(filter, enabled) => {
                self.set_harvest_filter(filter, enabled);
            }
            FolderBrowserMessage::ClearDropTarget(position) => {
                self.clear_drop_target_folder(position);
                self.clear_drop_target_source(position);
            }
            FolderBrowserMessage::ClearDropTargetUnless(id, position) => {
                self.clear_drop_target_folder_unless(&id, position);
            }
            FolderBrowserMessage::ClearSourceDropTargetUnless(id, position) => {
                self.clear_drop_target_source_unless(&id, position);
            }
            FolderBrowserMessage::HoverDropTarget(id, position) => {
                self.update_drag_pointer(position);
                self.hover_drop_target_folder(&id);
            }
            FolderBrowserMessage::HoverSourceDropTarget(id, position) => {
                self.update_drag_pointer(position);
                self.hover_drop_target_source(&id);
            }
            FolderBrowserMessage::ActivateFolder(id, modifiers) => {
                self.cancel_rename();
                self.activate_folder_with_modifiers(id, modifiers);
                self.sync_tree_view_to_selection(
                    super::FOLDER_TREE_PROJECTED_VIEWPORT_ROWS,
                    super::FOLDER_TREE_OVERSCAN_ROWS,
                    super::FOLDER_TREE_EDGE_CONTEXT_ROWS,
                );
            }
            FolderBrowserMessage::ToggleFolderExpansion(id) => {
                self.cancel_rename();
                self.toggle_folder_expansion(id);
                self.sync_tree_view_to_selection(
                    super::FOLDER_TREE_PROJECTED_VIEWPORT_ROWS,
                    super::FOLDER_TREE_OVERSCAN_ROWS,
                    super::FOLDER_TREE_EDGE_CONTEXT_ROWS,
                );
            }
            FolderBrowserMessage::OpenFolderContextMenu(_, _) => {}
            FolderBrowserMessage::CancelRename => {
                self.cancel_rename();
            }
            FolderBrowserMessage::DragFolder(id, message) => {
                self.apply_folder_drag(id, message);
            }
            FolderBrowserMessage::SortFileColumn(column_id) => {
                self.sort_file_column(column_id);
            }
            FolderBrowserMessage::ResizeFileColumn(column_id, message) => {
                self.resize_file_column(column_id, message);
            }
            FolderBrowserMessage::DragFileColumn(column_id, message) => {
                self.drag_file_column(column_id, message);
            }
            FolderBrowserMessage::CancelFileColumnDrag => {
                self.cancel_file_column_drag();
            }
            FolderBrowserMessage::ExitCollectionFocus => {
                self.exit_collection_focus();
            }
            FolderBrowserMessage::ToggleSimilarityAnchor(file_id) => {
                self.toggle_similarity_anchor(file_id);
            }
            FolderBrowserMessage::ResizeCollectionsPanel(message) => {
                self.resize_collections_panel(message);
            }
            FolderBrowserMessage::ResizeFilterPanel(message) => {
                self.resize_filter_panel(message);
            }
            FolderBrowserMessage::ResizeMetadataPanel(message) => {
                self.resize_metadata_panel(message);
            }
            FolderBrowserMessage::ActivateCollection(collection) => {
                self.activate_collection(collection);
            }
            FolderBrowserMessage::RenameCollection(collection) => {
                self.begin_rename_collection(collection);
            }
            FolderBrowserMessage::HoverCollectionDropTarget(collection, position) => {
                self.hover_drop_target_collection(collection, position);
            }
        }
    }

    pub(super) fn bump_file_content_revision(&mut self) {
        self.sample_list.bump_content_revision();
    }

    pub(in crate::native_app) fn visible_sample_window_needs_content_refresh(&self) -> bool {
        self.sample_list.prepared_content_revision != self.sample_list.content_revision
    }

    pub(in crate::native_app) fn invalidate_visible_sample_projection_cache(&mut self) {
        self.sample_list.projection_cache.clear();
    }
}

#[cfg(test)]
mod keyboard_focus_tests {
    use super::*;

    #[test]
    fn keyboard_focus_holds_then_fades_without_clearing_navigation_state() {
        let mut state = FolderBrowserState::empty();
        let shown_at = Instant::now();
        state.show_focus_at(shown_at, KEYBOARD_FOCUS_HOLD);

        assert_eq!(state.keyboard_focus_alpha_at(shown_at), u8::MAX);
        assert_eq!(
            state.keyboard_focus_alpha_at(shown_at + KEYBOARD_FOCUS_HOLD),
            u8::MAX
        );
        let midway = shown_at + KEYBOARD_FOCUS_HOLD + KEYBOARD_FOCUS_FADE / 2;
        assert!(state.keyboard_focus_alpha_at(midway).abs_diff(128) <= 1);
        assert_eq!(
            state.keyboard_focus_alpha_at(shown_at + KEYBOARD_FOCUS_HOLD + KEYBOARD_FOCUS_FADE),
            0
        );
        assert!(state.keyboard_focus_shown_at.is_some());
    }

    #[test]
    fn pointer_focus_fades_immediately_and_restarts_from_full_on_every_click() {
        let mut state = FolderBrowserState::empty();
        let first_click = Instant::now();
        state.show_focus_at(first_click, Duration::ZERO);
        let midway = first_click + KEYBOARD_FOCUS_FADE / 2;

        assert!(state.keyboard_focus_alpha_at(midway).abs_diff(128) <= 1);
        state.show_focus_at(midway, Duration::ZERO);
        assert_eq!(state.keyboard_focus_alpha_at(midway), u8::MAX);
        assert!(
            state
                .keyboard_focus_alpha_at(midway + KEYBOARD_FOCUS_FADE / 2)
                .abs_diff(128)
                <= 1
        );
        assert_eq!(
            state.keyboard_focus_alpha_at(midway + KEYBOARD_FOCUS_FADE),
            0
        );
    }

    #[test]
    fn frame_advancement_commits_focus_opacity_for_projection() {
        let mut state = FolderBrowserState::empty();
        let shown_at = Instant::now();
        state.show_focus_at(shown_at, Duration::ZERO);

        assert_eq!(state.keyboard_focus_alpha(), u8::MAX);
        state.advance_keyboard_focus_fade(shown_at + KEYBOARD_FOCUS_FADE / 2);
        assert!(state.keyboard_focus_alpha().abs_diff(128) <= 1);
        state.advance_keyboard_focus_fade(shown_at + KEYBOARD_FOCUS_FADE);
        assert_eq!(state.keyboard_focus_alpha(), 0);
    }
}

fn selected_source_index(sources: &[SourceEntry], selected_source: &str) -> usize {
    sources
        .iter()
        .position(|source| source.id == selected_source)
        .or(if sources.is_empty() { None } else { Some(0) })
        .expect("folder browser needs at least one source")
}
