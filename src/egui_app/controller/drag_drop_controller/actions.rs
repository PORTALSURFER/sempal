use super::*;
#[cfg(any(target_os = "windows", test))]
use std::time::{Duration, Instant};

pub(crate) trait DragDropActions {
    fn start_sample_drag(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        label: String,
        pos: Pos2,
    );
    fn start_samples_drag(&mut self, samples: Vec<DragSample>, label: String, pos: Pos2);
    fn start_selection_drag_payload(
        &mut self,
        bounds: SelectionRange,
        pos: Pos2,
        keep_source_focused: bool,
    );
    fn update_active_drag(
        &mut self,
        pos: Pos2,
        source: DragSource,
        target: DragTarget,
        shift_down: bool,
    );
    fn refresh_drag_position(&mut self, pos: Pos2, shift_down: bool);
    fn finish_active_drag(&mut self);
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

    fn start_samples_drag(&mut self, samples: Vec<DragSample>, label: String, pos: Pos2) {
        self.begin_drag(DragPayload::Samples { samples }, label, pos);
    }

    fn start_selection_drag_payload(
        &mut self,
        bounds: SelectionRange,
        pos: Pos2,
        keep_source_focused: bool,
    ) {
        if bounds.width() < MIN_SELECTION_WIDTH {
            return;
        }
        let Some(audio) = self.sample_view.wav.loaded_audio.clone() else {
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
            keep_source_focused,
        };
        let label = self.selection_drag_label(&audio, bounds);
        self.begin_drag(payload, label, pos);
    }

    fn update_active_drag(
        &mut self,
        pos: Pos2,
        source: DragSource,
        target: DragTarget,
        shift_down: bool,
    ) {
        if self.ui.drag.payload.is_none() {
            return;
        }
        if self.ui.drag.pointer_left_window {
            return;
        }
        debug!(
            "update_active_drag: pos={:?} source={:?} target={:?}",
            pos, source, target
        );
        self.ui.drag.position = Some(pos);
        self.ui.drag.set_target(source, target);
        if let Some(DragPayload::Selection {
            keep_source_focused,
            ..
        }) = self.ui.drag.payload.as_mut()
        {
            *keep_source_focused = shift_down;
        }
    }

    fn refresh_drag_position(&mut self, pos: Pos2, shift_down: bool) {
        if self.ui.drag.payload.is_some() {
            if self.ui.drag.pointer_left_window {
                return;
            }
            self.ui.drag.position = Some(pos);
            if let Some(DragPayload::Selection {
                keep_source_focused,
                ..
            }) = self.ui.drag.payload.as_mut()
            {
                *keep_source_focused = shift_down;
            }
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

        let is_sample_payload = matches!(
            payload,
            DragPayload::Sample { .. } | DragPayload::Samples { .. }
        );
        if is_sample_payload && over_folder_panel && folder_target.is_none() {
            self.reset_drag();
            self.set_status("Drop onto a folder to move the sample", StatusTone::Warning);
            return;
        }

        let current_collection_id = self.current_collection_id();

        if matches!(payload, DragPayload::Sample { .. } | DragPayload::Samples { .. })
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
            _ => None,
        };

        let drop_in_collections_panel =
            matches!(active_target, DragTarget::CollectionsDropZone { .. })
                || matches!(active_target, DragTarget::CollectionsRow(_))
                || (self.library.collections.is_empty()
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

        if matches!(payload, DragPayload::Sample { .. } | DragPayload::Samples { .. })
            && drop_in_collections_panel
            && current_collection_id.is_none()
            && collection_target.is_none()
            && folder_target.is_none()
            && triage_target.is_none()
        {
            debug!(
                "Blocked collection drop (no active collection): target={:?} collections_empty={} payload={:?}",
                active_target,
                self.library.collections.is_empty(),
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
            DragPayload::Samples { samples } => {
                if let Some(folder) = folder_target {
                    self.handle_samples_drop_to_folder(&samples, &folder);
                } else {
                    self.handle_samples_drop(&samples, collection_target, triage_target);
                }
            }
            DragPayload::Selection {
                source_id,
                relative_path,
                bounds,
                keep_source_focused,
            } => {
                if collection_target.is_none() && triage_target.is_none() && folder_target.is_none()
                {
                    return;
                }
                self.handle_selection_drop(
                    source_id,
                    relative_path,
                    bounds,
                    collection_target,
                    triage_target,
                    folder_target,
                    keep_source_focused,
                );
            }
        }
    }
}

impl DragDropController<'_> {
    #[cfg(any(target_os = "windows", test))]
    const EXTERNAL_DRAG_ARM_WINDOW: Duration = Duration::from_millis(250);

    #[cfg(any(target_os = "windows", test))]
    pub(super) fn should_launch_external_drag(
        &mut self,
        pointer_outside: bool,
        pointer_left: bool,
        now: Instant,
    ) -> bool {
        if self.ui.drag.payload.is_none() {
            self.ui.drag.external_arm_at = None;
            return false;
        }
        if !(pointer_outside || pointer_left) {
            self.ui.drag.external_arm_at = None;
            return false;
        }
        let Some(armed_at) = self.ui.drag.external_arm_at else {
            self.ui.drag.external_arm_at = Some(now);
            return false;
        };
        now.duration_since(armed_at) >= Self::EXTERNAL_DRAG_ARM_WINDOW
    }

    #[cfg(target_os = "windows")]
    pub(super) fn maybe_launch_external_drag(&mut self, pointer_outside: bool, pointer_left: bool) {
        if self.ui.drag.external_started {
            return;
        }
        if !self.should_launch_external_drag(pointer_outside, pointer_left, Instant::now()) {
            return;
        }
        self.ui.drag.external_started = true;
        let payload = self.ui.drag.payload.clone();
        let status = match payload {
            Some(DragPayload::Sample {
                source_id,
                relative_path,
            }) => {
                let absolute = self.sample_absolute_path(&source_id, &relative_path);
                self.start_external_drag(&[absolute])
                    .map(|_| format!("Drag {} to an external target", relative_path.display()))
            }
            Some(DragPayload::Samples { samples }) => {
                let absolutes: Vec<PathBuf> = samples
                    .iter()
                    .map(|sample| self.sample_absolute_path(&sample.source_id, &sample.relative_path))
                    .collect();
                self.start_external_drag(&absolutes).map(|_| {
                    format!(
                        "Drag {} samples to an external target",
                        samples.len()
                    )
                })
            }
            Some(DragPayload::Selection { bounds, .. }) => self
                .export_selection_for_drag(bounds)
                .and_then(|(absolute, label)| {
                    self.start_external_drag(&[absolute])?;
                    Ok(label)
                }),
            None => return,
        };
        match status {
            Ok(message) => {
                self.reset_drag();
                self.set_status(message, StatusTone::Info);
            }
            Err(err) => {
                self.reset_drag();
                self.set_status(err, StatusTone::Error);
            }
        }
    }
}

#[cfg(test)]
mod external_drag_tests {
    use super::*;

