use super::super::DragDropController;
use crate::egui_app::state::TriageFlagColumn;
use crate::egui_app::ui::style::StatusTone;
use crate::sample_sources::{CollectionId, SampleTag, SourceId};
use crate::selection::SelectionRange;
use std::path::{Path, PathBuf};

impl DragDropController<'_> {
    pub(crate) fn handle_selection_drop(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        bounds: SelectionRange,
        collection_target: Option<CollectionId>,
        triage_target: Option<TriageFlagColumn>,
        folder_target: Option<PathBuf>,
        keep_source_focused: bool,
    ) {
        if collection_target.is_none() && triage_target.is_none() && folder_target.is_none() {
            self.set_status(
                "Drag the selection onto Samples or a collection to save it",
                StatusTone::Warning,
            );
            return;
        }
        let target_tag = triage_target.map(|column| match column {
            TriageFlagColumn::Trash => SampleTag::Trash,
            TriageFlagColumn::Neutral => SampleTag::Neutral,
            TriageFlagColumn::Keep => SampleTag::Keep,
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
        if let Some(collection_id) = collection_target {
            self.handle_selection_drop_to_collection(
                &source_id,
                &relative_path,
                bounds,
                target_tag,
                &collection_id,
            );
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
        target_tag: Option<SampleTag>,
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

    fn handle_selection_drop_to_collection(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
        bounds: SelectionRange,
        target_tag: Option<SampleTag>,
        collection_id: &CollectionId,
    ) {
        let clip_root = match self.selection_clip_root_for_collection(collection_id) {
            Ok(root) => root,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        let clip_name_hint = relative_path
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("selection.wav"));
        match self.export_selection_clip_to_root(
            source_id,
            relative_path,
            bounds,
            target_tag,
            &clip_root,
            &clip_name_hint,
        ) {
            Ok(entry) => {
                self.selection_state.ctx.selected_collection = Some(collection_id.clone());
                let clip_relative = entry.relative_path.clone();
                if let Err(err) =
                    self.add_clip_to_collection(collection_id, clip_root, clip_relative)
                {
                    self.set_status(err, StatusTone::Error);
                    return;
                }
                let name = self
                    .library
                    .collections
                    .iter()
                    .find(|c| c.id == *collection_id)
                    .map(|c| c.name.as_str())
                    .unwrap_or("collection");
                let status = format!("Saved clip {} to {}", entry.relative_path.display(), name);
                self.set_status(status, StatusTone::Info);
            }
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }
}
