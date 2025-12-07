use super::*;
use crate::egui_app::state::DragPayload;
use egui::Pos2;

impl EguiController {
    /// Start tracking a drag for a sample.
    pub fn start_sample_drag(&mut self, path: PathBuf, label: String, pos: Pos2) {
        self.begin_drag(DragPayload::Sample { path }, label, pos);
    }

    /// Start tracking a drag for the current selection payload.
    pub fn start_selection_drag_payload(&mut self, bounds: SelectionRange, pos: Pos2) {
        if bounds.width() < MIN_SELECTION_WIDTH {
            return;
        }
        let Some(audio) = self.loaded_audio.clone() else {
            self.set_status("Load a sample before dragging a selection", StatusTone::Warning);
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
        hovering_triage: Option<TriageColumn>,
    ) {
        if self.ui.drag.payload.is_none() {
            return;
        }
        self.ui.drag.position = Some(pos);
        self.ui.drag.hovering_collection = hovering_collection;
        self.ui.drag.hovering_drop_zone = hovering_drop_zone;
        self.ui.drag.hovering_triage = hovering_triage;
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
        let collection_target = self
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
            .or_else(|| self.current_collection_id());
        let triage_target = self.ui.drag.hovering_triage;
        self.reset_drag();
        match payload {
            DragPayload::Sample { path } => {
                self.handle_sample_drop(path, collection_target, triage_target);
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
        self.ui.drag.hovering_triage = None;
    }

    fn begin_drag(&mut self, payload: DragPayload, label: String, pos: Pos2) {
        self.ui.drag.payload = Some(payload);
        self.ui.drag.label = label;
        self.ui.drag.position = Some(pos);
        self.ui.drag.hovering_collection = None;
        self.ui.drag.hovering_drop_zone = false;
        self.ui.drag.hovering_triage = None;
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

    fn handle_sample_drop(
        &mut self,
        path: PathBuf,
        collection_target: Option<CollectionId>,
        triage_target: Option<TriageColumn>,
    ) {
        if let Some(collection_id) = collection_target {
            if let Err(err) = self.add_sample_to_collection(&collection_id, &path) {
                self.set_status(err, StatusTone::Error);
            }
            return;
        }
        if let Some(column) = triage_target {
            self.suppress_autoplay_once = true;
            let _ = self.set_sample_tag(&path, column);
        }
    }

    fn handle_selection_drop(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        bounds: SelectionRange,
        collection_target: Option<CollectionId>,
        triage_target: Option<TriageColumn>,
    ) {
        if collection_target.is_none() && triage_target.is_none() {
            self.set_status(
                "Drag the selection onto Samples or a collection to save it",
                StatusTone::Warning,
            );
            return;
        }
        let target_tag = triage_target.map(|column| match column {
            TriageColumn::Trash => SampleTag::Trash,
            TriageColumn::Neutral => SampleTag::Neutral,
            TriageColumn::Keep => SampleTag::Keep,
        });
        match self.export_selection_clip(&source_id, &relative_path, bounds, target_tag) {
            Ok(entry) => {
                if let Some(collection_id) = collection_target.as_ref() {
                    self.selected_collection = Some(collection_id.clone());
                    if let Some(source) = self.sources.iter().find(|s| s.id == source_id).cloned() {
                        if let Err(err) = self.add_sample_to_collection_for_source(
                            collection_id,
                            &source,
                            &entry.relative_path,
                        ) {
                            self.set_status(err, StatusTone::Error);
                        }
                    } else {
                        self.set_status("Source not available for collection", StatusTone::Error);
                    }
                }
                self.ui.triage.autoscroll = true;
                self.suppress_autoplay_once = true;
                self.select_wav_by_path(&entry.relative_path);
                let status = if let Some(collection_id) = collection_target.as_ref() {
                    let name = self
                        .collections
                        .iter()
                        .find(|c| c.id == *collection_id)
                        .map(|c| c.name.as_str())
                        .unwrap_or("collection");
                    format!(
                        "Saved clip {} and added to {}",
                        entry.relative_path.display(),
                        name
                    )
                } else {
                    format!("Saved clip {}", entry.relative_path.display())
                };
                self.set_status(status, StatusTone::Info);
            }
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }
}
