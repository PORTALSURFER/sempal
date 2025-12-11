use super::*;
use crate::egui_app::controller::collection_items_helpers::file_metadata;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget};
use egui::Pos2;
use tracing::{debug, info, warn};

impl EguiController {
    /// Start tracking a drag for a sample.
    pub fn start_sample_drag(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        label: String,
        pos: Pos2,
    ) {
        self.begin_drag(
            DragPayload::Sample {
                source_id,
                relative_path,
            },
            label,
            pos,
        );
    }

    /// Start tracking a drag for the current selection payload.
    pub fn start_selection_drag_payload(&mut self, bounds: SelectionRange, pos: Pos2) {
        if bounds.width() < MIN_SELECTION_WIDTH {
            return;
        }
        let Some(audio) = self.loaded_audio.clone() else {
            self.set_status(
                "Load a sample before dragging a selection",
                StatusTone::Warning,
            );
            return;
        };
        let payload = DragPayload::Selection {
            source_id: audio.source_id.clone(),
            relative_path: audio.relative_path.clone(),
            bounds,
        };
        let label = self.selection_drag_label(&audio, bounds);
        self.begin_drag(payload, label, pos);
    }

    /// Update drag position and hover state.
    pub fn update_active_drag(&mut self, pos: Pos2, source: DragSource, target: DragTarget) {
        if self.ui.drag.payload.is_none() {
            return;
        }
        debug!(
            "update_active_drag: pos={:?} source={:?} target={:?}",
            pos, source, target
        );
        self.ui.drag.position = Some(pos);
        self.ui.drag.set_target(source, target);
    }

    /// Refresh drag cursor position when payload is active, without touching hover targets.
    pub fn refresh_drag_position(&mut self, pos: Pos2) {
        if self.ui.drag.payload.is_some() {
            self.ui.drag.position = Some(pos);
        }
    }

    /// Finish drag and perform drop if applicable.
    pub fn finish_active_drag(&mut self) {
        let payload = match self.ui.drag.payload.take() {
            Some(payload) => payload,

            None => {
                self.reset_drag();

                return;
            }
        };

        let active_target = self.ui.drag.active_target.clone();

        info!(
            "Finish drag payload={:?} active_target={:?} last_folder_target={:?}",
            payload, active_target, self.ui.drag.last_folder_target
        );

        let (triage_target, folder_target, over_folder_panel) = match &active_target {
            DragTarget::BrowserTriage(column) => (Some(*column), None, false),

            DragTarget::FolderPanel { folder } => {
                let target = folder
                    .clone()
                    .or_else(|| self.ui.drag.last_folder_target.clone());

                (None, target, true)
            }

            _ => (None, None, false),
        };

        if over_folder_panel && folder_target.is_none() {
            self.reset_drag();

            self.set_status("Drop onto a folder to move the sample", StatusTone::Warning);

            return;
        }

        let current_collection_id = self.current_collection_id();

        if matches!(payload, DragPayload::Sample { .. })
            && matches!(active_target, DragTarget::CollectionsDropZone { .. })
            && current_collection_id.is_none()
        {
            self.reset_drag();
            self.set_status(
                "Create or select a collection before dropping samples",
                StatusTone::Warning,
            );
            return;
        }
        let collection_target = match (&payload, &active_target) {
            (_, DragTarget::CollectionsRow(id)) => Some(id.clone()),

            (_, DragTarget::CollectionsDropZone { collection_id }) => collection_id
                .clone()
                .or_else(|| current_collection_id.clone()),

            (DragPayload::Selection { .. }, _)
                if triage_target.is_none() && folder_target.is_none() =>
            {
                current_collection_id.clone()
            }

            _ => None,
        };

        let drop_in_collections_panel =
            matches!(active_target, DragTarget::CollectionsDropZone { .. })
                || matches!(active_target, DragTarget::CollectionsRow(_))
                || (self.collections.is_empty()
                    && triage_target.is_none()
                    && folder_target.is_none());

        debug!(
            "Collection drop context: drop_in_panel={} current_collection_id={:?} collection_target={:?} folder_target={:?} triage_target={:?} pointer_at={:?}",
            drop_in_collections_panel,
            current_collection_id,
            collection_target,
            folder_target,
            triage_target,
            self.ui.drag.position,
        );

        if matches!(payload, DragPayload::Sample { .. })
            && drop_in_collections_panel
            && current_collection_id.is_none()
            && collection_target.is_none()
            && folder_target.is_none()
            && triage_target.is_none()
        {
            debug!(
                "Blocked collection drop (no active collection): target={:?} collections_empty={} payload={:?}",
                active_target,
                self.collections.is_empty(),
                payload,
            );

            self.reset_drag();

            self.set_status(
                "Create or select a collection before dropping samples",
                StatusTone::Warning,
            );

            return;
        }

        self.reset_drag();

        match payload {
            DragPayload::Sample {
                source_id,

                relative_path,
            } => {
                if let Some(folder) = folder_target {
                    self.handle_sample_drop_to_folder(source_id, relative_path, &folder);
                } else {
                    self.handle_sample_drop(
                        source_id,
                        relative_path,
                        collection_target,
                        triage_target,
                    );
                }
            }

            DragPayload::Selection {
                source_id,

                relative_path,

                bounds,
            } => {
                self.handle_selection_drop(
                    source_id,
                    relative_path,
                    bounds,
                    collection_target,
                    triage_target,
                );
            }
        }
    }

