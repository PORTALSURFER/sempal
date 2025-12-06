use super::*;
use egui::Pos2;

impl EguiController {
    /// Start tracking a drag for a sample.
    pub fn start_sample_drag(&mut self, path: PathBuf, label: String, pos: Pos2) {
        self.ui.drag.active_path = Some(path);
        self.ui.drag.label = label;
        self.ui.drag.position = Some(pos);
        self.ui.drag.hovering_collection = None;
        self.ui.drag.hovering_drop_zone = false;
        self.ui.drag.hovering_triage = None;
    }

    /// Update drag position and hover state.
    pub fn update_sample_drag(
        &mut self,
        pos: Pos2,
        hovering_collection: Option<CollectionId>,
        hovering_drop_zone: bool,
        hovering_triage: Option<TriageColumn>,
    ) {
        self.ui.drag.position = Some(pos);
        self.ui.drag.hovering_collection = hovering_collection;
        self.ui.drag.hovering_drop_zone = hovering_drop_zone;
        self.ui.drag.hovering_triage = hovering_triage;
    }

    /// Finish drag and perform drop if applicable.
    pub fn finish_sample_drag(&mut self) {
        let path = match self.ui.drag.active_path.take() {
            Some(path) => path,
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

    pub(super) fn reset_drag(&mut self) {
        self.ui.drag.active_path = None;
        self.ui.drag.label.clear();
        self.ui.drag.position = None;
        self.ui.drag.hovering_collection = None;
        self.ui.drag.hovering_drop_zone = false;
        self.ui.drag.hovering_triage = None;
    }
}
