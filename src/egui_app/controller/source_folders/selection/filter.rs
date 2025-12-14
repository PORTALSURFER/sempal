use super::super::*;

impl EguiController {
    pub(in crate::egui_app::controller) fn folder_selection_for_filter(
        &self,
    ) -> Option<&BTreeSet<PathBuf>> {
        let id = self.selected_source.as_ref()?;
        self.folder_browsers.get(id).map(|model| &model.selected)
    }

    pub(in crate::egui_app::controller) fn folder_filter_accepts(&self, relative_path: &Path) -> bool {
        let Some(selection) = self.folder_selection_for_filter() else {
            return true;
        };
        if selection.is_empty() {
            return true;
        }
        selection
            .iter()
            .any(|folder| relative_path.starts_with(folder))
    }
}
