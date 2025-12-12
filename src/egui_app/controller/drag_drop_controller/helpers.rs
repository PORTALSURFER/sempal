use super::*;

pub(crate) struct DragDropController<'a> {
    controller: &'a mut EguiController,
}

impl<'a> DragDropController<'a> {
    pub(crate) fn new(controller: &'a mut EguiController) -> Self {
        Self { controller }
    }
}

impl std::ops::Deref for DragDropController<'_> {
    type Target = EguiController;

    fn deref(&self) -> &Self::Target {
        self.controller
    }
}

impl std::ops::DerefMut for DragDropController<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.controller
    }
}

impl DragDropController<'_> {
    pub(super) fn reset_drag(&mut self) {
        self.ui.drag.payload = None;
        self.ui.drag.label.clear();
        self.ui.drag.position = None;
        self.ui.drag.clear_all_targets();
        self.ui.drag.last_folder_target = None;
        self.ui.drag.external_started = false;
    }

    #[cfg(target_os = "windows")]
    pub(super) fn start_external_drag(&self, paths: &[PathBuf]) -> Result<(), String> {
        let hwnd = self
            .drag_hwnd
            .ok_or_else(|| "Window handle unavailable for external drag".to_string())?;
        crate::external_drag::start_file_drag(hwnd, paths)
    }

    #[cfg(not(target_os = "windows"))]
    #[allow(dead_code)]
    pub(super) fn start_external_drag(&self, _paths: &[PathBuf]) -> Result<(), String> {
        Err("External drag-out is only supported on Windows in this build".into())
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(super) fn sample_absolute_path(
        &self,
        source_id: &SourceId,
        relative_path: &Path,
    ) -> PathBuf {
        self.sources
            .iter()
            .find(|s| &s.id == source_id)
            .map(|source| source.root.join(relative_path))
            .unwrap_or_else(|| relative_path.to_path_buf())
    }

    pub(super) fn begin_drag(&mut self, payload: DragPayload, label: String, pos: Pos2) {
        self.ui.drag.payload = Some(payload);
        self.ui.drag.label = label;
        self.ui.drag.position = Some(pos);
        self.ui.drag.clear_all_targets();
        self.ui.drag.last_folder_target = None;
        self.ui.drag.external_started = false;
    }

    pub(super) fn selection_drag_label(
        &self,
        audio: &LoadedAudio,
        bounds: SelectionRange,
    ) -> String {
        let name = audio
            .relative_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Selection");
        let seconds = (audio.duration_seconds * bounds.width()).max(0.0);
        format!("{name} ({seconds:.2}s)")
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(super) fn export_selection_for_drag(
        &mut self,
        bounds: SelectionRange,
    ) -> Result<(PathBuf, String), String> {
        let audio = self
            .loaded_audio
            .as_ref()
            .ok_or_else(|| "Load a sample before dragging a selection".to_string())?;
        let clip = self.selection_audio(&audio.source_id, &audio.relative_path)?;
        let entry = self.export_selection_clip(
            &clip.source_id,
            &clip.relative_path,
            bounds,
            None,
            true,
            true,
        )?;
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

    pub(super) fn handle_sample_drop(
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

    pub(super) fn handle_selection_drop(
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
        match self.export_selection_clip(
            source_id,
            relative_path,
            bounds,
            target_tag,
            true,
            true,
        ) {
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
        match self.export_selection_clip(
            source_id,
            relative_path,
            bounds,
            target_tag,
            false,
            false,
        ) {
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
