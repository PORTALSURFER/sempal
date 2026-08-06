use radiant::{
    gui::types::{Rect, Vector2},
    widgets::{
        CanvasGestureEvent, CanvasPointer, DragHandleMessage, PointerButton, PointerModifiers,
        WidgetInput, WidgetOutput,
    },
};

use super::{
    WaveformActiveDragKind, WaveformEditFadeHandle, WaveformInteraction, WaveformSelectionEdge,
    WaveformSelectionKind, WaveformWidget,
    widget::{LiveSelectionPreview, LiveSelectionPreviewAnchor},
};

const SELECTION_CLICK_SLOP_PX: f32 = 2.0;
const LIVE_SELECTION_PREVIEW_PIXEL_STEP: f32 = 1.0;

impl WaveformWidget {
    pub(in crate::native_app::waveform) fn handle_waveform_input(
        &mut self,
        bounds: Rect,
        input: WidgetInput,
    ) -> Option<WidgetOutput> {
        if input.pointer_start_outside(bounds) {
            return None;
        }
        let event = self.gesture.handle_input(bounds, &input)?;
        let pointer_inside = event.pointer_is_inside(bounds);
        let has_loaded_sample = self.has_loaded_sample();
        if let Some(pointer) = self.active_drag_motion_pointer(&event) {
            self.common.state.hovered = pointer_inside;
            self.clear_waveform_hover();
            if !has_loaded_sample {
                return None;
            }
            return self.active_drag_motion_output(bounds, &event, pointer);
        }
        if let Some(pointer) = event.hover_pointer() {
            self.common.state.hovered = pointer_inside;
            if !has_loaded_sample {
                self.clear_waveform_hover();
                return None;
            }
            if !pointer_inside {
                self.clear_waveform_hover();
                return None;
            }
            self.hovered_edit_fade_outer_gain_handle =
                self.edit_fade_outer_gain_handle_at(bounds, pointer.position);
            if self.hovered_edit_fade_outer_gain_handle.is_some() {
                self.hovered_edit_fade_handle = None;
                self.hovered_selection_handle = None;
                self.hovered_edit_gain_handle = false;
                self.hovered_similar_section = None;
                self.hover_cursor_ratio = None;
                return None;
            }
            self.hovered_edit_fade_handle = self.edit_fade_handle_at(bounds, pointer.position);
            if self.hovered_edit_fade_handle.is_some() {
                self.hovered_edit_fade_outer_gain_handle = None;
                self.hovered_selection_handle = None;
                self.hovered_edit_gain_handle = false;
                self.hovered_similar_section = None;
                self.hover_cursor_ratio = None;
                return None;
            }
            self.hovered_edit_gain_handle = self.edit_gain_handle_at(bounds, pointer.position);
            if self.hovered_edit_gain_handle {
                self.hovered_edit_fade_outer_gain_handle = None;
                self.hovered_selection_handle = None;
                self.hovered_similar_section = None;
                self.hover_cursor_ratio = None;
                return None;
            }
            self.hovered_selection_handle =
                self.selection_handle_hover_at(bounds, pointer.position);
            if self.hovered_selection_handle.is_some() {
                self.hovered_similar_section = None;
                self.hover_cursor_ratio = None;
                return None;
            }
            self.hovered_similar_section = self.similar_section_at(bounds, pointer.position);
            if self.hovered_similar_section.is_some() {
                self.hovered_edit_fade_outer_gain_handle = None;
                self.hovered_edit_fade_handle = None;
                self.hovered_edit_gain_handle = false;
                self.hover_cursor_ratio = None;
                return None;
            }
            let visible_ratio = pointer.normalized_x();
            self.hover_cursor_ratio = self.absolute_ratio_for_visible(visible_ratio);
            return None;
        }
        if let Some((pointer, delta)) = event.wheel_pointer_delta_inside(bounds) {
            let expand_silence_margin =
                matches!(&input, WidgetInput::Wheel { modifiers, .. } if modifiers.shift);
            return has_loaded_sample.then(|| {
                WidgetOutput::typed(WaveformInteraction::Wheel {
                    delta,
                    anchor_ratio: pointer.normalized_x(),
                    expand_silence_margin,
                })
            });
        }
        if !has_loaded_sample {
            return None;
        }
        if let Some(output) = self.command_option_sample_slide_press_output(bounds, &event) {
            return Some(output);
        }
        if let Some(pointer) = event.press_pointer_inside(bounds, PointerButton::Primary) {
            return self.handle_primary_press(bounds, pointer);
        }
        if let Some(pointer) = event.double_click_pointer_inside(bounds, PointerButton::Primary) {
            return self.handle_primary_double_click(bounds, pointer);
        }
        if let Some(pointer) = event.press_pointer_inside(bounds, PointerButton::Secondary) {
            return self.handle_secondary_press(bounds, pointer);
        }
        if let Some(pointer) = event.press_pointer_inside(bounds, PointerButton::Auxiliary) {
            return Some(WidgetOutput::typed(WaveformInteraction::BeginPan {
                visible_ratio: pointer.normalized_x(),
            }));
        }
        if let Some(pointer) = event.release_pointer(PointerButton::Primary) {
            self.last_live_selection_update_visible_ratio = None;
            self.clear_live_selection_preview();
            if self.active_drag_kind == Some(WaveformActiveDragKind::PlaySelectionExport) {
                return Some(WidgetOutput::typed(
                    WaveformInteraction::DragPlaySelectionExport(DragHandleMessage::ended(
                        pointer.position,
                    )),
                ));
            }
            if self.active_drag_kind == Some(WaveformActiveDragKind::EditGain) {
                return Some(WidgetOutput::typed(WaveformInteraction::FinishEditGain {
                    pointer_y: pointer.position.y,
                }));
            }
            if matches!(
                self.active_drag_kind,
                Some(WaveformActiveDragKind::EditFadeOuterGain(_))
            ) {
                return Some(WidgetOutput::typed(
                    WaveformInteraction::FinishEditFadeOuterGain {
                        vertical_ratio: pointer.normalized_y(),
                    },
                ));
            }
            if self.active_drag_kind == Some(WaveformActiveDragKind::SampleSlide) {
                self.clear_sample_slide_preview();
                return Some(WidgetOutput::typed(
                    WaveformInteraction::FinishSampleSlide {
                        visible_ratio: pointer.normalized_x(),
                    },
                ));
            }
            if self.primary_release_finishes_drag() {
                return Some(WidgetOutput::typed(WaveformInteraction::FinishSelection {
                    visible_ratio: self.finish_selection_visible_ratio(&event, pointer),
                }));
            }
        }
        if let Some(pointer) = event.release_pointer(PointerButton::Secondary)
            && self.active_drag_kind == Some(WaveformActiveDragKind::EditGain)
        {
            self.last_live_selection_update_visible_ratio = None;
            self.clear_live_selection_preview();
            return Some(WidgetOutput::typed(WaveformInteraction::FinishEditGain {
                pointer_y: pointer.position.y,
            }));
        }
        if let Some(pointer) = event.release_pointer(PointerButton::Secondary)
            && matches!(
                self.active_drag_kind,
                Some(WaveformActiveDragKind::EditFadeOuterGain(_))
            )
        {
            self.last_live_selection_update_visible_ratio = None;
            self.clear_live_selection_preview();
            return Some(WidgetOutput::typed(
                WaveformInteraction::FinishEditFadeOuterGain {
                    vertical_ratio: pointer.normalized_y(),
                },
            ));
        }
        if let Some(pointer) = event.release_pointer(PointerButton::Secondary)
            && self.secondary_release_finishes_drag()
        {
            self.last_live_selection_update_visible_ratio = None;
            self.clear_live_selection_preview();
            return Some(WidgetOutput::typed(WaveformInteraction::FinishSelection {
                visible_ratio: self.finish_selection_visible_ratio(&event, pointer),
            }));
        }
        if let Some(pointer) = event.release_pointer(PointerButton::Auxiliary)
            && self.active_drag_kind == Some(WaveformActiveDragKind::Pan)
        {
            self.last_live_selection_update_visible_ratio = None;
            self.clear_live_selection_preview();
            return Some(WidgetOutput::typed(WaveformInteraction::FinishSelection {
                visible_ratio: pointer.normalized_x(),
            }));
        }
        None
    }

