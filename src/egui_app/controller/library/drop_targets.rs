use super::*;
use crate::egui_app::view_model;
use crate::sample_sources::config::{DropTargetColor, DropTargetConfig};
use std::path::{Path, PathBuf};

/// Resolved drop target mapped to a configured source and relative folder.
#[derive(Clone, Debug)]
pub(crate) struct DropTargetLocation {
    pub(crate) source: SampleSource,
    pub(crate) relative_folder: PathBuf,
}

impl EguiController {
    /// Open a folder picker and add the chosen directory as a drop target.
    pub fn add_drop_target_via_dialog(&mut self) {
        let Some(path) = FileDialog::new().pick_folder() else {
            return;
        };
        if let Err(error) = self.add_drop_target_from_path(path) {
            self.set_status(error, StatusTone::Error);
        }
    }

    /// Add a new drop target from a known folder path.
    pub fn add_drop_target_from_path(&mut self, path: PathBuf) -> Result<(), String> {
        let normalized = crate::sample_sources::config::normalize_path(path.as_path());
        if !normalized.is_dir() {
            return Err("Please select a directory".into());
        }
        if self
            .settings
            .drop_targets
            .iter()
            .any(|existing| existing.path == normalized)
        {
            self.set_status("Drop target already added", StatusTone::Info);
            return Ok(());
        }
        if self.resolve_drop_target_location(&normalized).is_none() {
            return Err("Drop targets must live inside a configured source".into());
        }
        self.settings
            .drop_targets
            .push(DropTargetConfig::new(normalized));
        self.refresh_drop_targets_ui();
        self.persist_config("Failed to save drop targets")?;
        self.set_status("Drop target added", StatusTone::Info);
        Ok(())
    }

    /// Remove a configured drop target by index.
    pub(crate) fn remove_drop_target(&mut self, index: usize) {
        if index >= self.settings.drop_targets.len() {
            return;
        }
        self.settings.drop_targets.remove(index);
        if let Some(selected) = self.ui.sources.drop_targets.selected {
            if selected == index {
                self.ui.sources.drop_targets.selected = None;
            } else if selected > index {
                self.ui.sources.drop_targets.selected = Some(selected - 1);
            }
        }
        self.refresh_drop_targets_ui();
        let _ = self.persist_config("Failed to save drop targets");
        self.set_status("Drop target removed", StatusTone::Info);
    }

    /// Select a configured drop target and show its contents in the sample browser.
    pub(crate) fn select_drop_target_by_index(&mut self, index: usize) {
        let Some(config) = self.settings.drop_targets.get(index).cloned() else {
            return;
        };
        self.ui.sources.drop_targets.selected = Some(index);
        let Some(location) = self.resolve_drop_target_location(&config.path) else {
            self.set_status(
                "Drop target is no longer inside a configured source",
                StatusTone::Warning,
            );
            return;
        };
        let target_dir = location.source.root.join(&location.relative_folder);
        if !target_dir.is_dir() {
            self.set_status(
                format!("Drop target missing: {}", target_dir.display()),
                StatusTone::Warning,
            );
            return;
        }
        self.select_source(Some(location.source.id.clone()));
        self.focus_drop_target_folder(&location.relative_folder);
        self.focus_folder_context();
    }

    /// Convert a folder drag into a new drop target entry.
    pub(crate) fn handle_folder_drop_to_drop_targets(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
    ) {
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|source| source.id == source_id)
            .cloned()
        else {
            self.set_status("Source not available for drop target", StatusTone::Error);
            return;
        };
        let target_path = source.root.join(&relative_path);
        if !target_path.is_dir() {
            self.set_status("Folder not found for drop target", StatusTone::Error);
            return;
        }
        if let Err(error) = self.add_drop_target_from_path(target_path) {
            self.set_status(error, StatusTone::Error);
        }
    }

    /// Assign a preset color to a drop target entry.
    pub(crate) fn set_drop_target_color(
        &mut self,
        index: usize,
        color: Option<DropTargetColor>,
    ) {
        let Some(target) = self.settings.drop_targets.get_mut(index) else {
            return;
        };
        target.color = color;
        self.refresh_drop_targets_ui();
        let _ = self.persist_config("Failed to save drop target color");
    }

    /// Resolve a drop target path to its source and relative folder.
    pub(crate) fn resolve_drop_target_location(
        &self,
        target_path: &Path,
    ) -> Option<DropTargetLocation> {
        let normalized = crate::sample_sources::config::normalize_path(target_path);
        let mut best: Option<(usize, SampleSource, PathBuf)> = None;
        for source in &self.library.sources {
            if normalized.starts_with(&source.root) {
                let relative = normalized
                    .strip_prefix(&source.root)
                    .unwrap_or_else(|_| Path::new(""))
                    .to_path_buf();
                let weight = source.root.as_os_str().len();
                match best {
                    Some((best_weight, _, _)) if best_weight >= weight => {}
                    _ => {
                        best = Some((weight, source.clone(), relative));
                    }
                }
            }
        }
        best.map(|(_, source, relative_folder)| DropTargetLocation {
            source,
            relative_folder,
        })
    }

    /// Refresh the UI rows for the drop target list.
    pub(crate) fn refresh_drop_targets_ui(&mut self) {
        self.ui.sources.drop_targets.rows = self
            .settings
            .drop_targets
            .iter()
            .map(|config| {
                let missing = !config.path.is_dir()
                    || self.resolve_drop_target_location(&config.path).is_none();
                view_model::drop_target_row(&config.path, config.color, missing)
            })
            .collect();
        let count = self.ui.sources.drop_targets.rows.len();
        self.ui.sources.drop_targets.selected = self
            .ui
            .sources
            .drop_targets
            .selected
            .filter(|idx| *idx < count);
        self.ui.sources.drop_targets.scroll_to = self.ui.sources.drop_targets.selected;
        self.ui.sources.drop_targets.menu_row = None;
    }
}