    pub(super) fn reset_drag(&mut self) {
        self.ui.drag.payload = None;
        self.ui.drag.label.clear();
        self.ui.drag.position = None;
        self.ui.drag.clear_all_targets();
        self.ui.drag.last_folder_target = None;
        self.ui.drag.external_started = false;
    }

    #[cfg(target_os = "windows")]
    fn start_external_drag(&self, paths: &[PathBuf]) -> Result<(), String> {
        let hwnd = self
            .drag_hwnd
            .ok_or_else(|| "Window handle unavailable for external drag".to_string())?;
        crate::external_drag::start_file_drag(hwnd, paths)
    }

    #[cfg(not(target_os = "windows"))]
    #[allow(dead_code)]
    fn start_external_drag(&self, _paths: &[PathBuf]) -> Result<(), String> {
        Err("External drag-out is only supported on Windows in this build".into())
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    fn sample_absolute_path(&self, source_id: &SourceId, relative_path: &Path) -> PathBuf {
        self.sources
            .iter()
            .find(|s| &s.id == source_id)
            .map(|source| source.root.join(relative_path))
            .unwrap_or_else(|| relative_path.to_path_buf())
    }

    #[cfg(target_os = "windows")]
    pub fn maybe_launch_external_drag(&mut self, pointer_outside: bool) {
        if !pointer_outside || self.ui.drag.payload.is_none() || self.ui.drag.external_started {
            return;
        }
        let payload = self.ui.drag.payload.clone();
        self.ui
            .drag
            .set_target(DragSource::External, DragTarget::External);
        self.ui.drag.external_started = true;
        let status = match payload {
            Some(DragPayload::Sample {
                source_id,
                relative_path,
            }) => {
                let absolute = self.sample_absolute_path(&source_id, &relative_path);
                self.start_external_drag(&[absolute])
                    .map(|_| format!("Drag {} to an external target", relative_path.display()))
            }
            Some(DragPayload::Selection { bounds, .. }) => self
                .export_selection_for_drag(bounds)
                .and_then(|(absolute, label)| {
                    self.start_external_drag(&[absolute])?;
                    Ok(label)
                }),
            None => return,
        };
        self.reset_drag();
        match status {
            Ok(message) => self.set_status(message, StatusTone::Info),
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn maybe_launch_external_drag(&mut self, _pointer_outside: bool) {}

    fn begin_drag(&mut self, payload: DragPayload, label: String, pos: Pos2) {
        self.ui.drag.payload = Some(payload);
        self.ui.drag.label = label;
        self.ui.drag.position = Some(pos);
        self.ui.drag.clear_all_targets();
        self.ui.drag.last_folder_target = None;
        self.ui.drag.external_started = false;
    }

    fn selection_drag_label(&self, audio: &LoadedAudio, bounds: SelectionRange) -> String {
        let name = audio
            .relative_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Selection");
        let seconds = (audio.duration_seconds * bounds.width()).max(0.0);
        format!("{name} ({seconds:.2}s)")
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    fn export_selection_for_drag(
        &mut self,
        bounds: SelectionRange,
    ) -> Result<(PathBuf, String), String> {
        let audio = self
            .loaded_audio
            .as_ref()
            .ok_or_else(|| "Load a sample before dragging a selection".to_string())?;
        let clip = self.selection_audio(&audio.source_id, &audio.relative_path)?;
        let entry =
            self.export_selection_clip(&clip.source_id, &clip.relative_path, bounds, None, true)?;
        let source = self
            .sources
            .iter()
            .find(|s| s.id == clip.source_id)
            .cloned()
            .ok_or_else(|| "Source not available for selection export".to_string())?;
        let absolute = source.root.join(&entry.relative_path);
        let label = format!(
            "Drag {} to an external target",
            entry.relative_path.display()
        );
        Ok((absolute, label))
    }

    fn handle_sample_drop(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        collection_target: Option<CollectionId>,
        triage_target: Option<TriageFlagColumn>,
    ) {
        if let Some(collection_id) = collection_target {
            if let Some(source) = self.sources.iter().find(|s| s.id == source_id).cloned() {
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
            self.suppress_autoplay_once = true;
            let target_tag = match column {
                TriageFlagColumn::Trash => SampleTag::Trash,
                TriageFlagColumn::Neutral => SampleTag::Neutral,
                TriageFlagColumn::Keep => SampleTag::Keep,
            };
            if let Some(source) = self.sources.iter().find(|s| s.id == source_id).cloned() {
                let _ = self.set_sample_tag_for_source(&source, &relative_path, target_tag, false);
            } else {
                let _ = self.set_sample_tag(&relative_path, column);
            }
        }
    }

    fn handle_sample_drop_to_folder(
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
        let Some(source) = self.sources.iter().find(|s| s.id == source_id).cloned() else {
            warn!("Folder move: missing source {:?}", source_id);
            self.set_status("Source not available for move", StatusTone::Error);
            return;
        };
        if self
            .selected_source
            .as_ref()
            .is_some_and(|selected| selected != &source.id)
        {
            warn!(
                "Folder move blocked: selected source {:?} differs from sample source {:?}",
                self.selected_source, source.id
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

    fn handle_selection_drop(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        bounds: SelectionRange,
        collection_target: Option<CollectionId>,
        triage_target: Option<TriageFlagColumn>,
    ) {
        if collection_target.is_none() && triage_target.is_none() {
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
        if triage_target.is_some() {
            self.handle_selection_drop_to_browser(&source_id, &relative_path, bounds, target_tag);
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

    fn handle_selection_drop_to_browser(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
        bounds: SelectionRange,
        target_tag: Option<SampleTag>,
    ) {
        match self.export_selection_clip(source_id, relative_path, bounds, target_tag, true) {
            Ok(entry) => {
                self.ui.browser.autoscroll = true;
                self.suppress_autoplay_once = true;
                self.select_wav_by_path(&entry.relative_path);
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
        match self.export_selection_clip(source_id, relative_path, bounds, target_tag, false) {
            Ok(entry) => {
                self.selected_collection = Some(collection_id.clone());
                if let Some(source) = self.sources.iter().find(|s| s.id == *source_id).cloned() {
                    if let Err(err) = self.add_sample_to_collection_for_source(
                        collection_id,
                        &source,
                        &entry.relative_path,
                    ) {
                        self.set_status(err, StatusTone::Error);
                        return;
                    }
                } else {
                    self.set_status("Source not available for collection", StatusTone::Error);
                    return;
                }
                let name = self
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