    fn active_drag_motion_pointer(&self, event: &CanvasGestureEvent) -> Option<CanvasPointer> {
        self.active_drag_kind?;
        match event {
            CanvasGestureEvent::Drag { pointer, .. } => Some(*pointer),
            CanvasGestureEvent::Hover { pointer, .. } => Some(*pointer),
            _ => None,
        }
    }

    fn clear_waveform_hover(&mut self) {
        self.hover_cursor_ratio = None;
        self.hovered_selection_handle = None;
        self.hovered_edit_fade_handle = None;
        self.hovered_edit_fade_outer_gain_handle = None;
        self.hovered_edit_gain_handle = false;
        self.hovered_similar_section = None;
    }

    fn active_drag_motion_output(
        &mut self,
        bounds: Rect,
        event: &CanvasGestureEvent,
        pointer: CanvasPointer,
    ) -> Option<WidgetOutput> {
        if self.active_drag_kind == Some(WaveformActiveDragKind::PlaySelectionExport) {
            return Some(WidgetOutput::typed(
                WaveformInteraction::DragPlaySelectionExport(DragHandleMessage::moved(
                    pointer.position,
                )),
            ));
        }
        if self.active_drag_kind == Some(WaveformActiveDragKind::EditGain) {
            return Some(WidgetOutput::typed(WaveformInteraction::UpdateEditGain {
                pointer_y: pointer.position.y,
            }));
        }
        if matches!(
            self.active_drag_kind,
            Some(WaveformActiveDragKind::EditFadeOuterGain(_))
        ) {
            return Some(WidgetOutput::typed(
                WaveformInteraction::UpdateEditFadeOuterGain {
                    vertical_ratio: pointer.normalized_y(),
                },
            ));
        }
        if self.active_drag_kind == Some(WaveformActiveDragKind::SampleSlide) {
            let visible_ratio =
                quantized_live_selection_visible_ratio(bounds, pointer.normalized_x());
            self.update_sample_slide_preview(visible_ratio);
            return Some(WidgetOutput::typed(
                WaveformInteraction::UpdateSampleSlide { visible_ratio },
            ));
        }
        if self.selection_drag_is_inside_click_slop(event) {
            self.live_selection_preview = None;
            return None;
        }
        let visible_ratio = quantized_live_selection_visible_ratio(bounds, pointer.normalized_x());
        if self.last_live_selection_update_visible_ratio == Some(visible_ratio) {
            return None;
        }
        self.last_live_selection_update_visible_ratio = Some(visible_ratio);
        self.update_live_selection_preview_for_active_drag(visible_ratio);
        if self.active_selection_drag_can_paint_live_preview() {
            return None;
        }
        Some(WidgetOutput::typed(WaveformInteraction::UpdateSelection {
            visible_ratio,
        }))
    }

