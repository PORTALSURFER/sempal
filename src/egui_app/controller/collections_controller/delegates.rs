use super::*;

impl EguiController {
    /// Select a collection sample by index and load it into the waveform.
    pub fn select_collection_sample(&mut self, index: usize) {
        self.collections_ctrl().select_collection_sample(index);
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
        self.selected_collection.clone()
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
        let selected = self.selected_collection.as_ref()?;
        self.collections.iter().find(|c| &c.id == selected).cloned()
    }
}
