use super::*;

impl EguiController {
    /// Begin dragging a sample row from the UI.
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

    /// Begin dragging the current waveform selection as a payload.
    pub fn start_selection_drag_payload(&mut self, bounds: SelectionRange, pos: Pos2) {
        self.drag_drop().start_selection_drag_payload(bounds, pos);
    }

    /// Update the active drag state with a new pointer position and target.
    pub fn update_active_drag(&mut self, pos: Pos2, source: DragSource, target: DragTarget) {
        self.drag_drop().update_active_drag(pos, source, target);
    }

    /// Update the stored drag pointer position (used when egui pointer positions are missing).
    pub fn refresh_drag_position(&mut self, pos: Pos2) {
        self.drag_drop().refresh_drag_position(pos);
    }

    /// Finish the active drag gesture and apply any resulting action.
    pub fn finish_active_drag(&mut self) {
        self.drag_drop().finish_active_drag();
    }

    #[cfg(target_os = "windows")]
    /// Attempt to start an OS-level drag out of the app window (Windows-only).
    pub fn maybe_launch_external_drag(&mut self, pointer_outside: bool) {
        self.drag_drop().maybe_launch_external_drag(pointer_outside);
    }
}