    fn handle_primary_press(
        &mut self,
        bounds: Rect,
        pointer: CanvasPointer,
    ) -> Option<WidgetOutput> {
        let position = pointer.position;
        let visible_ratio = pointer.normalized_x();
        self.last_live_selection_update_visible_ratio = None;
        self.clear_live_selection_preview();
        self.clear_waveform_hover();
        if self.play_selection_export_handle_at(bounds, position) {
            return Some(WidgetOutput::typed(
                WaveformInteraction::DragPlaySelectionExport(DragHandleMessage::started(position)),
            ));
        }
        if let Some(handle) = self.edit_fade_outer_gain_handle_at(bounds, position) {
            return Some(WidgetOutput::typed(
                WaveformInteraction::BeginEditFadeOuterGain {
                    handle,
                    vertical_ratio: pointer.normalized_y(),
                },
            ));
        }
        if let Some(handle) = self.edit_fade_handle_at(bounds, position) {
            return Some(WidgetOutput::typed(WaveformInteraction::BeginEditFade {
                handle,
                visible_ratio,
            }));
        }
        if self.edit_gain_handle_at(bounds, position) {
            return Some(WidgetOutput::typed(WaveformInteraction::BeginEditGain {
                pointer_y: position.y,
            }));
        }
        if let Some(edge) =
            self.selection_resize_handle_at(bounds, position, WaveformSelectionKind::Edit)
        {
            self.begin_live_selection_preview(WaveformSelectionKind::Edit, visible_ratio);
            return Some(WidgetOutput::typed(
                WaveformInteraction::BeginSelectionResize {
                    kind: WaveformSelectionKind::Edit,
                    edge,
                    visible_ratio,
                },
            ));
        }
        if let Some(edge) =
            self.selection_resize_handle_at(bounds, position, WaveformSelectionKind::Play)
        {
            self.begin_live_selection_preview(WaveformSelectionKind::Play, visible_ratio);
            return Some(WidgetOutput::typed(
                WaveformInteraction::BeginSelectionResize {
                    kind: WaveformSelectionKind::Play,
                    edge,
                    visible_ratio,
                },
            ));
        }
        if self.selection_move_handle_at(bounds, position, WaveformSelectionKind::Play) {
            self.begin_live_selection_preview(WaveformSelectionKind::Play, visible_ratio);
            return Some(WidgetOutput::typed(
                WaveformInteraction::BeginSelectionMove {
                    kind: WaveformSelectionKind::Play,
                    visible_ratio,
                },
            ));
        }
        if self.selection_move_handle_at(bounds, position, WaveformSelectionKind::Edit) {
            self.begin_live_selection_preview(WaveformSelectionKind::Edit, visible_ratio);
            return Some(WidgetOutput::typed(
                WaveformInteraction::BeginSelectionMove {
                    kind: WaveformSelectionKind::Edit,
                    visible_ratio,
                },
            ));
        }
        if let Some(selection) = self.similar_section_at(bounds, position) {
            return Some(WidgetOutput::typed(
                WaveformInteraction::SelectSimilarSection { selection },
            ));
        }
        self.begin_live_selection_preview(WaveformSelectionKind::Play, visible_ratio);
        Some(WidgetOutput::typed(WaveformInteraction::BeginSelection {
            kind: WaveformSelectionKind::Play,
            visible_ratio,
        }))
    }

