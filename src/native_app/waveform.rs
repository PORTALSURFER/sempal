#![allow(missing_docs)]

use crate::native_app::ui::ids as widget_ids;

const WAVEFORM_WIDTH: usize = 1200;
const WAVEFORM_HEIGHT: usize = 320;
pub(in crate::native_app) const WAVEFORM_SIGNAL_WIDGET_ID: u64 =
    widget_ids::WAVEFORM_SIGNAL_WIDGET_ID;
pub(in crate::native_app) const WAVEFORM_WIDGET_ID: u64 = widget_ids::WAVEFORM_WIDGET_ID;
const MIN_VISIBLE_FRAMES: usize = 256;
const BAND_COUNT: usize = 4;
// Click-vs-drag intent is pixel-based in widget_input; do not add viewport-scaled delay here.
const SELECTION_DRAG_EPSILON: f32 = 0.0;
const SELECTION_FLASH_FRAMES: u8 = 12;
const DENIED_SELECTION_FLASH_FRAMES: u8 = 24;
const DENIED_SELECTION_FLASH_PULSE_FRAMES: u8 = 6;
#[cfg(test)]
const SYNTHETIC_SAMPLE_RATE: u32 = 48_000;
#[cfg(test)]
const SYNTHETIC_SECONDS: usize = 1;

mod types;
pub(super) use types::{
    PlaymarkLabelMessage, WaveformActiveDragKind, WaveformContextMenu, WaveformEditFadeHandle,
    WaveformEditFadeOuterGainHandle, WaveformInteraction, WaveformSelectionEdge,
    WaveformSelectionKind,
};

mod interaction;
use interaction::{WaveformDrag, edit_preview_for_selection};
mod bpm_snap;
use bpm_snap::snap_resize_ratio_to_whole_bpm;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::native_app) struct WaveformBpmSnapSettings {
    pub(in crate::native_app) enabled: bool,
    pub(in crate::native_app) beat_count: u8,
}

mod state;
mod state_extraction;
pub(in crate::native_app) use state_extraction::{
    WaveformExtractionCompletion, WaveformExtractionRequest, execute_waveform_extraction,
};
mod state_file;
mod state_interaction;
pub(in crate::native_app) use state::{WaveformDetailKey, WaveformDetailResult, WaveformState};
mod state_loading;
mod state_marked_ranges;
mod state_playback;
mod state_preserved_marks;
mod state_range_history;
pub(in crate::native_app) use state_preserved_marks::WaveformPreservedMarks;
mod state_selection;
mod state_transient;
mod state_viewport;
mod state_viewport_access;
mod zero_crossing_snap;
#[cfg(test)]
pub(in crate::native_app) use state_marked_ranges::random_marked_play_range_for_unit;

mod audio_file;
#[cfg(test)]
pub(in crate::native_app) use audio_file::PersistedPlaybackCacheFile;
pub(super) use audio_file::WaveformFile;
#[cfg(test)]
pub(in crate::native_app) use audio_file::cached_waveform_file_playback_ready_exists;
#[cfg(test)]
pub(in crate::native_app) use audio_file::file_backed_wav_playback_descriptor;
#[cfg(test)]
pub(super) use audio_file::store_cached_waveform_file_for_tests;
#[cfg(test)]
pub(super) use audio_file::store_summary_only_cached_waveform_file_for_tests;
pub(in crate::native_app) use audio_file::{
    InstantWaveformPreview, InstantWaveformPreviewTier, PersistedPlaybackDescriptor,
    PreviewAuditionClip, WaveformPlaybackReady, cached_waveform_file_audition_ready_exists,
    cached_waveform_file_exists, decode_wav_preview_clip,
    flush_background_waveform_cache_stores_for_shutdown, instant_waveform_head_preview_from_clip,
    invalidate_persisted_waveform_cache_path, invalidate_persisted_waveform_cache_paths,
    invalidate_persisted_waveform_cache_ref, load_cached_waveform_file_for_playback,
    load_cached_waveform_playback_descriptor_sidecar, load_wav_detail_summary,
    mark_cached_waveform_file_source_warm_attempted, remap_persisted_waveform_cache_after_move,
    should_use_file_backed_wav_decode, should_use_file_backed_wav_decode_for_entry,
};
#[cfg(test)]
pub(super) use audio_file::{
    test_decoded_waveform_file_from_mono_samples, test_file_backed_waveform_file_from_mono_samples,
    test_waveform_file_from_mono_samples,
};

mod similar_sections;
#[cfg(test)]
use audio_file::load_wav_waveform_summary_from_path_with_progress;
#[cfg(test)]
use audio_file::{downmix_to_mono, split_frequency_bands, waveform_file_from_mono_samples};
pub(in crate::native_app) use similar_sections::{
    SimilarSectionsResult, execute_similar_sections_scan,
};

mod widget;
#[cfg(test)]
pub(in crate::native_app::waveform) use widget::LiveSelectionPreview;
#[cfg(test)]
pub(in crate::native_app::waveform) use widget::signal_edit_selection_for_state;
#[cfg(test)]
pub(in crate::native_app::waveform) use widget::signal_gain_preview_for_state;
#[cfg(test)]
pub(in crate::native_app::waveform) use widget::waveform_signal_surface_view;
pub(super) use widget::{WaveformWidget, WaveformWidgetProps, waveform_viewport_view_with_tooltip};

mod widget_geometry;
mod widget_input;

mod edit_fade_curve_paint;
mod edit_fade_geometry;
mod edit_fade_paint;
mod played_range_paint;
mod playmark_label;
mod playmark_label_editor;
pub(super) use playmark_label_editor::PlaymarkLabelEditorState;
mod selection_paint;

mod viewport;
pub(super) use viewport::WaveformViewport;

#[cfg(test)]
mod tests;
