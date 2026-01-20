use super::super::*;
use crate::egui_app::state::RootFolderFilterMode;
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

    /// Read the active root filter mode for the current source.
    pub(crate) fn root_folder_filter_mode_for_filter(&self) -> Option<RootFolderFilterMode> {
        let id = self.selection_state.ctx.selected_source.as_ref()?;
        self.ui_cache
            .folders
            .models
            .get(id)
            .map(|model| model.root_filter_mode)
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
        let root_mode = self
            .root_folder_filter_mode_for_filter()
            .unwrap_or_default();
        folder_filter_accepts(relative_path, selection, negated, root_mode)
    }
}

pub(crate) fn folder_filter_accepts(
    relative_path: &Path,
    selection: Option<&BTreeSet<PathBuf>>,
    negated: Option<&BTreeSet<PathBuf>>,
    root_mode: RootFolderFilterMode,
) -> bool {
    let has_selection = selection.is_some_and(|set| !set.is_empty());
    if has_selection {
        let selection = selection.expect("checked above");
        let root_selected = selection.contains(Path::new(""));
        let selected = if root_selected {
            match root_mode {
                RootFolderFilterMode::AllDescendants => true,
                RootFolderFilterMode::RootOnly => {
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
                }
            }
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

/// Report whether folder-based filtering is active.
pub(crate) fn folder_filters_active(
    selection: Option<&BTreeSet<PathBuf>>,
    negated: Option<&BTreeSet<PathBuf>>,
    root_mode: RootFolderFilterMode,
) -> bool {
    let has_negation = negated.is_some_and(|set| !set.is_empty());
    if has_negation {
        return true;
    }
    let Some(selection) = selection else {
        return false;
    };
    if selection.is_empty() {
        return false;
    }
    let root_selected = selection.contains(Path::new(""));
    if root_selected && root_mode == RootFolderFilterMode::AllDescendants {
        return false;
    }
    true
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