    fn command_option_sample_slide_press_output(
        &mut self,
        bounds: Rect,
        event: &CanvasGestureEvent,
    ) -> Option<WidgetOutput> {
        let CanvasGestureEvent::Press {
            pointer,
            button: PointerButton::Primary,
            modifiers,
            ..
        } = event
        else {
            return None;
        };
        if !pointer.is_inside(bounds) || !sample_slide_modifiers(*modifiers) {
            return None;
        }
        let visible_ratio = pointer.normalized_x();
        self.last_live_selection_update_visible_ratio = None;
        self.clear_live_selection_preview();
        self.clear_waveform_hover();
        self.begin_sample_slide_preview(visible_ratio);
        Some(WidgetOutput::typed(WaveformInteraction::BeginSampleSlide {
            visible_ratio,
        }))
    }

    fn handle_primary_double_click(
        &self,
        bounds: Rect,
        pointer: CanvasPointer,
    ) -> Option<WidgetOutput> {
        let position = pointer.position;
        if let Some(
            handle @ (WaveformEditFadeHandle::InOuterStart | WaveformEditFadeHandle::OutOuterEnd),
        ) = self.edit_fade_handle_at(bounds, position)
        {
            return Some(WidgetOutput::typed(
                WaveformInteraction::ClearEditFadeSilence { handle },
            ));
        }
        None
    }

