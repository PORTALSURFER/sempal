use super::*;

pub(super) fn nudge_selection(controller: &mut EguiController, offset: isize) {
    let list = controller.visible_browser_indices().to_vec();
    if list.is_empty() {
        return;
    };
    let next_row = visible_row_after_offset(controller, offset, &list);
    controller.focus_browser_row_only(next_row);
    let _ = controller.play_audio(controller.ui.waveform.loop_enabled, None);
}

pub(super) fn grow_selection(controller: &mut EguiController, offset: isize) {
    let list = controller.visible_browser_indices().to_vec();
    if list.is_empty() {
        return;
    };
    let next_row = visible_row_after_offset(controller, offset, &list);
    controller.extend_browser_selection_to_row(next_row);
    let _ = controller.play_audio(controller.ui.waveform.loop_enabled, None);
}

fn visible_row_after_offset(controller: &EguiController, offset: isize, list: &[usize]) -> usize {
    let current_row = controller
        .ui
        .browser
        .selected_visible
        .or_else(|| {
            controller
                .selected_row_index()
                .and_then(|idx| list.iter().position(|i| *i == idx))
        })
        .unwrap_or(0) as isize;
    (current_row + offset).clamp(0, list.len() as isize - 1) as usize
}

