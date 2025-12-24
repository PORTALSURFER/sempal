use super::*;

pub(super) fn nudge_selection(controller: &mut EguiController, offset: isize) {
    let list_len = controller.visible_browser_len();
    if list_len == 0 {
        return;
    };
    let next_row = visible_row_after_offset(controller, offset, list_len);
    controller.focus_browser_row_only(next_row);
    let _ = controller.play_audio(controller.ui.waveform.loop_enabled, None);
}

pub(super) fn grow_selection(controller: &mut EguiController, offset: isize) {
    let list_len = controller.visible_browser_len();
    if list_len == 0 {
        return;
    };
    let next_row = visible_row_after_offset(controller, offset, list_len);
    controller.extend_browser_selection_to_row(next_row);
    let _ = controller.play_audio(controller.ui.waveform.loop_enabled, None);
}

fn visible_row_after_offset(controller: &mut EguiController, offset: isize, list_len: usize) -> usize {
    let selected_wav = controller.sample_view.wav.selected_wav.clone();
    let current_row = controller
        .ui
        .browser
        .selected_visible
        .or_else(|| {
            selected_wav
                .as_ref()
                .and_then(|path| controller.visible_row_for_path(path))
        })
        .unwrap_or(0) as isize;
    (current_row + offset).clamp(0, list_len as isize - 1) as usize
}