    fn handle_secondary_press(
        &mut self,
        bounds: Rect,
        pointer: CanvasPointer,
    ) -> Option<WidgetOutput> {
        let position = pointer.position;
        let visible_ratio = pointer.normalized_x();
        self.last_live_selection_update_visible_ratio = None;
        self.clear_live_selection_preview();
        self.clear_waveform_hover();
        if let Some(handle) = self.edit_fade_outer_gain_handle_at(bounds, position) {
            return Some(WidgetOutput::typed(
                WaveformInteraction::BeginEditFadeOuterGain {
                    handle,
                    vertical_ratio: pointer.normalized_y(),
                },
            ));
        }
        if let Some(handle) = self.edit_fade_handle_at(bounds, position) {
            return Some(WidgetOutput::typed(WaveformInteraction::BeginEditFade {
                handle,
                visible_ratio,
            }));
        }
        if self.edit_gain_handle_at(bounds, position) {
            return Some(WidgetOutput::typed(WaveformInteraction::BeginEditGain {
                pointer_y: position.y,
            }));
        }
        if let Some(edge) =
            self.selection_resize_handle_at(bounds, position, WaveformSelectionKind::Edit)
        {
            self.begin_live_selection_preview(WaveformSelectionKind::Edit, visible_ratio);
            return Some(WidgetOutput::typed(
                WaveformInteraction::BeginSelectionResize {
                    kind: WaveformSelectionKind::Edit,
                    edge,
                    visible_ratio,
                },
            ));
        }
        if self.selection_move_handle_at(bounds, position, WaveformSelectionKind::Edit) {
            self.begin_live_selection_preview(WaveformSelectionKind::Edit, visible_ratio);
            return Some(WidgetOutput::typed(
                WaveformInteraction::BeginSelectionMove {
                    kind: WaveformSelectionKind::Edit,
                    visible_ratio,
                },
            ));
        }
        if let Some(selection) = self.similar_section_at(bounds, position) {
            return Some(WidgetOutput::typed(
                WaveformInteraction::SelectSimilarSection { selection },
            ));
        }
        self.begin_live_selection_preview(WaveformSelectionKind::Edit, visible_ratio);
        Some(WidgetOutput::typed(WaveformInteraction::BeginSelection {
            kind: WaveformSelectionKind::Edit,
            visible_ratio,
        }))
    }

    fn primary_release_finishes_drag(&self) -> bool {
        self.active_drag_kind
            == Some(WaveformActiveDragKind::Selection(
                WaveformSelectionKind::Play,
            ))
            || matches!(
                self.active_drag_kind,
                Some(
                    WaveformActiveDragKind::EditFade(_)
                        | WaveformActiveDragKind::SelectionResize(_, _)
                        | WaveformActiveDragKind::SelectionMove(_)
                )
            )
    }

    fn secondary_release_finishes_drag(&self) -> bool {
        self.active_drag_kind
            == Some(WaveformActiveDragKind::Selection(
                WaveformSelectionKind::Edit,
            ))
            || matches!(
                self.active_drag_kind,
                Some(
                    WaveformActiveDragKind::EditFade(_)
                        | WaveformActiveDragKind::SelectionResize(WaveformSelectionKind::Edit, _)
                        | WaveformActiveDragKind::SelectionMove(WaveformSelectionKind::Edit)
                )
            )
    }

    fn selection_drag_is_inside_click_slop(&self, event: &CanvasGestureEvent) -> bool {
        if !self.active_drag_uses_click_slop() {
            return false;
        }
        matches!(
            event,
            CanvasGestureEvent::Drag { delta, .. }
                if horizontal_delta_inside_click_slop(*delta)
        )
    }