    #[test]
    fn external_drag_arms_and_resets_when_pointer_returns() {
        let renderer = WaveformRenderer::new(12, 12);
        let mut controller = EguiController::new(renderer, None);
        let mut drag = DragDropController::new(&mut controller);
        drag.ui.drag.payload = Some(DragPayload::Sample {
            source_id: SourceId::new(),
            relative_path: PathBuf::from("one.wav"),
        });

        let start = Instant::now();
        assert!(!drag.should_launch_external_drag(true, false, start));
        assert!(drag.ui.drag.external_arm_at.is_some());

        assert!(!drag.should_launch_external_drag(false, false, start));
        assert!(drag.ui.drag.external_arm_at.is_none());
    }

    #[test]
    fn external_drag_requires_outside_dwell_time() {
        let renderer = WaveformRenderer::new(12, 12);
        let mut controller = EguiController::new(renderer, None);
        let mut drag = DragDropController::new(&mut controller);
        drag.ui.drag.payload = Some(DragPayload::Sample {
            source_id: SourceId::new(),
            relative_path: PathBuf::from("one.wav"),
        });

        let start = Instant::now();
        assert!(!drag.should_launch_external_drag(true, false, start));
        assert!(!drag.should_launch_external_drag(
            true,
            false,
            start + DragDropController::EXTERNAL_DRAG_ARM_WINDOW - Duration::from_millis(1)
        ));
        assert!(drag.should_launch_external_drag(
            true,
            false,
            start + DragDropController::EXTERNAL_DRAG_ARM_WINDOW
        ));
    }

    #[test]
    fn external_drag_arms_on_pointer_gone_then_launches_after_dwell_time() {
        let renderer = WaveformRenderer::new(12, 12);
        let mut controller = EguiController::new(renderer, None);
        let mut drag = DragDropController::new(&mut controller);
        drag.ui.drag.payload = Some(DragPayload::Sample {
            source_id: SourceId::new(),
            relative_path: PathBuf::from("one.wav"),
        });

        let start = Instant::now();
        assert!(!drag.should_launch_external_drag(false, true, start));
        assert!(drag.ui.drag.external_arm_at.is_some());

        assert!(drag.should_launch_external_drag(
            true,
            false,
            start + DragDropController::EXTERNAL_DRAG_ARM_WINDOW
        ));
    }
}
