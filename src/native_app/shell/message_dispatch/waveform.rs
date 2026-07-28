use radiant::prelude as ui;
use std::{path::PathBuf, time::Instant};
use wavecrate::selection::SelectionRange;

use crate::native_app::app::{
    ClipboardHandoffTarget, GuiMessage, NativeAppState, WaveformActiveDragKind,
    WaveformContextMenu, WaveformEditSelectionSnapshot, WaveformInteraction,
    WaveformPlaySelectionSnapshot, WaveformSelectionKind, emit_gui_action,
};
use crate::native_app::ui::ids as widget_ids;
use crate::native_app::waveform::PlaymarkLabelMessage;

pub(in crate::native_app) const PLAY_SELECTION_TRANSACTION_LABEL: &str =
    "Change play mark selection";
const EDIT_FADE_TRANSACTION_LABEL: &str = "Waveform fade";
const EDIT_GAIN_TRANSACTION_LABEL: &str = "Editmark volume";
const EDIT_RESIZE_TRANSACTION_LABEL: &str = "Editmark resize";
const EDIT_MOVE_TRANSACTION_LABEL: &str = "Editmark move";

impl NativeAppState {
    pub(super) fn apply_playmark_label_message(
        &mut self,
        message: PlaymarkLabelMessage,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        match message {
            PlaymarkLabelMessage::BeginEdit => {
                if self.waveform.current.begin_playmark_label_edit(
                    self.ui.chrome.beat_guides_enabled,
                    self.ui.chrome.beat_guide_count,
                ) {
                    context.focus(widget_ids::WAVEFORM_PLAYMARK_LABEL_ID);
                    context.repaint(ui::RepaintScope::Surface);
                }
            }
            PlaymarkLabelMessage::Changed(draft) => {
                self.waveform.current.update_playmark_label_draft(draft);
            }
            PlaymarkLabelMessage::Commit(draft) => {
                if self
                    .waveform
                    .current
                    .playmark_label_commit_is_unchanged(&draft)
                {
                    self.waveform.current.close_playmark_label_editor();
                    context.clear_focus();
                    context.repaint(ui::RepaintScope::Surface);
                    return;
                }
                match self
                    .waveform
                    .current
                    .playmark_frame_range_from_text(&draft, self.ui.chrome.beat_guide_count)
                {
                    Ok((start_frame, end_frame)) => {
                        self.waveform.current.close_playmark_label_editor();
                        self.apply_waveform_message(
                            WaveformInteraction::SetPlaySelectionFrameRange {
                                start_frame,
                                end_frame,
                            },
                            context,
                        );
                        context.clear_focus();
                        context.repaint(ui::RepaintScope::Surface);
                    }
                    Err(error) => {
                        self.ui.status.sample = error.clone();
                        self.waveform.current.set_playmark_label_error(error);
                        context.focus(widget_ids::WAVEFORM_PLAYMARK_LABEL_ID);
                    }
                }
            }
            PlaymarkLabelMessage::Cancel => {
                self.waveform.current.close_playmark_label_editor();
                context.clear_focus();
                context.repaint(ui::RepaintScope::Surface);
            }
        }
    }