    fn finish_selection_visible_ratio(
        &self,
        event: &CanvasGestureEvent,
        fallback: CanvasPointer,
    ) -> f32 {
        if !self.active_drag_uses_click_slop() {
            return fallback.normalized_x();
        }
        match event {
            CanvasGestureEvent::Release { origin, delta, .. }
                if horizontal_delta_inside_click_slop(*delta) =>
            {
                origin.normalized_x()
            }
            _ => fallback.normalized_x(),
        }
    }

    fn active_drag_uses_click_slop(&self) -> bool {
        matches!(
            self.active_drag_kind,
            Some(WaveformActiveDragKind::Selection(_))
        )
    }

    fn active_selection_drag_can_paint_live_preview(&self) -> bool {
        matches!(
            self.active_drag_kind,
            Some(WaveformActiveDragKind::Selection(_))
        )
    }

    fn begin_live_selection_preview(&mut self, kind: WaveformSelectionKind, visible_ratio: f32) {
        let baseline = self.selection_for_kind(kind);
        self.live_selection_preview_anchor = Some(LiveSelectionPreviewAnchor {
            kind,
            visible_ratio,
            baseline,
        });
        self.live_selection_preview =
            baseline.map(|selection| LiveSelectionPreview { kind, selection });
    }

    fn clear_live_selection_preview(&mut self) {
        self.live_selection_preview_anchor = None;
        self.live_selection_preview = None;
    }

    fn begin_sample_slide_preview(&mut self, visible_ratio: f32) {
        self.active_drag_kind = Some(WaveformActiveDragKind::SampleSlide);
        self.live_sample_slide_anchor_visible_ratio = Some(visible_ratio);
        self.sample_slide_frame_offset = Some(0);
    }

    fn update_sample_slide_preview(&mut self, visible_ratio: f32) {
        let Some(anchor) = self.live_sample_slide_anchor_visible_ratio else {
            return;
        };
        self.sample_slide_frame_offset = Some(sample_slide_frame_offset(
            anchor,
            visible_ratio,
            self.viewport.visible_items(),
        ));
    }

    fn clear_sample_slide_preview(&mut self) {
        self.live_sample_slide_anchor_visible_ratio = None;
        self.sample_slide_frame_offset = None;
    }

    fn update_live_selection_preview_for_active_drag(&mut self, visible_ratio: f32) {
        let Some(active_kind) = self.active_drag_kind else {
            return;
        };
        let Some(selection) = self.preview_selection_for_active_drag(active_kind, visible_ratio)
        else {
            return;
        };
        let Some(kind) = active_kind.selection_kind() else {
            return;
        };
        self.live_selection_preview = Some(LiveSelectionPreview { kind, selection });
    }

    fn preview_selection_for_active_drag(
        &mut self,
        active_kind: WaveformActiveDragKind,
        visible_ratio: f32,
    ) -> Option<wavecrate::selection::SelectionRange> {
        match active_kind {
            WaveformActiveDragKind::Selection(kind) => {
                self.preview_created_selection(kind, visible_ratio)
            }
            WaveformActiveDragKind::SelectionResize(kind, edge) => {
                self.preview_resized_selection(kind, edge, visible_ratio)
            }
            WaveformActiveDragKind::SelectionMove(kind) => {
                self.preview_moved_selection(kind, visible_ratio)
            }
            _ => None,
        }
    }

    fn preview_created_selection(
        &mut self,
        kind: WaveformSelectionKind,
        visible_ratio: f32,
    ) -> Option<wavecrate::selection::SelectionRange> {
        let Some(anchor) = self.live_selection_preview_anchor else {
            return None;
        };
        if anchor.kind != kind {
            self.clear_live_selection_preview();
            return None;
        }
        let anchor_ratio = self.absolute_ratio_for_visible(anchor.visible_ratio)?;
        let current_ratio = self.absolute_ratio_for_visible(visible_ratio)?;
        Some(wavecrate::selection::SelectionRange::new(
            anchor_ratio,
            current_ratio,
        ))
    }

