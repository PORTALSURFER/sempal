use radiant::prelude as ui;

use crate::native_app::app::{NativeAppState, NativeFileDropHover, sample_path_label};
use crate::native_app::app_chrome::waveform_context_menu;
use crate::native_app::waveform::WaveformState;

pub(in crate::native_app) struct WaveformPanelViewModel<'a> {
    pub(in crate::native_app) waveform: &'a WaveformState,
    pub(in crate::native_app) panel_height: f32,
    pub(in crate::native_app) drop_hover: Option<&'a NativeFileDropHover>,
    pub(in crate::native_app) loading_label: Option<&'a str>,
    pub(in crate::native_app) failed_label: Option<String>,
    pub(in crate::native_app) instant_preview_label: Option<String>,
    pub(in crate::native_app) instant_preview_tier:
        Option<crate::native_app::waveform::InstantWaveformPreviewTier>,
    pub(in crate::native_app) instant_preview_active: bool,
    pub(in crate::native_app) block_input_while_loading: bool,
    pub(in crate::native_app) help_tooltips_enabled: bool,
    pub(in crate::native_app) beat_guides_enabled: bool,
    pub(in crate::native_app) bpm_snap_enabled: bool,
    pub(in crate::native_app) beat_guide_count: u8,
    pub(in crate::native_app) normalized_audition_enabled: bool,
    pub(in crate::native_app) playhead_occlusion_rect: Option<ui::Rect>,
}

impl<'a> WaveformPanelViewModel<'a> {
    pub(in crate::native_app) fn from_app_state(state: &'a NativeAppState) -> Self {
        Self {
            waveform: &state.waveform.current,
            panel_height: state.ui.chrome.waveform_panel_height(),
            drop_hover: state.ui.browser_interaction.native_file_drop_hover.as_ref(),
            loading_label: state.waveform.load.label.as_deref(),
            failed_label: state.waveform.load.selection.failed_waveform_label(),
            instant_preview_label: state
                .waveform
                .instant_preview_path()
                .map(|path| sample_path_label(&path.display().to_string())),
            instant_preview_tier: state.waveform.instant_preview_tier(),
            instant_preview_active: state.waveform.instant_preview_active(),
            block_input_while_loading: state.waveform_input_blocked_by_sample_load()
                || state.waveform.instant_preview_active(),
            help_tooltips_enabled: state.ui.chrome.help_tooltips_enabled,
            beat_guides_enabled: state.ui.chrome.beat_guides_enabled,
            bpm_snap_enabled: state.ui.chrome.bpm_snap_enabled,
            beat_guide_count: state.ui.chrome.beat_guide_count,
            normalized_audition_enabled: state.audio.normalized_audition_enabled,
            playhead_occlusion_rect: state
                .ui
                .browser_interaction
                .waveform_context_menu
                .as_ref()
                .map(waveform_context_menu::overlay_rect),
        }
    }
}
