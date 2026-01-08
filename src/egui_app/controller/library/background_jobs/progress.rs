use super::*;
use crate::egui_app::state::ProgressTaskKind;

pub(crate) fn ensure_progress_visible(
    controller: &mut EguiController,
    task: ProgressTaskKind,
    label: &str,
    total: usize,
    cancellable: bool,
) {
    if !controller.ui.progress.visible || controller.ui.progress.task != Some(task) {
        controller.show_status_progress(task, label, total, cancellable);
    }
}

pub(crate) fn update_progress_detail(
    controller: &mut EguiController,
    task: ProgressTaskKind,
    completed: usize,
    detail: Option<String>,
) {
    if controller.ui.progress.task == Some(task) {
        controller
            .ui
            .progress
            .set_counts(controller.ui.progress.total, completed);
        controller.ui.progress.set_detail(detail);
    }
}

pub(crate) fn update_progress_totals(
    controller: &mut EguiController,
    task: ProgressTaskKind,
    total: usize,
    completed: usize,
    detail: Option<String>,
) {
    if controller.ui.progress.task == Some(task) {
        controller.ui.progress.set_counts(total, completed);
        controller.ui.progress.set_detail(detail);
    }
}