    fn preview_resized_selection(
        &self,
        kind: WaveformSelectionKind,
        edge: WaveformSelectionEdge,
        visible_ratio: f32,
    ) -> Option<wavecrate::selection::SelectionRange> {
        let selection = self
            .live_selection_preview_anchor
            .filter(|anchor| anchor.kind == kind)
            .and_then(|anchor| anchor.baseline)
            .or_else(|| self.selection_for_kind(kind))?;
        let ratio = self.absolute_ratio_for_visible(visible_ratio)?;
        let fixed_ratio = match edge {
            WaveformSelectionEdge::Start => selection.end(),
            WaveformSelectionEdge::End => selection.start(),
        };
        let ratio = if self.bpm_snap_enabled {
            super::snap_resize_ratio_to_whole_bpm(
                fixed_ratio,
                ratio,
                self.file.frames as f32 / self.file.sample_rate.max(1) as f32,
                self.beat_guide_count,
                !self.allows_out_of_bounds_selection_preview(),
            )
        } else {
            ratio
        };
        Some(self.selection_range_for_current_domain(fixed_ratio, ratio))
    }

    fn preview_moved_selection(
        &self,
        kind: WaveformSelectionKind,
        visible_ratio: f32,
    ) -> Option<wavecrate::selection::SelectionRange> {
        let anchor = self.live_selection_preview_anchor?;
        if anchor.kind != kind {
            return None;
        }
        let selection = anchor.baseline.or_else(|| self.selection_for_kind(kind))?;
        let anchor_ratio = self.absolute_ratio_for_visible(anchor.visible_ratio)?;
        let ratio = self.absolute_ratio_for_visible(visible_ratio)?;
        let delta = ratio - anchor_ratio;
        Some(if self.allows_out_of_bounds_selection_preview() {
            selection.shift_unclamped(delta)
        } else {
            selection.shift(delta)
        })
    }

    fn selection_for_kind(
        &self,
        kind: WaveformSelectionKind,
    ) -> Option<wavecrate::selection::SelectionRange> {
        match kind {
            WaveformSelectionKind::Play => self.play_selection,
            WaveformSelectionKind::Edit => self.edit_selection,
        }
    }

    fn selection_range_for_current_domain(
        &self,
        start: f32,
        end: f32,
    ) -> wavecrate::selection::SelectionRange {
        if self.allows_out_of_bounds_selection_preview() {
            wavecrate::selection::SelectionRange::new_unclamped(start, end)
        } else {
            wavecrate::selection::SelectionRange::new(start, end)
        }
    }

    fn allows_out_of_bounds_selection_preview(&self) -> bool {
        self.viewport.extends_beyond_audio(self.file.frames)
    }
}

impl WaveformActiveDragKind {
    pub(super) fn selection_kind(self) -> Option<WaveformSelectionKind> {
        match self {
            Self::Selection(kind) | Self::SelectionResize(kind, _) | Self::SelectionMove(kind) => {
                Some(kind)
            }
            _ => None,
        }
    }
}

fn horizontal_delta_inside_click_slop(delta: Vector2) -> bool {
    delta.x.abs() <= SELECTION_CLICK_SLOP_PX
}

fn quantized_live_selection_visible_ratio(bounds: Rect, visible_ratio: f32) -> f32 {
    if !visible_ratio.is_finite() {
        return visible_ratio;
    }
    let steps = (bounds.width() / LIVE_SELECTION_PREVIEW_PIXEL_STEP)
        .round()
        .max(1.0);
    (visible_ratio.clamp(0.0, 1.0) * steps).round() / steps
}

fn sample_slide_modifiers(modifiers: PointerModifiers) -> bool {
    modifiers.command && modifiers.alt && !modifiers.shift
}

fn sample_slide_frame_offset(
    anchor_visible_ratio: f32,
    visible_ratio: f32,
    visible_frames: usize,
) -> i64 {
    if !anchor_visible_ratio.is_finite() || !visible_ratio.is_finite() {
        return 0;
    }
    ((visible_ratio - anchor_visible_ratio) * visible_frames.max(1) as f32).round() as i64
}
