use radiant::{
    gui::types::Rect,
    gui::visualization::TimelineEditPreview,
    layout::LayoutOutput,
    prelude as ui,
    runtime::{
        GpuSurfaceCapabilities, GpuSurfaceContent, PaintPrimitive, gpu_surface_with_capabilities,
        push_fill_rect,
    },
    theme::ThemeTokens,
    widgets::{CanvasGestureState, Widget, WidgetCommon, WidgetInput, WidgetOutput},
};

use crate::native_app::app_chrome::palette::ACCENT_SOFT;
use std::sync::Arc;

use crate::native_app::app::GuiMessage;
use crate::native_app::ui::ids as widget_ids;
use crate::native_app::waveform::{WAVEFORM_SIGNAL_WIDGET_ID, WAVEFORM_WIDGET_ID};

use super::{
    DENIED_SELECTION_FLASH_FRAMES, DENIED_SELECTION_FLASH_PULSE_FRAMES, WAVEFORM_HEIGHT,
    WAVEFORM_WIDTH, WaveformActiveDragKind, WaveformEditFadeHandle,
    WaveformEditFadeOuterGainHandle, WaveformFile, WaveformInteraction, WaveformSelectionKind,
    WaveformState, WaveformViewport,
    audio_file::{gain_preview_for_range_with_gain, gain_preview_for_selection},
    edit_preview_for_selection,
    playmark_label_editor::PlaymarkLabelWidget,
    widget_geometry::WaveformSelectionHandleHover,
};

const fn protected_source_error_flash_visible(frames: u8) -> bool {
    if frames == 0 {
        return false;
    }
    let elapsed = DENIED_SELECTION_FLASH_FRAMES.saturating_sub(frames);
    ((elapsed / DENIED_SELECTION_FLASH_PULSE_FRAMES) % 2) == 0
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::native_app::waveform) struct LiveSelectionPreview {
    pub(super) kind: WaveformSelectionKind,
    pub(super) selection: wavecrate::selection::SelectionRange,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::native_app::waveform) struct LiveSelectionPreviewAnchor {
    pub(super) kind: WaveformSelectionKind,
    pub(super) visible_ratio: f32,
    pub(super) baseline: Option<wavecrate::selection::SelectionRange>,
}

pub(in crate::native_app) fn waveform_viewport_view_with_tooltip(
    state: &WaveformState,
    tooltip: Option<&'static str>,
    beat_guides_enabled: bool,
    bpm_snap_enabled: bool,
    beat_guide_count: u8,
    normalized_audition_enabled: bool,
    playhead_occlusion_rect: Option<Rect>,
) -> ui::View<GuiMessage> {
    let props = WaveformWidgetProps::from_state_with_playhead_occlusion(
        state,
        beat_guides_enabled,
        bpm_snap_enabled,
        beat_guide_count,
        playhead_occlusion_rect,
    );
    let interaction = ui::custom_widget(
        WaveformWidget::new(props.clone()).with_external_playmark_label_owner(),
        |output| {
            output
                .typed_copied::<WaveformInteraction>()
                .map(GuiMessage::Waveform)
        },
    )
    .id(WAVEFORM_WIDGET_ID)
    .size(WAVEFORM_WIDTH as f32, WAVEFORM_HEIGHT as f32);
    let interaction = if let Some(tooltip) = tooltip {
        interaction.tooltip(tooltip)
    } else {
        interaction
    };
    let label = PlaymarkLabelWidget::new(
        widget_ids::WAVEFORM_PLAYMARK_LABEL_ID,
        WaveformWidget::new(props.clone()),
        state,
        beat_guides_enabled,
        beat_guide_count,
    )
    .map(|widget| {
        ui::custom_widget(widget, |output| {
            output
                .typed_cloned::<super::PlaymarkLabelMessage>()
                .map(GuiMessage::PlaymarkLabel)
        })
        .id(widget_ids::WAVEFORM_PLAYMARK_LABEL_ID)
        .size(WAVEFORM_WIDTH as f32, WAVEFORM_HEIGHT as f32)
    });
    let layers = [
        waveform_signal_surface_view(
            state,
            signal_gain_preview_for_state(state, normalized_audition_enabled),
            state.pending_sample_slide_frame_offset,
        )
        .id(WAVEFORM_SIGNAL_WIDGET_ID)
        .size(WAVEFORM_WIDTH as f32, WAVEFORM_HEIGHT as f32),
        interaction,
    ]
    .into_iter()
    .chain(label);

    ui::stack(layers)
        .id(widget_ids::WAVEFORM_VIEWPORT_STACK_ID)
        .size(WAVEFORM_WIDTH as f32, WAVEFORM_HEIGHT as f32)
}

