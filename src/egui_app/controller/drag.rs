use super::*;
use crate::egui_app::state::DragPayload;
use egui::Pos2;

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
    pub fn update_active_drag(
        &mut self,
        pos: Pos2,
        hovering_collection: Option<CollectionId>,
        hovering_drop_zone: bool,
        hovering_triage: Option<TriageFlagColumn>,
    ) {
        if self.ui.drag.payload.is_none() {
            return;
        }
        self.ui.drag.position = Some(pos);
        self.ui.drag.hovering_collection = hovering_collection;
        self.ui.drag.hovering_drop_zone = hovering_drop_zone;
        self.ui.drag.hovering_browser = hovering_triage;
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
        let triage_target = self.ui.drag.hovering_browser;
        let collection_target = match &payload {
            DragPayload::Sample { .. } => self
                .ui
                .drag
                .hovering_collection
                .clone()
                .or_else(|| {
                    if self.ui.drag.hovering_drop_zone {
                        self.current_collection_id()
                    } else {
                        None
                    }
                })
                .or_else(|| self.current_collection_id()),
            DragPayload::Selection { .. } => self
                .ui
                .drag
                .hovering_collection
                .clone()
                .or_else(|| {
                    if self.ui.drag.hovering_drop_zone {
                        self.current_collection_id()
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    if triage_target.is_none() {
                        self.current_collection_id()
                    } else {
                        None
                    }
                }),
        };
        self.reset_drag();
        match payload {
            DragPayload::Sample {
                source_id,
                relative_path,
            } => {
                self.handle_sample_drop(source_id, relative_path, collection_target, triage_target);
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
        self.ui.drag.hovering_collection = None;
        self.ui.drag.hovering_drop_zone = false;
        self.ui.drag.hovering_browser = None;
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
        self.ui.drag.hovering_collection = None;
        self.ui.drag.hovering_drop_zone = false;
        self.ui.drag.hovering_browser = None;
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
