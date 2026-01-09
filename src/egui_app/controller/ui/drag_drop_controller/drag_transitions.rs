use super::{DragDropController, DragPayload, Pos2};

impl DragDropController<'_> {
    pub(crate) fn reset_drag(&mut self) {
        self.ui.drag.payload = None;
        self.ui.drag.label.clear();
        self.ui.drag.position = None;
        self.ui.drag.clear_all_targets();
        self.ui.drag.last_folder_target = None;
        self.ui.drag.copy_on_drop = false;
        self.ui.drag.external_started = false;
        self.ui.drag.external_arm_at = None;
        self.ui.drag.pointer_left_window = false;
        self.ui.drag.pending_os_drag = None;
        self.ui.drag.origin_source = None;
    }

    pub(crate) fn begin_drag(&mut self, payload: DragPayload, label: String, pos: Pos2) {
        self.ui.drag.payload = Some(payload);
        self.ui.drag.label = label;
        self.ui.drag.position = Some(pos);
        self.ui.drag.clear_all_targets();
        self.ui.drag.last_folder_target = None;
        self.ui.drag.copy_on_drop = false;
        self.ui.drag.external_started = false;
        self.ui.drag.external_arm_at = None;
        self.ui.drag.pointer_left_window = false;
        self.ui.drag.pending_os_drag = None;
        self.ui.drag.origin_source = None;
    }
}
