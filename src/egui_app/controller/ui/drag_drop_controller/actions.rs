use super::*;
use crate::egui_app::state::{DragSample, DragSource};
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
    fn start_folder_drag(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        label: String,
        pos: Pos2,
    );
    fn start_selection_drag_payload(
        &mut self,
        bounds: SelectionRange,
        pos: Pos2,
        keep_source_focused: bool,
    );
    /// Begin dragging a drop target row to reorder the sidebar list.
    fn start_drop_target_drag(&mut self, path: PathBuf, label: String, pos: Pos2);
    fn update_active_drag(
        &mut self,
        pos: Pos2,
        source: DragSource,
        target: DragTarget,
        shift_down: bool,
        alt_down: bool,
    );
    fn refresh_drag_position(&mut self, pos: Pos2, shift_down: bool, alt_down: bool);
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

    fn start_folder_drag(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        label: String,
        pos: Pos2,
    ) {
        self.begin_drag(
            DragPayload::Folder {
                source_id,
                relative_path,
            },
            label,
            pos,
        );
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

    fn start_drop_target_drag(&mut self, path: PathBuf, label: String, pos: Pos2) {
        self.begin_drag(DragPayload::DropTargetReorder { path }, label, pos);
    }

    fn update_active_drag(
        &mut self,
        pos: Pos2,
        source: DragSource,
        target: DragTarget,
        shift_down: bool,
        alt_down: bool,
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
        self.ui.drag.copy_on_drop = alt_down;
        if self.ui.drag.origin_source.is_none() {
            self.ui.drag.origin_source = Some(source);
        }
        self.ui.drag.set_target(source, target);
        if let Some(DragPayload::Selection {
            keep_source_focused,
            ..
        }) = self.ui.drag.payload.as_mut()
        {
            let _ = shift_down;
            *keep_source_focused = true;
        }
    }

    fn refresh_drag_position(&mut self, pos: Pos2, shift_down: bool, alt_down: bool) {
        if self.ui.drag.payload.is_some() {
            if self.ui.drag.pointer_left_window {
                return;
            }
            self.ui.drag.position = Some(pos);
            self.ui.drag.copy_on_drop = alt_down;
            if let Some(DragPayload::Selection {
                keep_source_focused,
                ..
            }) = self.ui.drag.payload.as_mut()
            {
                let _ = shift_down;
                *keep_source_focused = true;
            }
        }
    }

    fn finish_active_drag(&mut self) {
        let origin_source = self.ui.drag.origin_source;
        let payload = match self.ui.drag.payload.take() {
            Some(payload) => payload,
            None => {
                self.reset_drag();
                return;
            }
        };

        let active_target = self.ui.drag.active_target.clone();
        let copy_requested = self.ui.drag.copy_on_drop;

        info!(
            "Finish drag payload={:?} active_target={:?} last_folder_target={:?}",
            payload, active_target, self.ui.drag.last_folder_target
        );
        debug!(
            "Drag origin_source={:?} active_target={:?} payload={:?}",
            origin_source, active_target, payload
        );

        let source_target = match &active_target {
            DragTarget::SourcesRow(id) => Some(id.clone()),
            _ => None,
        };
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
        let drop_target_path = match &active_target {
            DragTarget::DropTarget { path } => Some(path.clone()),
            _ => None,
        };
        let drop_targets_panel = matches!(active_target, DragTarget::DropTargetsPanel);

        let is_sample_payload = matches!(
            payload,
            DragPayload::Sample { .. } | DragPayload::Samples { .. }
        );
        let is_folder_payload = matches!(payload, DragPayload::Folder { .. });
        if is_sample_payload && over_folder_panel && folder_target.is_none() {
            self.reset_drag();
            self.set_status("Drop onto a folder to move the sample", StatusTone::Warning);
            return;
        }
        if is_folder_payload && over_folder_panel && folder_target.is_none() {
            self.reset_drag();
            self.set_status("Drop onto a folder to move it", StatusTone::Warning);
            return;
        }

        self.reset_drag();

        match payload {
            DragPayload::Sample {
                source_id,
                relative_path,
            } => {
                if origin_source == Some(DragSource::Waveform)
                    && matches!(active_target, DragTarget::BrowserTriage(_))
                {
                    self.handle_waveform_sample_drop_to_browser(source_id, relative_path);
                } else if let Some(target) = source_target {
                    self.handle_sample_drop_to_source(source_id, relative_path, target);
                } else if let Some(target_path) = drop_target_path.clone() {
                    self.handle_sample_drop_to_drop_target(
                        source_id,
                        relative_path,
                        target_path,
                        copy_requested,
                    );
                } else if let Some(folder) = folder_target {
                    self.handle_sample_drop_to_folder(source_id, relative_path, &folder);
                } else if triage_target.is_some() {
                    self.handle_sample_drop(source_id, relative_path, triage_target);
                } else {
                    self.set_status(
                        "Drop onto a triage column or folder to move the sample",
                        StatusTone::Warning,
                    );
                }
            }
            DragPayload::Samples { samples } => {
                if let Some(target) = source_target {
                    self.handle_samples_drop_to_source(&samples, target);
                } else if let Some(target_path) = drop_target_path.clone() {
                    self.handle_samples_drop_to_drop_target(
                        &samples,
                        target_path,
                        copy_requested,
                    );
                } else if let Some(folder) = folder_target {
                    self.handle_samples_drop_to_folder(&samples, &folder);
                } else if triage_target.is_some() {
                    self.handle_samples_drop(&samples, triage_target);
                } else {
                    self.set_status(
                        "Drop onto a triage column or folder to move samples",
                        StatusTone::Warning,
                    );
                }
            }
            DragPayload::Folder {
                source_id,
                relative_path,
            } => {
                if let Some(folder) = folder_target {
                    self.handle_folder_drop_to_folder(source_id, relative_path, &folder);
                } else if drop_targets_panel {
                    self.handle_folder_drop_to_drop_targets(source_id, relative_path);
                } else if drop_target_path.is_some() {
                    self.set_status(
                        "Drop targets accept samples, not folders",
                        StatusTone::Warning,
                    );
                } else {
                    self.set_status("Drop onto a folder to move it", StatusTone::Warning);
                }
            }
            DragPayload::Selection {
                source_id,
                relative_path,
                bounds,
                keep_source_focused,
            } => {
                if source_target.is_some() {
                    self.set_status(
                        "Drop samples onto a source to move them",
                        StatusTone::Warning,
                    );
                    return;
                }
                if drop_target_path.is_some() {
                    self.set_status(
                        "Drop targets accept samples, not selections",
                        StatusTone::Warning,
                    );
                    return;
                }
                if triage_target.is_none() && folder_target.is_none() {
                    return;
                }
                self.handle_selection_drop(
                    source_id,
                    relative_path,
                    bounds,
                    triage_target,
                    folder_target,
                    keep_source_focused,
                );
            }
            DragPayload::DropTargetReorder { path } => {
                let target_path = match active_target {
                    DragTarget::DropTarget { path } => Some(path),
                    DragTarget::DropTargetsPanel => None,
                    _ => return,
                };
                self.reorder_drop_targets(&path, target_path.as_deref());
            }
        }
    }
}