pub(super) fn signal_edit_selection_for_state(
    state: &WaveformState,
) -> Option<wavecrate::selection::SelectionRange> {
    let edit_selection = state.edit_selection();
    if active_edit_selection_drag_skips_signal_preview(state.active_drag_kind()) {
        None
    } else {
        edit_selection
    }
}

fn active_edit_selection_drag_skips_signal_preview(
    active_drag_kind: Option<WaveformActiveDragKind>,
) -> bool {
    matches!(
        active_drag_kind,
        Some(
            WaveformActiveDragKind::Selection(WaveformSelectionKind::Edit)
                | WaveformActiveDragKind::SelectionResize(WaveformSelectionKind::Edit, _)
                | WaveformActiveDragKind::SelectionMove(WaveformSelectionKind::Edit)
        )
    )
}

pub(in crate::native_app::waveform) fn waveform_signal_surface_view(
    state: &WaveformState,
    gain_preview: Option<radiant::runtime::GpuSignalGainPreview>,
    sample_slide_frame_offset: Option<i64>,
) -> ui::View<GuiMessage> {
    let file = state.file();
    let viewport = state.viewport();
    let detail = state
        .render_detail()
        .filter(|_| sample_slide_frame_offset.unwrap_or(0) == 0);
    let (frames, frame_range, summary, gain_preview, detail_revision) = if let Some(detail) = detail
    {
        (
            detail.summary.frames,
            [0.0, detail.summary.frames as f32],
            Arc::clone(&detail.summary),
            gain_preview
                .map(|preview| remap_gain_preview_to_detail(preview, file.frames, &detail.key)),
            detail.key.start_frame as u64 ^ (detail.key.end_frame as u64).rotate_left(17),
        )
    } else {
        (
            file.frames,
            viewport.frame_range(),
            Arc::clone(&file.gpu_signal_summary),
            gain_preview,
            0,
        )
    };
    gpu_surface_with_capabilities(
        file.path_hash(),
        file.content_revision() ^ detail_revision,
        GpuSurfaceContent::SignalSummaryBands {
            frames,
            band_count: super::BAND_COUNT,
            frame_range,
            summary,
            gain_preview,
            sample_slide_frame_offset: sample_slide_frame_offset.unwrap_or(0),
        },
        GpuSurfaceCapabilities {
            fast_pointer_move: true,
            coalesce_vertical_wheel: true,
            runtime_overlays: Default::default(),
        },
    )
}

fn remap_gain_preview_to_detail(
    mut preview: radiant::runtime::GpuSignalGainPreview,
    source_frames: usize,
    key: &super::WaveformDetailKey,
) -> radiant::runtime::GpuSignalGainPreview {
    let source_frames = source_frames.max(1) as f32;
    let detail_start = key.start_frame as f32 / source_frames;
    let detail_width = key.end_frame.saturating_sub(key.start_frame).max(1) as f32 / source_frames;
    preview.start = (preview.start - detail_start) / detail_width;
    preview.end = (preview.end - detail_start) / detail_width;
    preview
}

pub(in crate::native_app::waveform) fn signal_gain_preview_for_state(
    state: &WaveformState,
    normalized_audition_enabled: bool,
) -> Option<radiant::runtime::GpuSignalGainPreview> {
    if normalized_audition_enabled {
        let selection = state.normalized_audition_preview_selection();
        let gain = state.normalized_audition_gain_for_span(selection.start(), selection.end());
        return gain_preview_for_range_with_gain(selection, gain);
    }
    gain_preview_for_selection(signal_edit_selection_for_state(state))
}

