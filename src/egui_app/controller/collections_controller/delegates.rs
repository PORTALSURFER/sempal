use super::*;

impl EguiController {
    pub fn select_collection_sample(&mut self, index: usize) {
        self.collections_ctrl().select_collection_sample(index);
    }

    pub fn select_collection_by_index(&mut self, index: Option<usize>) {
        self.collections_ctrl().select_collection_by_index(index);
    }

    pub fn nudge_collection_row(&mut self, offset: isize) {
        self.collections_ctrl().nudge_collection_row(offset);
    }

    pub fn add_collection(&mut self) {
        self.collections_ctrl().add_collection();
    }

    pub fn delete_collection(&mut self, collection_id: &CollectionId) -> Result<(), String> {
        self.collections_ctrl().delete_collection(collection_id)
    }

    pub fn rename_collection(&mut self, collection_id: &CollectionId, new_name: String) {
        self.collections_ctrl()
            .rename_collection(collection_id, new_name);
    }

    pub fn add_sample_to_collection(
        &mut self,
        collection_id: &CollectionId,
        relative_path: &Path,
    ) -> Result<(), String> {
        self.collections_ctrl()
            .add_sample_to_collection(collection_id, relative_path)
    }

    pub fn add_sample_to_collection_for_source(
        &mut self,
        collection_id: &CollectionId,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        self.collections_ctrl()
            .add_sample_to_collection_for_source(collection_id, source, relative_path)
    }

    pub fn nudge_collection_sample(&mut self, offset: isize) {
        self.collections_ctrl().nudge_collection_sample(offset);
    }

    pub fn current_collection_id(&self) -> Option<CollectionId> {
        self.selected_collection.clone()
    }

    pub(in crate::egui_app::controller) fn refresh_collections_ui(&mut self) {
        self.collections_ctrl().refresh_collections_ui();
    }

    pub(in crate::egui_app::controller) fn refresh_collection_samples(&mut self) {
        self.collections_ctrl().refresh_collection_samples();
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
