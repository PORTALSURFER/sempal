//! Selection and waveform view state for the controller.

use super::super::{
    CollectionId, DecodedWaveform, SampleSource, SelectionRange, SourceId, WaveformRenderer, wavs,
};
use super::audio::LoadedAudio;
use crate::selection::SelectionState;
use std::path::PathBuf;

pub(in crate::egui_app::controller) struct WavSelectionState {
    pub(in crate::egui_app::controller) selected_wav: Option<PathBuf>,
    pub(in crate::egui_app::controller) loaded_wav: Option<PathBuf>,
    pub(in crate::egui_app::controller) loaded_audio: Option<LoadedAudio>,
}

impl WavSelectionState {
    pub(in crate::egui_app::controller) fn new() -> Self {
        Self {
            selected_wav: None,
            loaded_wav: None,
            loaded_audio: None,
        }
    }
}

pub(in crate::egui_app::controller) struct ControllerSampleViewState {
    pub(in crate::egui_app::controller) renderer: WaveformRenderer,
    pub(in crate::egui_app::controller) waveform: WaveformState,
    pub(in crate::egui_app::controller) waveform_slide: Option<WaveformSlideState>,
    pub(in crate::egui_app::controller) wav: WavSelectionState,
}

impl ControllerSampleViewState {
    pub(in crate::egui_app::controller) fn new(renderer: WaveformRenderer) -> Self {
        let (waveform_width, waveform_height) = renderer.dimensions();
        Self {
            renderer,
            waveform: WaveformState {
                size: [waveform_width, waveform_height],
                decoded: None,
                render_meta: None,
            },
            waveform_slide: None,
            wav: WavSelectionState::new(),
        }
    }
}

/// Cached state for a circular waveform slide drag.
pub(in crate::egui_app::controller) struct WaveformSlideState {
    pub(in crate::egui_app::controller) source: SampleSource,
    pub(in crate::egui_app::controller) relative_path: PathBuf,
    pub(in crate::egui_app::controller) absolute_path: PathBuf,
    pub(in crate::egui_app::controller) original_samples: Vec<f32>,
    pub(in crate::egui_app::controller) channels: usize,
    pub(in crate::egui_app::controller) spec_channels: u16,
    pub(in crate::egui_app::controller) sample_rate: u32,
    pub(in crate::egui_app::controller) start_normalized: f32,
    pub(in crate::egui_app::controller) last_offset_frames: isize,
}

pub(in crate::egui_app::controller) struct SelectionContextState {
    pub(in crate::egui_app::controller) selected_source: Option<SourceId>,
    pub(in crate::egui_app::controller) last_selected_browsable_source: Option<SourceId>,
    pub(in crate::egui_app::controller) selected_collection: Option<CollectionId>,
}

impl SelectionContextState {
    pub(in crate::egui_app::controller) fn new() -> Self {
        Self {
            selected_source: None,
            last_selected_browsable_source: None,
            selected_collection: None,
        }
    }
}

pub(in crate::egui_app::controller) struct SelectionUndoState {
    pub(in crate::egui_app::controller) label: String,
    pub(in crate::egui_app::controller) before: Option<SelectionRange>,
}

pub(in crate::egui_app::controller) struct ControllerSelectionState {
    pub(in crate::egui_app::controller) ctx: SelectionContextState,
    pub(in crate::egui_app::controller) range: SelectionState,
    pub(in crate::egui_app::controller) pending_undo: Option<SelectionUndoState>,
    pub(in crate::egui_app::controller) suppress_autoplay_once: bool,
    pub(in crate::egui_app::controller) bpm_scale_beats: Option<f32>,
}

impl ControllerSelectionState {
    pub(in crate::egui_app::controller) fn new() -> Self {
        Self {
            ctx: SelectionContextState::new(),
            range: SelectionState::new(),
            pending_undo: None,
            suppress_autoplay_once: false,
            bpm_scale_beats: None,
        }
    }
}

pub(in crate::egui_app::controller) struct WaveformState {
    pub(in crate::egui_app::controller) size: [u32; 2],
    pub(in crate::egui_app::controller) decoded: Option<DecodedWaveform>,
    pub(in crate::egui_app::controller) render_meta: Option<wavs::WaveformRenderMeta>,
}