#[derive(Clone, Debug)]
pub(in crate::native_app) struct WaveformWidgetProps {
    file: Arc<WaveformFile>,
    viewport: WaveformViewport,
    playhead_ratio: Option<f32>,
    play_mark_ratio: Option<f32>,
    edit_mark_ratio: Option<f32>,
    play_selection: Option<wavecrate::selection::SelectionRange>,
    edit_selection: Option<wavecrate::selection::SelectionRange>,
    hover_cursor_ratio: Option<f32>,
    hovered_selection_handle: Option<WaveformSelectionHandleHover>,
    hovered_edit_fade_handle: Option<WaveformEditFadeHandle>,
    hovered_edit_fade_outer_gain_handle: Option<WaveformEditFadeOuterGainHandle>,
    hovered_edit_gain_handle: bool,
    hovered_similar_section: Option<wavecrate::selection::SelectionRange>,
    extracted_ranges: Vec<wavecrate::selection::SelectionRange>,
    played_ranges: Vec<wavecrate::selection::SelectionRange>,
    similar_section_ranges: Vec<wavecrate::selection::SelectionRange>,
    play_selection_flash_frames: u8,
    edit_selection_flash_frames: u8,
    play_selection_denied_flash_frames: u8,
    edit_selection_denied_flash_frames: u8,
    copy_flash_frames: u8,
    protected_source_error_flash_frames: u8,
    sample_slide_frame_offset: Option<i64>,
    beat_guides_enabled: bool,
    bpm_snap_enabled: bool,
    beat_guide_count: u8,
    playhead_occlusion_rect: Option<Rect>,
    playmark_label_editor_active: bool,
    pub(in crate::native_app::waveform) active_drag_kind: Option<WaveformActiveDragKind>,
}

impl WaveformWidgetProps {
    #[cfg(test)]
    pub(super) fn from_state(
        state: &WaveformState,
        beat_guides_enabled: bool,
        bpm_snap_enabled: bool,
        beat_guide_count: u8,
    ) -> Self {
        Self::from_state_with_playhead_occlusion(
            state,
            beat_guides_enabled,
            bpm_snap_enabled,
            beat_guide_count,
            None,
        )
    }

    pub(in crate::native_app) fn from_state_with_playhead_occlusion(
        state: &WaveformState,
        beat_guides_enabled: bool,
        bpm_snap_enabled: bool,
        beat_guide_count: u8,
        playhead_occlusion_rect: Option<Rect>,
    ) -> Self {
        let active_drag_kind = state.active_drag_kind();
        Self {
            file: state.file(),
            viewport: state.viewport(),
            playhead_ratio: if state.is_playing() {
                None
            } else {
                state.playhead_ratio()
            },
            play_mark_ratio: state.play_mark_ratio(),
            edit_mark_ratio: state.edit_mark_ratio(),
            play_selection: state.play_selection(),
            edit_selection: state.edit_selection(),
            hover_cursor_ratio: None,
            hovered_selection_handle: None,
            hovered_edit_fade_handle: None,
            hovered_edit_fade_outer_gain_handle: None,
            hovered_edit_gain_handle: false,
            hovered_similar_section: None,
            extracted_ranges: if extracted_range_overlays_visible(active_drag_kind) {
                state.extracted_ranges().to_vec()
            } else {
                Vec::new()
            },
            played_ranges: state.played_ranges().to_vec(),
            similar_section_ranges: if similar_section_overlays_visible(active_drag_kind) {
                state.similar_section_ranges().to_vec()
            } else {
                Vec::new()
            },
            play_selection_flash_frames: state.play_selection_flash_frames(),
            edit_selection_flash_frames: state.edit_selection_flash_frames(),
            play_selection_denied_flash_frames: state.play_selection_denied_flash_frames(),
            edit_selection_denied_flash_frames: state.edit_selection_denied_flash_frames(),
            copy_flash_frames: state.copy_flash_frames(),
            protected_source_error_flash_frames: state.protected_source_error_flash_frames(),
            sample_slide_frame_offset: state.pending_sample_slide_frame_offset,
            beat_guides_enabled,
            bpm_snap_enabled,
            beat_guide_count,
            playhead_occlusion_rect,
            playmark_label_editor_active: state.playmark_label_editor.is_some(),
            active_drag_kind,
        }
    }

