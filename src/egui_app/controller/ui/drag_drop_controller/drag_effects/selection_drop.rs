use super::super::DragDropController;
use crate::egui_app::state::TriageFlagColumn;
use crate::egui_app::ui::style::StatusTone;
use crate::sample_sources::{Rating, SourceId};
use crate::selection::SelectionRange;
use std::path::{Path, PathBuf};

impl DragDropController<'_> {
    pub(crate) fn handle_selection_drop(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        bounds: SelectionRange,
        triage_target: Option<TriageFlagColumn>,
        folder_target: Option<PathBuf>,
        keep_source_focused: bool,
    ) {
        if triage_target.is_none() && folder_target.is_none() {
            self.set_status(
                "Drag the selection onto Samples or a folder to save it",
                StatusTone::Warning,
            );
            return;
        }
        let target_tag = triage_target.map(|column| match column {
            TriageFlagColumn::Trash => Rating::TRASH_1,
            TriageFlagColumn::Neutral => Rating::NEUTRAL,
            TriageFlagColumn::Keep => Rating::KEEP_1,
        });
        if let Some(folder) = folder_target.as_deref()
            && !folder.as_os_str().is_empty()
        {
            self.handle_selection_drop_to_folder(
                &source_id,
                &relative_path,
                bounds,
                folder,
                keep_source_focused,
            );
            return;
        }
        if triage_target.is_some() {
            self.handle_selection_drop_to_browser(
                &source_id,
                &relative_path,
                bounds,
                target_tag,
                keep_source_focused,
            );
            return;
        }
    }

    fn handle_selection_drop_to_folder(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
        bounds: SelectionRange,
        folder: &Path,
        keep_source_focused: bool,
    ) {
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|s| &s.id == source_id)
            .cloned()
        else {
            self.set_status(
                "Source not available for selection export",
                StatusTone::Error,
            );
            return;
        };
        if self
            .selection_state
            .ctx
            .selected_source
            .as_ref()
            .is_some_and(|selected| selected != &source.id)
        {
            self.set_status(
                "Switch to the sample's source before saving into its folders",
                StatusTone::Warning,
            );
            return;
        }
        let destination = source.root.join(folder);
        if !destination.is_dir() {
            self.set_status(
                format!("Folder not found: {}", folder.display()),
                StatusTone::Error,
            );
            return;
        }
        match self.export_selection_clip_in_folder(
            source_id,
            relative_path,
            bounds,
            None,
            true,
            true,
            folder,
        ) {
            Ok(entry) => {
                if !keep_source_focused {
                    self.ui.browser.autoscroll = true;
                    self.selection_state.suppress_autoplay_once = true;
                    self.select_from_browser(&entry.relative_path);
                }
                self.set_status(
                    format!("Saved clip {}", entry.relative_path.display()),
                    StatusTone::Info,
                );
            }
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }

    fn handle_selection_drop_to_browser(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
        bounds: SelectionRange,
        target_tag: Option<Rating>,
        keep_source_focused: bool,
    ) {
        let folder_override = self
            .selection_state
            .ctx
            .selected_source
            .as_ref()
            .is_some_and(|selected| selected == source_id)
            .then(|| {
                self.ui.sources.folders.focused.and_then(|idx| {
                    self.ui
                        .sources
                        .folders
                        .rows
                        .get(idx)
                        .map(|row| row.path.clone())
                })
            })
            .flatten()
            .filter(|path| !path.as_os_str().is_empty());
        let export = if let Some(folder) = folder_override.as_deref() {
            self.export_selection_clip_in_folder(
                source_id,
                relative_path,
                bounds,
                target_tag,
                true,
                true,
                folder,
            )
        } else {
            self.export_selection_clip(source_id, relative_path, bounds, target_tag, true, true)
        };
        match export {
            Ok(entry) => {
                if !keep_source_focused {
                    self.ui.browser.autoscroll = true;
                    self.selection_state.suppress_autoplay_once = true;
                    self.select_from_browser(&entry.relative_path);
                }
                let status = format!("Saved clip {}", entry.relative_path.display());
                self.set_status(status, StatusTone::Info);
            }
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }

}