    pub(super) fn apply_waveform_message(
        &mut self,
        message: WaveformInteraction,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if let WaveformInteraction::DragLoadedSample(drag) = message {
            self.ui.browser_interaction.clipboard_handoff_target =
                ClipboardHandoffTarget::BrowserFiles;
            self.drag_loaded_waveform_sample(drag, context);
            return;
        }
        self.ui.browser_interaction.clipboard_handoff_target =
            ClipboardHandoffTarget::WaveformSelection;
        let started_at = Instant::now();
        let action = waveform_interaction_action(&message);
        let active_drag = self.waveform.current.active_drag_kind();
        let clipping_signature_before = waveform_clipping_signature(self);
        let play_selection_before = self.play_selection_transaction_begin_snapshot(&message);
        let edit_selection_before = self.edit_selection_transaction_begin_snapshot(&message);
        let edit_fade_before = self.edit_fade_transaction_begin_snapshot(&message);
        let harvest_mark_before = waveform_interaction_can_finish_mark_change(&message)
            .then(|| WaveformHarvestMarkSnapshot::from_state(self))
            .flatten();
        if let WaveformInteraction::DragPlaySelectionExport(drag) = message
            && !self.drag_waveform_play_selection(drag, context)
        {
            return;
        }
        self.waveform.current.apply_interaction_with_bpm_snap(
            message,
            crate::native_app::waveform::WaveformBpmSnapSettings {
                enabled: self.ui.chrome.bpm_snap_enabled,
                beat_count: self.ui.chrome.beat_guide_count,
            },
        );
        if waveform_clipping_signature(self) != clipping_signature_before {
            self.ui.chrome.overflow_fades.arm();
            context.request_paint_only();
        }
        self.queue_waveform_detail_refinement(context);
        if matches!(message, WaveformInteraction::FinishSampleSlide { .. })
            && let Some(frame_offset) = self
                .waveform
                .current
                .take_pending_sample_slide_frame_offset()
        {
            self.request_slide_loaded_sample_audio(frame_offset, context);
        }
        self.mark_harvest_touched_after_waveform_mark_change(
            &message,
            harvest_mark_before,
            context,
        );
        if let Some(before) = play_selection_before {
            self.waveform.pending_play_selection_transaction =
                (matches!(
                    message,
                    WaveformInteraction::SetPlaySelectionFrameRange { .. }
                ) || play_selection_drag_active(self.waveform.current.active_drag_kind()))
                .then_some(before);
        }
        if let Some(before) = edit_selection_before {
            self.waveform.pending_edit_selection_transaction =
                edit_selection_drag_active(self.waveform.current.active_drag_kind())
                    .then_some(before);
        }
        if let Some(before) = edit_fade_before {
            self.waveform.pending_edit_fade_transaction = Some(before);
        }
        if waveform_interaction_updates_edit_selection(&message, active_drag) {
            self.sync_edit_fade_audio_state();
        }
        if waveform_interaction_updates_play_selection(&message, active_drag) {
            if waveform_interaction_finishes_play_selection_update(&message) {
                self.retarget_playback_to_play_selection_now();
            } else {
                self.schedule_play_selection_playback_retarget();
            }
        }
        if play_selection_transaction_finishes(&message, active_drag) {
            self.register_finished_play_selection_transaction();
        }
        if edit_selection_transaction_finishes(&message, active_drag) {
            let label = edit_selection_transaction_label(active_drag);
            self.register_finished_edit_selection_transaction(label);
        }
        if edit_fade_transaction_finishes(&message, active_drag) {
            let label =
                edit_fade_transaction_label(&message).unwrap_or(EDIT_FADE_TRANSACTION_LABEL);
            self.register_finished_edit_fade_transaction(label);
        }
        if let Some(action) = action {
            emit_gui_action(action, Some("waveform"), None, "applied", started_at, None);
        }
        if let Some(start_ratio) = self.waveform.current.take_pending_playback_start() {
            self.maybe_open_audio_player(context);
            self.play_waveform_from_ratio(start_ratio, context);
        }
    }

    pub(super) fn finish_waveform_detail_refinement(
        &mut self,
        result: crate::native_app::waveform::WaveformDetailResult,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.waveform.current.apply_detail_result(result);
        self.queue_waveform_detail_refinement(context);
    }

    fn queue_waveform_detail_refinement(&mut self, context: &mut ui::UiUpdateContext<GuiMessage>) {
        let Some(key) = self.waveform.current.desired_detail_key() else {
            return;
        };
        self.waveform.current.mark_detail_pending(key.clone());
        context.business().background("gui-waveform-detail").run(
            move |_| crate::native_app::waveform::load_wav_detail_summary(key),
            GuiMessage::WaveformDetailRefined,
        );
    }

    fn play_selection_transaction_begin_snapshot(
        &self,
        interaction: &WaveformInteraction,
    ) -> Option<WaveformPlaySelectionSnapshot> {
        let begins_play_selection_change = matches!(
            interaction,
            WaveformInteraction::BeginSelection {
                kind: WaveformSelectionKind::Play,
                ..
            } | WaveformInteraction::BeginSelectionResize {
                kind: WaveformSelectionKind::Play,
                ..
            } | WaveformInteraction::BeginSelectionMove {
                kind: WaveformSelectionKind::Play,
                ..
            } | WaveformInteraction::SlidePlaySelection { .. }
                | WaveformInteraction::SetPlaySelectionFrameRange { .. }
        );
        begins_play_selection_change
            .then(|| WaveformPlaySelectionSnapshot::from_waveform(&self.waveform.current))
    }