    #[cfg(test)]
    pub(in crate::native_app::waveform) fn static_range_overlay_counts(&self) -> (usize, usize) {
        (
            self.extracted_ranges.len(),
            self.similar_section_ranges.len(),
        )
    }
}

fn extracted_range_overlays_visible(active_drag_kind: Option<WaveformActiveDragKind>) -> bool {
    active_drag_kind.is_none()
        || active_drag_kind
            .and_then(WaveformActiveDragKind::selection_kind)
            .is_some()
}

fn similar_section_overlays_visible(active_drag_kind: Option<WaveformActiveDragKind>) -> bool {
    active_drag_kind.is_none()
}

#[derive(Clone, Debug)]
pub(in crate::native_app) struct WaveformWidget {
    pub(super) common: WidgetCommon,
    pub(super) gesture: CanvasGestureState,
    pub(super) file: Arc<WaveformFile>,
    pub(super) viewport: WaveformViewport,
    pub(super) playhead_ratio: Option<f32>,
    pub(super) play_mark_ratio: Option<f32>,
    pub(super) edit_mark_ratio: Option<f32>,
    pub(super) play_selection: Option<wavecrate::selection::SelectionRange>,
    pub(super) edit_selection: Option<wavecrate::selection::SelectionRange>,
    pub(super) hover_cursor_ratio: Option<f32>,
    pub(super) hovered_selection_handle: Option<WaveformSelectionHandleHover>,
    pub(super) hovered_edit_fade_handle: Option<WaveformEditFadeHandle>,
    pub(super) hovered_edit_fade_outer_gain_handle: Option<WaveformEditFadeOuterGainHandle>,
    pub(super) hovered_edit_gain_handle: bool,
    pub(super) hovered_similar_section: Option<wavecrate::selection::SelectionRange>,
    pub(super) extracted_ranges: Vec<wavecrate::selection::SelectionRange>,
    pub(super) played_ranges: Vec<wavecrate::selection::SelectionRange>,
    pub(super) similar_section_ranges: Vec<wavecrate::selection::SelectionRange>,
    pub(super) play_selection_flash_frames: u8,
    pub(super) edit_selection_flash_frames: u8,
    pub(super) play_selection_denied_flash_frames: u8,
    pub(super) edit_selection_denied_flash_frames: u8,
    pub(super) copy_flash_frames: u8,
    pub(super) protected_source_error_flash_frames: u8,
    pub(super) sample_slide_frame_offset: Option<i64>,
    pub(super) beat_guides_enabled: bool,
    pub(super) bpm_snap_enabled: bool,
    pub(super) beat_guide_count: u8,
    pub(super) playhead_occlusion_rect: Option<Rect>,
    pub(super) playmark_label_editor_active: bool,
    pub(super) external_playmark_label_owner: bool,
    pub(super) edit_preview: TimelineEditPreview,
    pub(super) last_live_selection_update_visible_ratio: Option<f32>,
    pub(super) live_selection_preview_anchor: Option<LiveSelectionPreviewAnchor>,
    pub(super) live_selection_preview: Option<LiveSelectionPreview>,
    pub(super) live_sample_slide_anchor_visible_ratio: Option<f32>,
    pub(in crate::native_app::waveform) active_drag_kind: Option<WaveformActiveDragKind>,
}

