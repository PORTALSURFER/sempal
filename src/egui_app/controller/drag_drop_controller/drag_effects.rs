use super::{DragDropController, file_metadata};
use crate::egui_app::state::DragSample;
use crate::egui_app::state::TriageFlagColumn;
use crate::egui_app::ui::style::StatusTone;
use crate::sample_sources::{CollectionId, SampleTag, SourceId, WavEntry};
use crate::selection::SelectionRange;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

impl DragDropController<'_> {
    #[cfg(target_os = "windows")]
    pub(super) fn start_external_drag(&self, paths: &[PathBuf]) -> Result<(), String> {
        let hwnd = self
            .drag_hwnd
            .ok_or_else(|| "Window handle unavailable for external drag".to_string())?;
        crate::external_drag::start_file_drag(hwnd, paths)
    }

    pub(super) fn handle_sample_drop(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        collection_target: Option<CollectionId>,
        triage_target: Option<TriageFlagColumn>,
    ) {
        if let Some(collection_id) = collection_target {
            if let Some(source) = self
                .library
                .sources
                .iter()
                .find(|s| s.id == source_id)
                .cloned()
            {
                if let Err(err) = self.add_sample_to_collection_for_source(
                    &collection_id,
                    &source,
                    &relative_path,
                ) {
                    self.set_status(err, StatusTone::Error);
                }
            } else if let Err(err) =
                self.add_sample_to_collection(&collection_id, &relative_path.clone())
            {
                self.set_status(err, StatusTone::Error);
            }
            return;
        }
        if let Some(column) = triage_target {
            self.selection_state.suppress_autoplay_once = true;
            let target_tag = match column {
                TriageFlagColumn::Trash => SampleTag::Trash,
                TriageFlagColumn::Neutral => SampleTag::Neutral,
                TriageFlagColumn::Keep => SampleTag::Keep,
            };
            if let Some(source) = self
                .library
                .sources
                .iter()
                .find(|s| s.id == source_id)
                .cloned()
            {
                let _ = self.set_sample_tag_for_source(&source, &relative_path, target_tag, false);
            } else {
                let _ = self.set_sample_tag(&relative_path, column);
            }
        }
    }

    pub(super) fn handle_samples_drop(
        &mut self,
        samples: &[DragSample],
        collection_target: Option<CollectionId>,
        triage_target: Option<TriageFlagColumn>,
    ) {
        for sample in samples {
            self.handle_sample_drop(
                sample.source_id.clone(),
                sample.relative_path.clone(),
                collection_target.clone(),
                triage_target,
            );
        }
    }

    pub(super) fn handle_waveform_sample_drop_to_browser(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
    ) {
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|s| s.id == source_id)
            .cloned()
        else {
            self.set_status("Source not available", StatusTone::Error);
            return;
        };
        let absolute = source.root.join(&relative_path);
        if !absolute.exists() {
            self.set_status(
                format!("Source file missing: {}", relative_path.display()),
                StatusTone::Error,
            );
            return;
        }
        let parent = relative_path.parent().map(|parent| parent.to_path_buf());
        let stem = relative_path
            .file_stem()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "sample".to_string());
        let extension = relative_path
            .extension()
            .map(|ext| ext.to_string_lossy().to_string());
        let mut copy_relative = None;
        let mut copy_absolute = None;
        for count in 1.. {
            let suffix = format!("{stem}_copy{count:03}");
            let file_name = if let Some(ext) = &extension {
                format!("{suffix}.{ext}")
            } else {
                suffix
            };
            let candidate = if let Some(parent) = parent.as_ref() {
                if parent.as_os_str().is_empty() {
                    PathBuf::from(&file_name)
                } else {
                    parent.join(&file_name)
                }
            } else {
                PathBuf::from(&file_name)
            };
            let candidate_abs = source.root.join(&candidate);
            if !candidate_abs.exists() {
                copy_relative = Some(candidate);
                copy_absolute = Some(candidate_abs);
                break;
            }
        }
        let copy_relative = match copy_relative {
            Some(path) => path,
            None => {
                self.set_status("Unable to find a unique copy name", StatusTone::Error);
                return;
            }
        };
        let copy_absolute = copy_absolute.unwrap();
        if let Some(parent) = copy_relative.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(err) = fs::create_dir_all(source.root.join(parent)) {
                    self.set_status(
                        format!("Failed to create folder for copy: {err}"),
                        StatusTone::Error,
                    );
                    return;
                }
            }
        }
        if let Err(err) = fs::copy(&absolute, &copy_absolute) {
            self.set_status(format!("Failed to copy sample: {err}"), StatusTone::Error);
            return;
        }
        let (file_size, modified_ns) = match file_metadata(&copy_absolute) {
            Ok(meta) => meta,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        let db = match self.database_for(&source) {
            Ok(db) => db,
            Err(err) => {
                self.set_status(
                    format!("Failed to open source DB: {err}"),
                    StatusTone::Error,
                );
                return;
            }
        };
        if let Err(err) = db.upsert_file(&copy_relative, file_size, modified_ns) {
            self.set_status(format!("Failed to register copy: {err}"), StatusTone::Error);
            return;
        }
        let entry = WavEntry {
            relative_path: copy_relative.clone(),
            file_size,
            modified_ns,
            content_hash: None,
            tag: SampleTag::Neutral,
            missing: false,
        };
        self.wav_entries.entries.push(entry);
        self.wav_entries
            .entries
            .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        self.rebuild_wav_lookup();
        self.rebuild_browser_lists();
        self.select_wav_by_path_with_rebuild(&copy_relative, true);
        self.set_status(
            format!("Copied sample to {}", copy_relative.display()),
            StatusTone::Info,
        );
    }

    pub(super) fn handle_sample_drop_to_folder(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        target_folder: &Path,
    ) {
        info!(
            "Folder drop requested: source_id={:?} path={} target={}",
            source_id,
            relative_path.display(),
            target_folder.display()
        );
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|s| s.id == source_id)
            .cloned()
        else {
            warn!("Folder move: missing source {:?}", source_id);
            self.set_status("Source not available for move", StatusTone::Error);
            return;
        };
        if self
            .selection_state
            .ctx
            .selected_source
            .as_ref()
            .is_some_and(|selected| selected != &source.id)
        {
            warn!(
                "Folder move blocked: selected source {:?} differs from sample source {:?}",
                self.selection_state.ctx.selected_source, source.id
            );
            self.set_status(
                "Switch to the sample's source before moving into its folders",
                StatusTone::Warning,
            );
            return;
        }
        let file_name = match relative_path.file_name() {
            Some(name) => name.to_owned(),
            None => {
                warn!(
                    "Folder move aborted: missing file name for {:?}",
                    relative_path
                );
                self.set_status("Sample name unavailable for move", StatusTone::Error);
                return;
            }
        };
        let new_relative = if target_folder.as_os_str().is_empty() {
            PathBuf::from(file_name)
        } else {
            target_folder.join(file_name)
        };
        if new_relative == relative_path {
            info!("Folder move skipped: already in target {:?}", target_folder);
            self.set_status("Sample is already in that folder", StatusTone::Info);
            return;
        }
        let destination_dir = source.root.join(target_folder);
        if !destination_dir.is_dir() {
            self.set_status(
                format!("Folder not found: {}", target_folder.display()),
                StatusTone::Error,
            );
            return;
        }
        let absolute = source.root.join(&relative_path);
        if !absolute.exists() {
            warn!(
                "Folder move aborted: missing source file {}",
                relative_path.display()
            );
            self.set_status(
                format!("File missing: {}", relative_path.display()),
                StatusTone::Error,
            );
            return;
        }
        let target_absolute = source.root.join(&new_relative);
        if target_absolute.exists() {
            warn!(
                "Folder move aborted: target already exists {}",
                new_relative.display()
            );
            self.set_status(
                format!("A file already exists at {}", new_relative.display()),
                StatusTone::Error,
            );
            return;
        }
        let tag = match self.sample_tag_for(&source, &relative_path) {
            Ok(tag) => tag,
            Err(err) => {
                warn!(
                    "Folder move aborted: failed to resolve tag for {}: {}",
                    relative_path.display(),
                    err
                );
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        if let Err(err) = std::fs::rename(&absolute, &target_absolute)
            .map_err(|err| format!("Failed to move file: {err}"))
        {
            warn!(
                "Folder move aborted: rename failed {} -> {} : {}",
                absolute.display(),
                target_absolute.display(),
                err
            );
            self.set_status(err, StatusTone::Error);
            return;
        }
        let (file_size, modified_ns) = match file_metadata(&target_absolute) {
            Ok(meta) => meta,
            Err(err) => {
                let _ = std::fs::rename(&target_absolute, &absolute);
                warn!(
                    "Folder move aborted: metadata failed for {} : {}",
                    target_absolute.display(),
                    err
                );
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        if let Err(err) = self.rewrite_db_entry_for_source(
            &source,
            &relative_path,
            &new_relative,
            file_size,
            modified_ns,
            tag,
        ) {
            let _ = std::fs::rename(&target_absolute, &absolute);
            warn!(
                "Folder move aborted: db rewrite failed {} -> {} : {}",
                relative_path.display(),
                new_relative.display(),
                err
            );
            self.set_status(err, StatusTone::Error);
            return;
        }
        let new_entry = WavEntry {
            relative_path: new_relative.clone(),
            file_size,
            modified_ns,
            content_hash: None,
            tag,
            missing: false,
        };
        self.update_cached_entry(&source, &relative_path, new_entry);
        if self.update_collections_for_rename(&source.id, &relative_path, &new_relative) {
            let _ = self.persist_config("Failed to save collections after move");
        }
        info!(
            "Folder move success: {} -> {}",
            relative_path.display(),
            new_relative.display()
        );
        self.set_status(
            format!("Moved to {}", target_folder.display()),
            StatusTone::Info,
        );
    }

    pub(super) fn handle_samples_drop_to_folder(
        &mut self,
        samples: &[DragSample],
        target_folder: &Path,
    ) {
        for sample in samples {
            self.handle_sample_drop_to_folder(
                sample.source_id.clone(),
                sample.relative_path.clone(),
                target_folder,
            );
        }
    }

    pub(super) fn handle_selection_drop(
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