impl DragDropController<'_> {
    #[cfg(any(target_os = "windows", test))]
    const EXTERNAL_DRAG_ARM_WINDOW: Duration = Duration::from_millis(250);

    #[cfg(any(target_os = "windows", test))]
    pub(crate) fn should_launch_external_drag(
        &mut self,
        pointer_outside: bool,
        pointer_left: bool,
        now: Instant,
    ) -> bool {
        let should_consider = matches!(
            self.ui.drag.payload,
            Some(
                DragPayload::Sample { .. }
                    | DragPayload::Samples { .. }
                    | DragPayload::Selection { .. }
            )
        );
        if !should_consider {
            self.ui.drag.external_arm_at = None;
            return false;
        }
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
    pub(crate) fn maybe_launch_external_drag(&mut self, pointer_outside: bool, pointer_left: bool) {
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
                    .map(|sample| {
                        self.sample_absolute_path(&sample.source_id, &sample.relative_path)
                    })
                    .collect();
                self.start_external_drag(&absolutes)
                    .map(|_| format!("Drag {} samples to an external target", samples.len()))
            }
            Some(DragPayload::Selection { bounds, .. }) => self
                .export_selection_for_drag(bounds)
                .and_then(|(absolute, label)| {
                    self.start_external_drag(&[absolute])?;
                    Ok(label)
                }),
            Some(DragPayload::Folder { .. }) => return,
            Some(DragPayload::DropTargetReorder { .. }) => return,
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