impl WaveformWidget {
    pub(in crate::native_app) fn new(props: WaveformWidgetProps) -> Self {
        let WaveformWidgetProps {
            file,
            viewport,
            playhead_ratio,
            play_mark_ratio,
            edit_mark_ratio,
            play_selection,
            edit_selection,
            hover_cursor_ratio,
            hovered_selection_handle,
            hovered_edit_fade_handle,
            hovered_edit_fade_outer_gain_handle,
            hovered_edit_gain_handle,
            hovered_similar_section,
            extracted_ranges,
            played_ranges,
            similar_section_ranges,
            play_selection_flash_frames,
            edit_selection_flash_frames,
            play_selection_denied_flash_frames,
            edit_selection_denied_flash_frames,
            copy_flash_frames,
            protected_source_error_flash_frames,
            sample_slide_frame_offset,
            beat_guides_enabled,
            bpm_snap_enabled,
            beat_guide_count,
            playhead_occlusion_rect,
            playmark_label_editor_active,
            active_drag_kind,
        } = props;
        let common = WidgetCommon::fixed(0, WAVEFORM_WIDTH as f32, WAVEFORM_HEIGHT as f32)
            .with_pointer_focus()
            .without_default_chrome();
        Self {
            common,
            gesture: CanvasGestureState::new(),
            file,
            viewport,
            playhead_ratio,
            play_mark_ratio,
            edit_mark_ratio,
            play_selection,
            edit_selection,
            hover_cursor_ratio,
            hovered_selection_handle,
            hovered_edit_fade_handle,
            hovered_edit_fade_outer_gain_handle,
            hovered_edit_gain_handle,
            hovered_similar_section,
            extracted_ranges,
            played_ranges,
            similar_section_ranges,
            play_selection_flash_frames,
            edit_selection_flash_frames,
            play_selection_denied_flash_frames,
            edit_selection_denied_flash_frames,
            copy_flash_frames,
            protected_source_error_flash_frames,
            sample_slide_frame_offset,
            beat_guides_enabled,
            bpm_snap_enabled,
            beat_guide_count,
            playhead_occlusion_rect,
            playmark_label_editor_active,
            external_playmark_label_owner: false,
            edit_preview: edit_preview_for_selection(edit_selection),
            last_live_selection_update_visible_ratio: None,
            live_selection_preview_anchor: None,
            live_selection_preview: None,
            live_sample_slide_anchor_visible_ratio: None,
            active_drag_kind,
        }
    }

    pub(super) fn has_loaded_sample(&self) -> bool {
        self.file.has_loaded_sample_metadata()
    }

    fn with_external_playmark_label_owner(mut self) -> Self {
        self.external_playmark_label_owner = true;
        self
    }
}

impl Widget for WaveformWidget {
    fn common(&self) -> &WidgetCommon {
        &self.common
    }

    fn common_mut(&mut self) -> &mut WidgetCommon {
        &mut self.common
    }

    fn handle_input(&mut self, bounds: Rect, input: WidgetInput) -> Option<WidgetOutput> {
        self.handle_waveform_input(bounds, input)
    }

    fn synchronize_from_previous(&mut self, previous: &dyn Widget) {
        let Some(previous) = previous.as_any().downcast_ref::<Self>() else {
            return;
        };
        self.common.state = previous.common.state;
        self.gesture = previous.gesture.clone();
        self.hover_cursor_ratio = previous.hover_cursor_ratio;
        self.hovered_selection_handle = previous.hovered_selection_handle;
        self.hovered_edit_fade_handle = previous.hovered_edit_fade_handle;
        self.hovered_edit_fade_outer_gain_handle = previous.hovered_edit_fade_outer_gain_handle;
        self.hovered_edit_gain_handle = previous.hovered_edit_gain_handle;
        self.hovered_similar_section = previous.hovered_similar_section;
        if self.should_preserve_live_selection_preview_from(previous) {
            self.last_live_selection_update_visible_ratio =
                previous.last_live_selection_update_visible_ratio;
            self.live_selection_preview_anchor = previous.live_selection_preview_anchor;
            self.live_selection_preview = previous.live_selection_preview;
        }
        if self.should_preserve_sample_slide_preview_from(previous) {
            self.live_sample_slide_anchor_visible_ratio =
                previous.live_sample_slide_anchor_visible_ratio;
            if self.sample_slide_frame_offset.is_none() {
                self.sample_slide_frame_offset = previous.sample_slide_frame_offset;
            }
        }
    }

    fn accepts_wheel_input(&self) -> bool {
        true
    }

    fn accepts_pointer_move(&self) -> bool {
        true
    }