    fn edit_fade_transaction_begin_snapshot(
        &self,
        interaction: &WaveformInteraction,
    ) -> Option<WaveformEditSelectionSnapshot> {
        let begins_edit_fade_change = matches!(
            interaction,
            WaveformInteraction::BeginEditFade { .. }
                | WaveformInteraction::BeginEditFadeOuterGain { .. }
                | WaveformInteraction::BeginEditGain { .. }
                | WaveformInteraction::ClearEditFadeSilence { .. }
        );
        begins_edit_fade_change
            .then(|| WaveformEditSelectionSnapshot::from_waveform(&self.waveform.current))
    }

    fn edit_selection_transaction_begin_snapshot(
        &self,
        interaction: &WaveformInteraction,
    ) -> Option<WaveformEditSelectionSnapshot> {
        let begins_edit_selection_range_change = matches!(
            interaction,
            WaveformInteraction::BeginSelectionResize {
                kind: WaveformSelectionKind::Edit,
                ..
            } | WaveformInteraction::BeginSelectionMove {
                kind: WaveformSelectionKind::Edit,
                ..
            }
        );
        begins_edit_selection_range_change
            .then(|| WaveformEditSelectionSnapshot::from_waveform(&self.waveform.current))
    }

    fn register_finished_play_selection_transaction(&mut self) {
        let Some(before) = self.waveform.pending_play_selection_transaction.take() else {
            return;
        };
        let after = WaveformPlaySelectionSnapshot::from_waveform(&self.waveform.current);
        if before.path != after.path || before == after {
            return;
        }
        let undo_snapshot = before.clone();
        let redo_snapshot = after;
        self.register_transaction_action(
            PLAY_SELECTION_TRANSACTION_LABEL,
            move |transaction| transaction.restore_play_selection(undo_snapshot.clone()),
            move |transaction| transaction.restore_play_selection(redo_snapshot.clone()),
        );
    }

    fn register_finished_edit_fade_transaction(&mut self, label: &'static str) {
        let Some(before) = self.waveform.pending_edit_fade_transaction.take() else {
            return;
        };
        let after = WaveformEditSelectionSnapshot::from_waveform(&self.waveform.current);
        if before.path != after.path || before == after {
            return;
        }
        let undo_snapshot = before.clone();
        let redo_snapshot = after;
        self.register_transaction_action(
            label,
            move |transaction| transaction.restore_edit_selection(undo_snapshot.clone()),
            move |transaction| transaction.restore_edit_selection(redo_snapshot.clone()),
        );
    }

    fn register_finished_edit_selection_transaction(&mut self, label: &'static str) {
        let Some(before) = self.waveform.pending_edit_selection_transaction.take() else {
            return;
        };
        let after = WaveformEditSelectionSnapshot::from_waveform(&self.waveform.current);
        if before.path != after.path || before == after {
            return;
        }
        let undo_snapshot = before.clone();
        let redo_snapshot = after;
        self.register_transaction_action(
            label,
            move |transaction| transaction.restore_edit_selection(undo_snapshot.clone()),
            move |transaction| transaction.restore_edit_selection(redo_snapshot.clone()),
        );
    }

    pub(in crate::native_app) fn open_play_selection_context_menu_from_shortcut(
        &mut self,
        current_pointer_position: Option<ui::Point>,
    ) {
        let Some(position) = current_pointer_position
            .filter(|position| position.x.is_finite() && position.y.is_finite())
            .or_else(|| self.waveform.current.play_selection_context_menu_anchor())
        else {
            self.ui.status.sample = String::from("Set a playmark selection first");
            return;
        };
        self.open_play_selection_context_menu(position);
    }

    fn open_play_selection_context_menu(&mut self, position: ui::Point) {
        if !self
            .waveform
            .current
            .play_selection()
            .is_some_and(|selection| selection.width() > 0.0)
        {
            return;
        }
        self.ui.browser_interaction.context_menu = None;
        let loaded_path = self.waveform.current.path();
        self.ui.browser_interaction.waveform_context_menu = Some(WaveformContextMenu {
            anchor: position,
            title: String::from("Playmark Selection"),
            extract_to_harvest_destination: self
                .playmark_harvest_destination_action_available(&loaded_path),
        });
        emit_gui_action(
            "waveform.playmark_context_menu.open",
            Some("waveform"),
            None,
            "opened",
            Instant::now(),
            None,
        );
    }

