use super::*;
use crate::egui_app::state::WaveformView;

pub(super) fn clear_waveform_view(controller: &mut EguiController) {
    controller.ui.waveform.image = None;
    controller.ui.waveform.notice = None;
    controller.ui.waveform.loading = None;
    controller.ui.waveform.transients.clear();
    controller.ui.waveform.transient_cache_token = None;
    controller.sample_view.waveform.decoded = None;
    controller.ui.waveform.playhead = PlayheadState::default();
    controller.ui.waveform.last_start_marker = None;
    controller.ui.waveform.cursor = None;
    controller.ui.waveform.selection = None;
    controller.ui.waveform.selection_duration = None;
    controller.ui.waveform.slices.clear();
    controller.ui.waveform.view = WaveformView::default();
    controller.selection_state.range.clear();
    controller.sample_view.wav.loaded_audio = None;
    controller.sample_view.wav.loaded_wav = None;
    controller.ui.loaded_wav = None;
    controller.sample_view.waveform.render_meta = None;
    if let Some(player) = controller.audio.player.as_ref() {
        player.borrow_mut().stop();
    }
    controller.runtime.jobs.set_pending_audio(None);
    controller.runtime.jobs.set_pending_playback(None);
}