    fn prefers_pointer_move_paint_only(&self) -> bool {
        !matches!(
            self.active_drag_kind,
            Some(WaveformActiveDragKind::Selection(
                WaveformSelectionKind::Play
            )) | Some(WaveformActiveDragKind::SelectionMove(
                WaveformSelectionKind::Play
            )) | Some(WaveformActiveDragKind::SelectionResize(
                WaveformSelectionKind::Play,
                _
            ))
        )
    }

    fn append_paint(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
        bounds: Rect,
        _layout: &LayoutOutput,
        _theme: &ThemeTokens,
    ) {
        self.append_copy_flash_paint(primitives, bounds);
        self.append_protected_source_error_flash_paint(primitives, bounds);
        self.append_selection_and_marker_paint(primitives, bounds);
        self.append_edit_fade_paint(primitives, bounds);
    }

    fn append_runtime_overlay_paint(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
        bounds: Rect,
        _layout: &LayoutOutput,
        _theme: &ThemeTokens,
    ) {
        self.append_live_selection_preview_paint(primitives, bounds);
        self.append_playmark_drag_ghost_paint(primitives, bounds);
        self.append_sample_slide_preview_paint(primitives, bounds);
        self.append_hover_edit_fade_handle_paint(primitives, bounds);
        self.append_hover_edit_fade_outer_gain_handle_paint(primitives, bounds);
        self.append_hover_selection_handle_paint(primitives, bounds);
        self.append_hover_similar_section_paint(primitives, bounds);
        self.append_hover_cursor_paint(primitives, bounds);
    }
}

impl WaveformWidget {
    fn append_copy_flash_paint(&self, primitives: &mut Vec<PaintPrimitive>, bounds: Rect) {
        if self.copy_flash_frames == 0 {
            return;
        }
        push_fill_rect(
            primitives,
            self.common.id,
            bounds,
            ui::Rgba8::new(255, 174, 89, 46),
        );
    }

    fn append_protected_source_error_flash_paint(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
        bounds: Rect,
    ) {
        if !protected_source_error_flash_visible(self.protected_source_error_flash_frames) {
            return;
        }
        push_fill_rect(
            primitives,
            self.common.id,
            bounds,
            ui::Rgba8::new(255, 69, 54, 62),
        );
    }

    fn append_sample_slide_preview_paint(
        &self,
        primitives: &mut Vec<PaintPrimitive>,
        bounds: Rect,
    ) {
        if self.active_drag_kind != Some(WaveformActiveDragKind::SampleSlide) {
            return;
        }
        let Some(frame_offset) = self.sample_slide_frame_offset else {
            return;
        };
        let strip_height = bounds.height().min(4.0);
        let strip = Rect::from_xy_size(
            bounds.min.x,
            bounds.max.y - strip_height,
            bounds.width(),
            strip_height,
        );
        push_fill_rect(
            primitives,
            self.common.id,
            strip,
            ACCENT_SOFT.with_alpha(120),
        );
        let width = ((frame_offset.unsigned_abs() as f32 / self.viewport.visible_items() as f32)
            * bounds.width())
        .round()
        .clamp(2.0, bounds.width().max(2.0));
        let x = if frame_offset >= 0 {
            bounds.max.x - width
        } else {
            bounds.min.x
        };
        push_fill_rect(
            primitives,
            self.common.id,
            Rect::from_xy_size(x, strip.min.y, width, strip.height()),
            ACCENT_SOFT.with_alpha(210),
        );
    }

    fn should_preserve_live_selection_preview_from(&self, previous: &Self) -> bool {
        if self.active_drag_kind == previous.active_drag_kind {
            return true;
        }
        let Some(anchor) = previous.live_selection_preview_anchor else {
            return false;
        };
        self.active_drag_kind
            .and_then(WaveformActiveDragKind::selection_kind)
            == Some(anchor.kind)
    }

    fn should_preserve_sample_slide_preview_from(&self, previous: &Self) -> bool {
        self.active_drag_kind == Some(WaveformActiveDragKind::SampleSlide)
            && previous.active_drag_kind == Some(WaveformActiveDragKind::SampleSlide)
    }
}
