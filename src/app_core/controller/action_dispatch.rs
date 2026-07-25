use std::time::Instant;

use crate::app_core::actions::{
    GuiSurface, NativeColumnTriageAction, NativeUiAction, action_catalog_entry,
};
use crate::logging::{ActionDebugEvent, emit_action_debug_event};
use tracing::error;

use super::{
    AppController, apply_browser_ui_action, apply_map_ui_action, apply_prompt_and_update_ui_action,
    apply_waveform_ui_action,
};

/// Apply one UI runtime UI action and emit action telemetry.
pub(super) fn apply_ui_action(controller: &mut AppController, action: NativeUiAction) -> bool {
    let started_at = Instant::now();
    let action_id = ui_action_id(&action);
    let pane = ui_action_pane(&action);
    controller.begin_waveform_refresh_batch();
    let action = match apply_transport_ui_action(controller, action) {
        Ok(()) => {
            controller.end_waveform_refresh_batch();
            record_ui_action(action_id, pane, "success", started_at, None);
            return true;
        }
        Err(action) => action,
    };
    let action = match apply_browser_ui_action(controller, action) {
        Ok(()) => {
            controller.end_waveform_refresh_batch();
            record_ui_action(action_id, pane, "success", started_at, None);
            return true;
        }
        Err(action) => action,
    };
    let action = match apply_map_ui_action(controller, action) {
        Ok(()) => {
            controller.end_waveform_refresh_batch();
            record_ui_action(action_id, pane, "success", started_at, None);
            return true;
        }
        Err(action) => action,
    };
    let action = match apply_waveform_ui_action(controller, action) {
        Ok(()) => {
            controller.end_waveform_refresh_batch();
            record_ui_action(action_id, pane, "success", started_at, None);
            return true;
        }
        Err(action) => action,
    };
    let handled = match apply_prompt_and_update_ui_action(controller, action) {
        Ok(()) => true,
        Err(unhandled) => {
            error!(
                ?unhandled,
                "UI action was not handled by any dispatcher group"
            );
            false
        }
    };
    controller.end_waveform_refresh_batch();
    record_ui_action(
        action_id,
        pane,
        if handled { "success" } else { "unhandled" },
        started_at,
        (!handled).then_some("unhandled"),
    );
    handled
}

/// Try to dispatch transport-oriented UI actions.
fn apply_transport_ui_action(
    controller: &mut AppController,
    action: NativeUiAction,
) -> Result<(), NativeUiAction> {
    match action {
        NativeUiAction::ColumnTriage(NativeColumnTriageAction::SelectColumn { index }) => {
            controller.select_column_by_index(index)
        }
        NativeUiAction::ColumnTriage(NativeColumnTriageAction::MoveColumn { delta }) => {
            controller.move_selection_column(delta as isize)
        }
        NativeUiAction::Transport(
            crate::app_core::actions::NativeTransportAction::PlayFromStart,
        ) => {
            controller.play_from_start();
        }
        NativeUiAction::Transport(
            crate::app_core::actions::NativeTransportAction::PlayFromCurrentPlayhead,
        ) => {
            controller.play_from_current_playhead();
        }
        NativeUiAction::Transport(
            crate::app_core::actions::NativeTransportAction::PlayFromWaveformCursor,
        ) => {
            controller.play_from_cursor();
        }
        NativeUiAction::Transport(
            crate::app_core::actions::NativeTransportAction::PlayWaveformAtPrecise {
                position_nanos,
            },
        ) => {
            controller.seek_waveform_nanos(position_nanos);
        }
        NativeUiAction::Transport(
            crate::app_core::actions::NativeTransportAction::ToggleTransport,
        ) => controller.toggle_play_pause(),
        NativeUiAction::Transport(
            crate::app_core::actions::NativeTransportAction::PlayCompareAnchor,
        ) => controller.play_compare_anchor(),
        NativeUiAction::Transport(
            crate::app_core::actions::NativeTransportAction::HandleEscape,
        ) => controller.handle_escape(),
        NativeUiAction::HistoryAndUpdate(
            crate::app_core::actions::NativeHistoryUpdateAction::Undo,
        ) => controller.undo(),
        NativeUiAction::HistoryAndUpdate(
            crate::app_core::actions::NativeHistoryUpdateAction::Redo,
        ) => controller.redo(),
        NativeUiAction::Options(
            crate::app_core::actions::NativeOptionsAction::ToggleLoopPlayback,
        ) => controller.toggle_loop(),
        NativeUiAction::Options(crate::app_core::actions::NativeOptionsAction::ToggleLoopLock) => {
            controller.toggle_loop_lock()
        }
        NativeUiAction::Options(crate::app_core::actions::NativeOptionsAction::SetVolume {
            value_milli,
        }) => {
            controller.set_volume_live((f32::from(value_milli.min(1000)) / 1000.0).clamp(0.0, 1.0))
        }
        NativeUiAction::Options(
            crate::app_core::actions::NativeOptionsAction::CommitVolumeSetting,
        ) => controller.commit_volume_setting(),
        action => return Err(action),
    }
    Ok(())
}

fn ui_action_id(action: &NativeUiAction) -> &'static str {
    action_catalog_entry(action).action_id
}

fn ui_action_pane(action: &NativeUiAction) -> Option<&'static str> {
    let entry = action_catalog_entry(action);
    Some(match entry.surface {
        GuiSurface::Browser => "browser",
        GuiSurface::Sources => "sources",
        GuiSurface::Waveform => "waveform",
        GuiSurface::Transport => "transport",
        GuiSurface::Map => "map",
        GuiSurface::Options => "options",
        GuiSurface::Prompt => "prompt",
        GuiSurface::Update => "update",
    })
}

fn record_ui_action(
    action: &'static str,
    pane: Option<&'static str>,
    outcome: &'static str,
    started_at: Instant,
    error: Option<&'static str>,
) {
    if !should_record_ui_action(action, outcome) {
        return;
    }
    emit_action_debug_event(ActionDebugEvent {
        action,
        pane,
        source: None,
        outcome,
        elapsed: started_at.elapsed(),
        error,
    });
}

fn should_record_ui_action(action: &'static str, outcome: &'static str) -> bool {
    if action == "set_browser_view_start" && outcome == "success" {
        #[cfg(test)]
        return false;
        #[cfg(not(test))]
        return crate::env_flags::env_var_truthy(crate::hotpath_telemetry::HOTPATH_TELEMETRY_ENV);
    }
    true
}
