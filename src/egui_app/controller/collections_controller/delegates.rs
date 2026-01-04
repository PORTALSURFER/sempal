use super::*;
use crate::egui_app::state::FocusContext;

impl EguiController {
    /// Select a collection sample by index and load it into the waveform.
    pub fn select_collection_sample(&mut self, index: usize) {
        self.collections_ctrl().select_collection_sample(index);
    }

    /// Focus a collection sample row and replace the multi-selection set.
    pub fn focus_collection_sample_row(&mut self, row: usize) {
        self.collections_ctrl().focus_collection_sample_row(row);
    }

    /// Toggle a collection sample row in the multi-selection set.
    pub fn toggle_collection_sample_selection(&mut self, row: usize) {
        self.collections_ctrl()
            .toggle_collection_sample_selection(row);
    }

    /// Extend the collection selection to a row (shift).
    pub fn extend_collection_sample_selection_to_row(&mut self, row: usize) {
        self.collections_ctrl()
            .extend_collection_sample_selection_to_row(row);
    }

    /// Add a range of collection rows to the selection (shift + ctrl).
    pub fn add_range_collection_sample_selection(&mut self, row: usize) {
        self.collections_ctrl()
            .add_range_collection_sample_selection(row);
    }

    /// Clear the collection sample multi-selection set.
    pub fn clear_collection_sample_selection(&mut self) {
        self.collections_ctrl().clear_collection_sample_selection();
    }

    /// Select a collection row (or clear selection).
    pub fn select_collection_by_index(&mut self, index: Option<usize>) {
        self.collections_ctrl().select_collection_by_index(index);
    }

    /// Move collection selection by an offset.
    pub fn nudge_collection_row(&mut self, offset: isize) {
        self.collections_ctrl().nudge_collection_row(offset);
    }

    /// Create a new collection.
    pub fn add_collection(&mut self) {
        self.collections_ctrl().add_collection();
    }

    /// Delete a collection and update selection/exports.
    pub fn delete_collection(&mut self, collection_id: &CollectionId) -> Result<(), String> {
        self.collections_ctrl().delete_collection(collection_id)
    }

    /// Rename a collection.
    pub fn rename_collection(&mut self, collection_id: &CollectionId, new_name: String) {
        self.collections_ctrl()
            .rename_collection(collection_id, new_name);
    }

    /// Begin renaming the focused collection in the list.
    pub fn start_collection_rename(&mut self) {
        let Some(collection) = self.current_collection() else {
            self.set_status("Select a collection to rename it", StatusTone::Info);
            return;
        };
        self.focus_collections_list_context();
        self.ui.collections.pending_action =
            Some(crate::egui_app::state::CollectionActionPrompt::Rename {
                target: collection.id.clone(),
                name: collection.name.clone(),
            });
        self.ui.collections.rename_focus_requested = true;
    }

    /// Mark the collections list as the active focus surface.
    pub fn focus_collections_list_from_ui(&mut self) {
        self.focus_collections_list_context();
    }

    /// Cancel a pending collection rename.
    pub fn cancel_collection_rename(&mut self) {
        if matches!(
            self.ui.collections.pending_action,
            Some(crate::egui_app::state::CollectionActionPrompt::Rename { .. })
        ) {
            self.ui.collections.pending_action = None;
            self.ui.collections.rename_focus_requested = false;
        }
    }

    /// Apply a pending inline rename for the collection list.
    pub fn apply_pending_collection_rename(&mut self) {
        let action = self.ui.collections.pending_action.clone();
        if let Some(crate::egui_app::state::CollectionActionPrompt::Rename { target, name }) =
            action
        {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                self.set_status("Collection name cannot be empty", StatusTone::Error);
                return;
            }
            self.rename_collection(&target, trimmed.to_string());
            self.cancel_collection_rename();
        }
    }

    /// Add a sample from the current source to a collection.
    pub fn add_sample_to_collection(
        &mut self,
        collection_id: &CollectionId,
        relative_path: &Path,
    ) -> Result<(), String> {
        self.collections_ctrl()
            .add_sample_to_collection(collection_id, relative_path)
    }

    /// Add a sample from an explicit source to a collection.
    pub fn add_sample_to_collection_for_source(
        &mut self,
        collection_id: &CollectionId,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        self.collections_ctrl().add_sample_to_collection_for_source(
            collection_id,
            source,
            relative_path,
        )
    }

    /// Move a sample from a source into a collection export folder.
    pub fn move_sample_to_collection_for_source(
        &mut self,
        collection_id: &CollectionId,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<String, String> {
        self.collections_ctrl()
            .move_sample_to_collection(collection_id, source, relative_path)
    }

    /// Bind or clear a number hotkey (1-9) for a collection.
    pub fn bind_collection_hotkey(&mut self, collection_id: &CollectionId, hotkey: Option<u8>) {
        self.collections_ctrl()
            .bind_collection_hotkey(collection_id, hotkey);
    }

    /// Apply a collection hotkey in the given focus context.
    pub fn apply_collection_hotkey(&mut self, hotkey: u8, focus: FocusContext) -> bool {
        self.collections_ctrl()
            .apply_collection_hotkey(hotkey, focus)
    }

    pub(in crate::egui_app::controller) fn add_clip_to_collection(
        &mut self,
        collection_id: &CollectionId,
        clip_root: PathBuf,
        clip_relative_path: PathBuf,
    ) -> Result<(), String> {
        self.collections_ctrl()
            .add_clip_to_collection(collection_id, clip_root, clip_relative_path)
    }

    /// Move focused collection sample selection by an offset.
    pub fn nudge_collection_sample(&mut self, offset: isize) {
        self.collections_ctrl().nudge_collection_sample(offset);
    }

    /// Currently selected collection id (if any).
    pub fn current_collection_id(&self) -> Option<CollectionId> {
        self.selection_state.ctx.selected_collection.clone()
    }

    pub(in crate::egui_app::controller) fn refresh_collections_ui(&mut self) {
        self.collections_ctrl().refresh_collections_ui();
    }

    pub(in crate::egui_app::controller) fn ensure_collection_selection(&mut self) {
        self.collections_ctrl().ensure_collection_selection();
    }

    pub(in crate::egui_app::controller) fn ensure_sample_db_entry(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        self.collections_ctrl()
            .ensure_sample_db_entry(source, relative_path)
    }

    pub(in crate::egui_app::controller) fn current_collection(&self) -> Option<Collection> {
        let selected = self.selection_state.ctx.selected_collection.as_ref()?;
        self.library
            .collections
            .iter()
            .find(|c| &c.id == selected)
            .cloned()
    }
}