    fn playmark_harvest_destination_action_available(&self, path: &std::path::Path) -> bool {
        let source_known = self
            .library
            .folder_browser
            .sample_source_for_file_path(path)
            .is_some();
        source_known
            && (self.library.folder_browser.harvest_filter().is_some()
                || self
                    .library
                    .folder_browser
                    .path_is_in_protected_source(path))
    }

    fn mark_harvest_touched_after_waveform_mark_change(
        &mut self,
        interaction: &WaveformInteraction,
        before: Option<WaveformHarvestMarkSnapshot>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let Some(before) = before else {
            return;
        };
        let Some(after) = WaveformHarvestMarkSnapshot::from_state(self) else {
            return;
        };
        if (before.path == after.path && before.marks_changed(&after))
            || matches!(interaction, WaveformInteraction::FinishSelection { .. })
        {
            self.schedule_harvest_touched_for_path(&after.path, context);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct WaveformClippingSignature {
    fully_zoomed_out: bool,
    offset_fraction_bits: u32,
    visible_fraction_bits: u32,
}

fn waveform_clipping_signature(state: &NativeAppState) -> WaveformClippingSignature {
    let waveform = &state.waveform.current;
    WaveformClippingSignature {
        fully_zoomed_out: waveform.fully_zoomed_out(),
        offset_fraction_bits: waveform.offset_fraction().to_bits(),
        visible_fraction_bits: waveform.visible_fraction().to_bits(),
    }
}

#[derive(Clone, Debug, PartialEq)]
struct WaveformHarvestMarkSnapshot {
    path: PathBuf,
    play_selection: Option<SelectionRange>,
    edit_selection: Option<SelectionRange>,
}

impl WaveformHarvestMarkSnapshot {
    fn from_state(state: &NativeAppState) -> Option<Self> {
        state.waveform.current.has_loaded_sample().then(|| Self {
            path: state.waveform.current.path(),
            play_selection: state.waveform.current.play_selection(),
            edit_selection: state.waveform.current.edit_selection(),
        })
    }

    fn marks_changed(&self, other: &Self) -> bool {
        self.play_selection != other.play_selection || self.edit_selection != other.edit_selection
    }
}

fn waveform_interaction_updates_play_selection(
    interaction: &WaveformInteraction,
    active_drag: Option<WaveformActiveDragKind>,
) -> bool {
    if !matches!(
        interaction,
        WaveformInteraction::UpdateSelection { .. }
            | WaveformInteraction::FinishSelection { .. }
            | WaveformInteraction::SlidePlaySelection { .. }
            | WaveformInteraction::SetPlaySelectionFrameRange { .. }
    ) {
        return false;
    }
    if matches!(interaction, WaveformInteraction::SlidePlaySelection { .. }) {
        return true;
    }
    if matches!(
        interaction,
        WaveformInteraction::SetPlaySelectionFrameRange { .. }
    ) {
        return true;
    }
    match active_drag {
        Some(WaveformActiveDragKind::Selection(WaveformSelectionKind::Play)) => {
            matches!(interaction, WaveformInteraction::FinishSelection { .. })
        }
        Some(
            WaveformActiveDragKind::SelectionResize(WaveformSelectionKind::Play, _)
            | WaveformActiveDragKind::SelectionMove(WaveformSelectionKind::Play),
        ) => true,
        _ => false,
    }
}

fn waveform_interaction_finishes_play_selection_update(interaction: &WaveformInteraction) -> bool {
    matches!(
        interaction,
        WaveformInteraction::FinishSelection { .. }
            | WaveformInteraction::SlidePlaySelection { .. }
            | WaveformInteraction::SetPlaySelectionFrameRange { .. }
    )
}

fn waveform_interaction_updates_edit_selection(
    interaction: &WaveformInteraction,
    active_drag: Option<WaveformActiveDragKind>,
) -> bool {
    match interaction {
        WaveformInteraction::BeginSelection {
            kind: WaveformSelectionKind::Edit,
            ..
        }
        | WaveformInteraction::BeginSelectionResize {
            kind: WaveformSelectionKind::Edit,
            ..
        }
        | WaveformInteraction::BeginSelectionMove {
            kind: WaveformSelectionKind::Edit,
            ..
        }
        | WaveformInteraction::BeginEditFade { .. }
        | WaveformInteraction::UpdateEditFadeOuterGain { .. }
        | WaveformInteraction::FinishEditFadeOuterGain { .. }
        | WaveformInteraction::UpdateEditGain { .. }
        | WaveformInteraction::FinishEditGain { .. }
        | WaveformInteraction::ClearEditFadeSilence { .. }
        | WaveformInteraction::SelectSimilarSection { .. } => true,
        WaveformInteraction::SlidePlaySelection { .. } => false,
        WaveformInteraction::UpdateSelection { .. }
        | WaveformInteraction::FinishSelection { .. } => {
            matches!(
                active_drag,
                Some(
                    WaveformActiveDragKind::Selection(WaveformSelectionKind::Edit)
                        | WaveformActiveDragKind::SelectionResize(WaveformSelectionKind::Edit, _)
                        | WaveformActiveDragKind::SelectionMove(WaveformSelectionKind::Edit)
                        | WaveformActiveDragKind::EditFade(_)
                )
            )
        }
        _ => false,
    }
}

fn waveform_interaction_can_finish_mark_change(interaction: &WaveformInteraction) -> bool {
    matches!(
        interaction,
        WaveformInteraction::FinishSelection { .. }
            | WaveformInteraction::SelectSimilarSection { .. }
            | WaveformInteraction::SlidePlaySelection { .. }
            | WaveformInteraction::SetPlaySelectionFrameRange { .. }
            | WaveformInteraction::ClearEditFadeSilence { .. }
            | WaveformInteraction::FinishEditFadeOuterGain { .. }
            | WaveformInteraction::FinishEditGain { .. }
    )
}

fn play_selection_transaction_finishes(
    interaction: &WaveformInteraction,
    active_drag: Option<WaveformActiveDragKind>,
) -> bool {
    matches!(
        interaction,
        WaveformInteraction::SlidePlaySelection { .. }
            | WaveformInteraction::SetPlaySelectionFrameRange { .. }
    ) || (matches!(interaction, WaveformInteraction::FinishSelection { .. })
        && play_selection_drag_active(active_drag))
}

fn play_selection_drag_active(active_drag: Option<WaveformActiveDragKind>) -> bool {
    matches!(
        active_drag,
        Some(WaveformActiveDragKind::Selection(
            WaveformSelectionKind::Play
        )) | Some(WaveformActiveDragKind::SelectionResize(
            WaveformSelectionKind::Play,
            _
        )) | Some(WaveformActiveDragKind::SelectionMove(
            WaveformSelectionKind::Play
        ))
    )
}

fn edit_selection_drag_active(active_drag: Option<WaveformActiveDragKind>) -> bool {
    matches!(
        active_drag,
        Some(WaveformActiveDragKind::SelectionResize(
            WaveformSelectionKind::Edit,
            _
        )) | Some(WaveformActiveDragKind::SelectionMove(
            WaveformSelectionKind::Edit
        ))
    )
}

fn edit_selection_transaction_finishes(
    interaction: &WaveformInteraction,
    active_drag: Option<WaveformActiveDragKind>,
) -> bool {
    matches!(interaction, WaveformInteraction::FinishSelection { .. })
        && edit_selection_drag_active(active_drag)
}

fn edit_selection_transaction_label(active_drag: Option<WaveformActiveDragKind>) -> &'static str {
    match active_drag {
        Some(WaveformActiveDragKind::SelectionMove(WaveformSelectionKind::Edit)) => {
            EDIT_MOVE_TRANSACTION_LABEL
        }
        _ => EDIT_RESIZE_TRANSACTION_LABEL,
    }
}

fn edit_fade_transaction_finishes(
    interaction: &WaveformInteraction,
    active_drag: Option<WaveformActiveDragKind>,
) -> bool {
    matches!(
        interaction,
        WaveformInteraction::ClearEditFadeSilence { .. }
            | WaveformInteraction::FinishEditFadeOuterGain { .. }
    ) || (matches!(interaction, WaveformInteraction::FinishSelection { .. })
        && matches!(active_drag, Some(WaveformActiveDragKind::EditFade(_))))
        || (matches!(interaction, WaveformInteraction::FinishEditGain { .. })
            && active_drag == Some(WaveformActiveDragKind::EditGain))
}

fn edit_fade_transaction_label(interaction: &WaveformInteraction) -> Option<&'static str> {
    match interaction {
        WaveformInteraction::FinishEditGain { .. } => Some(EDIT_GAIN_TRANSACTION_LABEL),
        WaveformInteraction::ClearEditFadeSilence { .. }
        | WaveformInteraction::FinishEditFadeOuterGain { .. }
        | WaveformInteraction::FinishSelection { .. } => Some(EDIT_FADE_TRANSACTION_LABEL),
        _ => None,
    }
}

fn waveform_interaction_action(interaction: &WaveformInteraction) -> Option<&'static str> {
    match interaction {
        WaveformInteraction::Wheel { .. } => Some("waveform.zoom_wheel"),
        WaveformInteraction::ZoomToPlaySelection => Some("waveform.zoom_to_play_selection"),
        WaveformInteraction::SlidePlaySelection { .. } => Some("waveform.playmark.slide"),
        WaveformInteraction::SetPlaySelectionFrameRange { .. } => {
            Some("waveform.playmark.length_commit")
        }
        WaveformInteraction::ZoomFull => Some("waveform.zoom_full"),
        WaveformInteraction::ScrollTo { .. } => Some("waveform.scroll"),
        WaveformInteraction::BeginSelection { .. } => Some("waveform.selection.begin"),
        WaveformInteraction::BeginEditFade { .. } => Some("waveform.edit_fade.begin"),
        WaveformInteraction::BeginEditFadeOuterGain { .. } => {
            Some("waveform.edit_fade_outer_gain.begin")
        }
        WaveformInteraction::FinishEditFadeOuterGain { .. } => {
            Some("waveform.edit_fade_outer_gain.finish")
        }
        WaveformInteraction::BeginEditGain { .. } => Some("waveform.edit_gain.begin"),
        WaveformInteraction::FinishEditGain { .. } => Some("waveform.edit_gain.finish"),
        WaveformInteraction::ClearEditFadeSilence { .. } => {
            Some("waveform.edit_fade.clear_silence")
        }
        WaveformInteraction::SelectSimilarSection { .. } => Some("waveform.similar_section.select"),
        WaveformInteraction::BeginSelectionResize { .. } => Some("waveform.selection.resize_begin"),
        WaveformInteraction::BeginSelectionMove { .. } => Some("waveform.selection.move_begin"),
        WaveformInteraction::BeginSampleSlide { .. } => Some("waveform.sample_slide.begin"),
        WaveformInteraction::FinishSampleSlide { .. } => Some("waveform.sample_slide.finish"),
        WaveformInteraction::BeginPan { .. } => Some("waveform.pan_begin"),
        WaveformInteraction::DragPlaySelectionExport(drag) => match drag.phase() {
            ui::DragHandlePhase::Started => Some("waveform.selection_export_drag.begin"),
            ui::DragHandlePhase::Moved => None,
            ui::DragHandlePhase::Ended => Some("waveform.selection_export_drag.end"),
            ui::DragHandlePhase::DoubleActivate => None,
            ui::DragHandlePhase::Cancelled => None,
        },
        WaveformInteraction::DragLoadedSample(_) => None,
        WaveformInteraction::FinishSelection { .. } => Some("waveform.selection.finish"),
        WaveformInteraction::UpdateSelection { .. }
        | WaveformInteraction::UpdateSampleSlide { .. }
        | WaveformInteraction::UpdateEditFadeOuterGain { .. }
        | WaveformInteraction::UpdateEditGain { .. }
        | WaveformInteraction::Frame => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_app::waveform::{WaveformEditFadeHandle, WaveformSelectionEdge};

    #[test]
    fn live_selection_updates_do_not_finish_harvest_mark_changes() {
        assert!(!waveform_interaction_can_finish_mark_change(
            &WaveformInteraction::BeginSelection {
                kind: WaveformSelectionKind::Play,
                visible_ratio: 0.1,
            }
        ));
        assert!(!waveform_interaction_can_finish_mark_change(
            &WaveformInteraction::UpdateSelection { visible_ratio: 0.4 }
        ));
        assert!(!waveform_interaction_can_finish_mark_change(
            &WaveformInteraction::UpdateEditGain { pointer_y: 24.0 }
        ));
        assert!(!waveform_interaction_can_finish_mark_change(
            &WaveformInteraction::UpdateEditFadeOuterGain {
                vertical_ratio: 0.4,
            }
        ));
    }

    #[test]
    fn finished_and_discrete_selection_edits_finish_harvest_mark_changes() {
        assert!(waveform_interaction_can_finish_mark_change(
            &WaveformInteraction::FinishSelection { visible_ratio: 0.4 }
        ));
        assert!(waveform_interaction_can_finish_mark_change(
            &WaveformInteraction::SelectSimilarSection {
                selection: SelectionRange::new(0.2, 0.4),
            }
        ));
        assert!(waveform_interaction_can_finish_mark_change(
            &WaveformInteraction::ClearEditFadeSilence {
                handle: WaveformEditFadeHandle::InOuterStart,
            }
        ));
        assert!(waveform_interaction_can_finish_mark_change(
            &WaveformInteraction::FinishEditGain { pointer_y: 12.0 }
        ));
        assert!(waveform_interaction_can_finish_mark_change(
            &WaveformInteraction::FinishEditFadeOuterGain {
                vertical_ratio: 0.4,
            }
        ));
    }

    #[test]
    fn freshly_painted_playmark_retargets_playback_only_on_release() {
        let active_drag = Some(WaveformActiveDragKind::Selection(
            WaveformSelectionKind::Play,
        ));

        assert!(!waveform_interaction_updates_play_selection(
            &WaveformInteraction::UpdateSelection {
                visible_ratio: 0.21
            },
            active_drag,
        ));
        assert!(waveform_interaction_updates_play_selection(
            &WaveformInteraction::FinishSelection {
                visible_ratio: 0.40
            },
            active_drag,
        ));
    }

    #[test]
    fn existing_playmark_edits_keep_live_playback_retargeting() {
        for active_drag in [
            Some(WaveformActiveDragKind::SelectionResize(
                WaveformSelectionKind::Play,
                WaveformSelectionEdge::End,
            )),
            Some(WaveformActiveDragKind::SelectionMove(
                WaveformSelectionKind::Play,
            )),
        ] {
            assert!(waveform_interaction_updates_play_selection(
                &WaveformInteraction::UpdateSelection {
                    visible_ratio: 0.35
                },
                active_drag,
            ));
            assert!(waveform_interaction_updates_play_selection(
                &WaveformInteraction::FinishSelection {
                    visible_ratio: 0.45
                },
                active_drag,
            ));
        }
    }

    #[test]
    fn committed_label_length_is_one_finished_playmark_mutation() {
        let interaction = WaveformInteraction::SetPlaySelectionFrameRange {
            start_frame: 120,
            end_frame: 480,
        };

        assert!(waveform_interaction_can_finish_mark_change(&interaction));
        assert!(waveform_interaction_updates_play_selection(
            &interaction,
            None
        ));
        assert!(waveform_interaction_finishes_play_selection_update(
            &interaction
        ));
        assert!(play_selection_transaction_finishes(&interaction, None));
    }

    #[test]
    fn playmark_drag_updates_do_not_sync_edit_fade_audio() {
        assert!(!waveform_interaction_updates_edit_selection(
            &WaveformInteraction::UpdateSelection {
                visible_ratio: 0.35
            },
            Some(WaveformActiveDragKind::Selection(
                WaveformSelectionKind::Play
            )),
        ));
        assert!(!waveform_interaction_updates_edit_selection(
            &WaveformInteraction::FinishSelection {
                visible_ratio: 0.45
            },
            Some(WaveformActiveDragKind::SelectionResize(
                WaveformSelectionKind::Play,
                WaveformSelectionEdge::End,
            )),
        ));
    }

    #[test]
    fn editmark_drag_updates_sync_edit_fade_audio() {
        assert!(waveform_interaction_updates_edit_selection(
            &WaveformInteraction::UpdateSelection {
                visible_ratio: 0.35
            },
            Some(WaveformActiveDragKind::Selection(
                WaveformSelectionKind::Edit
            )),
        ));
        assert!(waveform_interaction_updates_edit_selection(
            &WaveformInteraction::UpdateEditGain { pointer_y: 18.0 },
            None,
        ));
    }
}
