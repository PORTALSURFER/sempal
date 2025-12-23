use crate::egui_app::controller::EguiController;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget, PendingOsDragStart};
use eframe::egui::{self, StrokeKind};

pub(super) fn pointer_pos_for_drag(
    ui: &egui::Ui,
    drag_position: Option<egui::Pos2>,
) -> Option<egui::Pos2> {
    ui.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
        .or(drag_position)
}

pub(super) fn handle_drop_zone(
    ui: &egui::Ui,
    controller: &mut EguiController,
    drag_active: bool,
    pointer_pos: Option<egui::Pos2>,
    target_rect: egui::Rect,
    drag_source: DragSource,
    drag_target: DragTarget,
    stroke: egui::Stroke,
    stroke_kind: StrokeKind,
) -> bool {
    if !drag_active {
        return false;
    }
    let Some(pointer) = pointer_pos else {
        return false;
    };
    if !target_rect.contains(pointer) {
        return false;
    }
    let shift_down = ui.input(|i| i.modifiers.shift);
    controller.update_active_drag(pointer, drag_source, drag_target, shift_down);
    ui.painter()
        .rect_stroke(target_rect, 6.0, stroke, stroke_kind);
    true
}

pub(super) fn handle_sample_row_drag<StartDrag, BuildPending, PendingMatch>(
    ui: &egui::Ui,
    response: &egui::Response,
    drag_active: bool,
    controller: &mut EguiController,
    drag_source: DragSource,
    drag_target: DragTarget,
    start_drag: StartDrag,
    build_pending: BuildPending,
    matches_pending: PendingMatch,
) where
    StartDrag: FnOnce(egui::Pos2, &mut EguiController),
    BuildPending: FnOnce(egui::Pos2, &EguiController) -> Option<PendingOsDragStart>,
    PendingMatch: Fn(&PendingOsDragStart) -> bool,
{
    let should_start_drag = response.drag_started() || (!drag_active && response.dragged());
    if should_start_drag {
        controller.ui.drag.pending_os_drag = None;
        if let Some(pos) = response.interact_pointer_pos() {
            start_drag(pos, controller);
        }
        return;
    }
    if !drag_active
        && controller.ui.drag.payload.is_none()
        && controller.ui.drag.os_left_mouse_pressed
        && controller.ui.drag.pending_os_drag.is_none()
    {
        let pointer_pos = cursor_pos_for_pending(ui, controller);
        if let Some(pos) = pointer_pos {
            if !response.rect.contains(pos) {
                return;
            }
            if let Some(pending) = build_pending(pos, controller) {
                controller.ui.drag.pending_os_drag = Some(pending);
            }
        }
        return;
    }
    if !drag_active
        && controller.ui.drag.payload.is_none()
        && controller.ui.drag.os_left_mouse_down
        && let Some(pending) = controller.ui.drag.pending_os_drag.clone()
        && matches_pending(&pending)
    {
        let pointer_pos = cursor_pos_for_pending(ui, controller);
        if let Some(pos) = pointer_pos {
            let moved_sq = (pos - pending.origin).length_sq();
            const START_DRAG_DISTANCE_SQ: f32 = 4.0 * 4.0;
            if moved_sq >= START_DRAG_DISTANCE_SQ {
                controller.ui.drag.pending_os_drag = None;
                start_drag_from_pending(controller, pending, pos);
            }
        }
        return;
    }
    if drag_active && response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let shift_down = ui.input(|i| i.modifiers.shift);
            controller.update_active_drag(pos, drag_source, drag_target, shift_down);
        }
    } else if response.drag_stopped() {
        controller.finish_active_drag();
    }
}

fn cursor_pos_for_pending(ui: &egui::Ui, controller: &EguiController) -> Option<egui::Pos2> {
    ui.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
        .or(controller.ui.drag.os_cursor_pos)
}

fn start_drag_from_pending(
    controller: &mut EguiController,
    pending: PendingOsDragStart,
    pos: egui::Pos2,
) {
    match pending.payload {
        DragPayload::Sample {
            source_id,
            relative_path,
        } => controller.start_sample_drag(source_id, relative_path, pending.label, pos),
        DragPayload::Samples { samples } => {
            controller.start_samples_drag(samples, pending.label, pos)
        }
        DragPayload::Selection { .. } => {}
    }
}
