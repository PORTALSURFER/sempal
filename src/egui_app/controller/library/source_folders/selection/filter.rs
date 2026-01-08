use super::super::*;
use std::collections::BTreeSet;

impl EguiController {
    pub(crate) fn folder_selection_for_filter(
        &self,
    ) -> Option<&BTreeSet<PathBuf>> {
        let id = self.selection_state.ctx.selected_source.as_ref()?;
        self.ui_cache
            .folders
            .models
            .get(id)
            .map(|model| &model.selected)
    }

    pub(crate) fn folder_negation_for_filter(
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
    pub(crate) fn folder_filter_accepts(
        &self,
        relative_path: &Path,
    ) -> bool {
        let selection = self.folder_selection_for_filter();
        let negated = self.folder_negation_for_filter();
        folder_filter_accepts(relative_path, selection, negated)
    }
}

pub(crate) fn folder_filter_accepts(
    relative_path: &Path,
    selection: Option<&BTreeSet<PathBuf>>,
    negated: Option<&BTreeSet<PathBuf>>,
) -> bool {
    let has_selection = selection.is_some_and(|set| !set.is_empty());
    if has_selection {
        let selection = selection.expect("checked above");
        let root_selected = selection.contains(Path::new(""));
        let selected = if root_selected {
            let in_root = relative_path
                .parent()
                .unwrap_or(Path::new(""))
                .as_os_str()
                .is_empty();
            let in_selected_folder = selection
                .iter()
                .filter(|folder| !folder.as_os_str().is_empty())
                .any(|folder| relative_path.starts_with(folder));
            in_root || in_selected_folder
        } else {
            selection
                .iter()
                .filter(|folder| !folder.as_os_str().is_empty())
                .any(|folder| relative_path.starts_with(folder))
        };
        if !selected {
            return false;
        }
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
