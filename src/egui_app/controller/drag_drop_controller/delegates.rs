use super::*;
use crate::egui_app::state::DragSample;

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

    /// Begin dragging multiple sample rows from the UI.
    pub fn start_samples_drag(&mut self, samples: Vec<DragSample>, label: String, pos: Pos2) {
        self.drag_drop().start_samples_drag(samples, label, pos);
    }

    /// Move sample rows into a target folder (used by folder hotkeys).
    pub fn move_samples_to_folder(&mut self, samples: Vec<DragSample>, target_folder: PathBuf) {
        self.drag_drop()
            .handle_samples_drop_to_folder(&samples, &target_folder);
    }

    /// Begin dragging a folder row from the UI.
    pub fn start_folder_drag(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        label: String,
        pos: Pos2,
    ) {
        self.drag_drop()
            .start_folder_drag(source_id, relative_path, label, pos);
    }

    /// Begin dragging the current waveform selection as a payload.
    pub fn start_selection_drag_payload(
        &mut self,
        bounds: SelectionRange,
        pos: Pos2,
        keep_source_focused: bool,
    ) {
        self.drag_drop()
            .start_selection_drag_payload(bounds, pos, keep_source_focused);
    }

    /// Update the active drag state with a new pointer position and target.
    pub fn update_active_drag(
        &mut self,
        pos: Pos2,
        source: DragSource,
        target: DragTarget,
        shift_down: bool,
    ) {
        self.drag_drop()
            .update_active_drag(pos, source, target, shift_down);
    }

    /// Update the stored drag pointer position (used when egui pointer positions are missing).
    pub fn refresh_drag_position(&mut self, pos: Pos2, shift_down: bool) {
        self.drag_drop().refresh_drag_position(pos, shift_down);
    }

    /// Finish the active drag gesture and apply any resulting action.
    pub fn finish_active_drag(&mut self) {
        self.drag_drop().finish_active_drag();
    }

    /// Cancel the active drag gesture without applying any drop action.
    pub fn cancel_active_drag(&mut self) {
        self.drag_drop().reset_drag();
    }

    #[cfg(target_os = "windows")]
    /// Attempt to start an OS-level drag out of the app window (Windows-only).
    pub fn maybe_launch_external_drag(&mut self, pointer_outside: bool, pointer_left: bool) {
        self.drag_drop()
            .maybe_launch_external_drag(pointer_outside, pointer_left);
    }
}
