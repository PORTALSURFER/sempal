use super::super::*;
use std::collections::BTreeSet;

impl EguiController {
    pub(in crate::egui_app::controller) fn folder_selection_for_filter(
        &self,
    ) -> Option<&BTreeSet<PathBuf>> {
        let id = self.selection_state.ctx.selected_source.as_ref()?;
        self.ui_cache
            .folders
            .models
            .get(id)
            .map(|model| &model.selected)
    }

    pub(in crate::egui_app::controller) fn folder_negation_for_filter(
        &self,
    ) -> Option<&BTreeSet<PathBuf>> {
        let id = self.selection_state.ctx.selected_source.as_ref()?;
        self.ui_cache
            .folders
            .models
            .get(id)
            .map(|model| &model.negated)
    }

    #[allow(dead_code)]
    pub(in crate::egui_app::controller) fn folder_filter_accepts(
        &self,
        relative_path: &Path,
    ) -> bool {
        let selection = self.folder_selection_for_filter();
        let negated = self.folder_negation_for_filter();
        folder_filter_accepts(relative_path, selection, negated)
    }
}

fn folder_filter_accepts(
    relative_path: &Path,
    selection: Option<&BTreeSet<PathBuf>>,
    negated: Option<&BTreeSet<PathBuf>>,
) -> bool {
    let selected = selection
        .map(|set| set.iter().any(|folder| relative_path.starts_with(folder)))
        .unwrap_or(true);
    let has_selection = selection.is_some_and(|set| !set.is_empty());
    if has_selection && !selected {
        return false;
    }
    let excluded = negated
        .map(|set| is_negated_relative_path(relative_path, set))
        .unwrap_or(false);
    !excluded
}

fn is_negated_relative_path(relative_path: &Path, negated: &BTreeSet<PathBuf>) -> bool {
    if negated.is_empty() {
        return false;
    }
    let root_negated = negated.contains(Path::new(""));
    if root_negated {
        let parent = relative_path.parent().unwrap_or(Path::new(""));
        if parent.as_os_str().is_empty() {
            return true;
        }
    }
    negated
        .iter()
        .filter(|folder| !folder.as_os_str().is_empty())
        .any(|folder| relative_path.starts_with(folder))
}
