use super::*;

impl EguiController {
    pub fn start_sample_drag(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        label: String,
        pos: Pos2,
    ) {
        self.drag_drop()
            .start_sample_drag(source_id, relative_path, label, pos);
    }

    pub fn start_selection_drag_payload(&mut self, bounds: SelectionRange, pos: Pos2) {
        self.drag_drop().start_selection_drag_payload(bounds, pos);
    }

    pub fn update_active_drag(&mut self, pos: Pos2, source: DragSource, target: DragTarget) {
        self.drag_drop().update_active_drag(pos, source, target);
    }

    pub fn refresh_drag_position(&mut self, pos: Pos2) {
        self.drag_drop().refresh_drag_position(pos);
    }

    pub fn finish_active_drag(&mut self) {
        self.drag_drop().finish_active_drag();
    }

    #[cfg(target_os = "windows")]
    pub fn maybe_launch_external_drag(&mut self, pointer_outside: bool) {
        self.drag_drop().maybe_launch_external_drag(pointer_outside);
    }

    #[cfg(not(target_os = "windows"))]
    pub fn maybe_launch_external_drag(&mut self, pointer_outside: bool) {
        self.drag_drop().maybe_launch_external_drag(pointer_outside);
    }

    pub(super) fn reset_drag(&mut self) {
        self.drag_drop().reset_drag();
    }
}
