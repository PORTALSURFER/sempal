mod actions;
mod format;
mod types;

pub(crate) use format::format_keypress;
pub(crate) use types::{HotkeyAction, HotkeyCommand, HotkeyGesture, HotkeyScope, KeyPress};

use actions::HOTKEY_ACTIONS;
use crate::egui_app::state::FocusContext;

pub(crate) fn iter_actions() -> impl Iterator<Item = HotkeyAction> {
    HOTKEY_ACTIONS.iter().copied()
}

pub(crate) fn focused_actions(focus: FocusContext) -> Vec<HotkeyAction> {
    let focus = match focus {
        FocusContext::None => FocusContext::SampleBrowser,
        other => other,
    };
    HOTKEY_ACTIONS
        .iter()
        .copied()
        .filter(|action| matches!(action.scope, HotkeyScope::Focus(_)) && action.is_active(focus))
        .collect()
}

pub(crate) fn global_actions() -> Vec<HotkeyAction> {
    HOTKEY_ACTIONS
        .iter()
        .copied()
        .filter(|action| matches!(action.scope, HotkeyScope::Global))
        .collect()
}

