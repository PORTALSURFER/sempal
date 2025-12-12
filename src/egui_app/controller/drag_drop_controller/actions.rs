use super::*;

pub(crate) trait DragDropActions {
    fn start_sample_drag(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        label: String,
        pos: Pos2,
    );
    fn start_selection_drag_payload(&mut self, bounds: SelectionRange, pos: Pos2);
    fn update_active_drag(&mut self, pos: Pos2, source: DragSource, target: DragTarget);
    fn refresh_drag_position(&mut self, pos: Pos2);
    fn finish_active_drag(&mut self);
    #[cfg(target_os = "windows")]
    fn maybe_launch_external_drag(&mut self, pointer_outside: bool);
    #[cfg(not(target_os = "windows"))]
    fn maybe_launch_external_drag(&mut self, pointer_outside: bool);
}

impl DragDropActions for DragDropController<'_> {
    fn start_sample_drag(
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

    fn start_selection_drag_payload(&mut self, bounds: SelectionRange, pos: Pos2) {
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

    fn update_active_drag(&mut self, pos: Pos2, source: DragSource, target: DragTarget) {
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

    fn refresh_drag_position(&mut self, pos: Pos2) {
        if self.ui.drag.payload.is_some() {
            self.ui.drag.position = Some(pos);
        }
    }

    fn finish_active_drag(&mut self) {
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

        let is_sample_payload = matches!(payload, DragPayload::Sample { .. });
        if is_sample_payload && over_folder_panel && folder_target.is_none() {
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

    #[cfg(target_os = "windows")]
    fn maybe_launch_external_drag(&mut self, pointer_outside: bool) {
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
    fn maybe_launch_external_drag(&mut self, _pointer_outside: bool) {}
}
