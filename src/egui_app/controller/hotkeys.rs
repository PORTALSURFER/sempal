use super::*;
use crate::egui_app::state::FocusContext;
use egui::Key;

/// Identifies the surface that owns a hotkey action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HotkeyScope {
    Global,
    Focus(FocusContext),
}

impl HotkeyScope {
    pub(crate) fn matches(self, focus: FocusContext) -> bool {
        match self {
            HotkeyScope::Global => true,
            HotkeyScope::Focus(target) => target == focus,
        }
    }
}

/// Keyboard gesture used to trigger an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HotkeyGesture {
    pub(crate) key: Key,
    pub(crate) command: bool,
    pub(crate) shift: bool,
    pub(crate) alt: bool,
}

impl HotkeyGesture {
    pub const fn new(key: Key) -> Self {
        Self {
            key,
            command: false,
            shift: false,
            alt: false,
        }
    }

    pub const fn with_command(key: Key) -> Self {
        Self {
            key,
            command: true,
            shift: false,
            alt: false,
        }
    }
}

/// Logical identifier for controller-dispatched hotkey commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HotkeyCommand {
    ToggleFocusedSelection,
    NormalizeFocusedSample,
    DeleteFocusedSample,
    AddFocusedToCollection,
    ToggleOverlay,
    ToggleLoop,
}

/// Hotkey metadata surfaced to the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HotkeyAction {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) gesture: HotkeyGesture,
    scope: HotkeyScope,
    command: HotkeyCommand,
}

impl HotkeyAction {
    pub(crate) fn is_active(&self, focus: FocusContext) -> bool {
        self.scope.matches(focus)
    }

    pub(crate) fn is_global(&self) -> bool {
        matches!(self.scope, HotkeyScope::Global)
    }
}

const HOTKEY_ACTIONS: &[HotkeyAction] = &[
    HotkeyAction {
        id: "toggle-select",
        label: "Toggle selection",
        gesture: HotkeyGesture::new(Key::X),
        scope: HotkeyScope::Focus(FocusContext::SampleBrowser),
        command: HotkeyCommand::ToggleFocusedSelection,
    },
    HotkeyAction {
        id: "normalize-browser",
        label: "Normalize sample",
        gesture: HotkeyGesture::new(Key::N),
        scope: HotkeyScope::Focus(FocusContext::SampleBrowser),
        command: HotkeyCommand::NormalizeFocusedSample,
    },
    HotkeyAction {
        id: "normalize-collection",
        label: "Normalize sample",
        gesture: HotkeyGesture::new(Key::N),
        scope: HotkeyScope::Focus(FocusContext::CollectionSample),
        command: HotkeyCommand::NormalizeFocusedSample,
    },
    HotkeyAction {
        id: "delete-browser",
        label: "Delete sample",
        gesture: HotkeyGesture::new(Key::D),
        scope: HotkeyScope::Focus(FocusContext::SampleBrowser),
        command: HotkeyCommand::DeleteFocusedSample,
    },
    HotkeyAction {
        id: "delete-collection",
        label: "Delete sample",
        gesture: HotkeyGesture::new(Key::D),
        scope: HotkeyScope::Focus(FocusContext::CollectionSample),
        command: HotkeyCommand::DeleteFocusedSample,
    },
    HotkeyAction {
        id: "add-to-collection",
        label: "Add to collection",
        gesture: HotkeyGesture::new(Key::C),
        scope: HotkeyScope::Focus(FocusContext::SampleBrowser),
        command: HotkeyCommand::AddFocusedToCollection,
    },
    HotkeyAction {
        id: "show-hotkeys",
        label: "Show hotkeys",
        gesture: HotkeyGesture::with_command(Key::Slash),
        scope: HotkeyScope::Global,
        command: HotkeyCommand::ToggleOverlay,
    },
    HotkeyAction {
        id: "toggle-loop",
        label: "Toggle loop",
        gesture: HotkeyGesture::new(Key::L),
        scope: HotkeyScope::Global,
        command: HotkeyCommand::ToggleLoop,
    },
];

pub(crate) fn iter_actions() -> impl Iterator<Item = HotkeyAction> {
    HOTKEY_ACTIONS.iter().copied()
}

pub(crate) fn focused_actions(focus: FocusContext) -> Vec<HotkeyAction> {
    HOTKEY_ACTIONS
        .iter()
        .copied()
        .filter(|action| action.is_active(focus))
        .collect()
}

pub(crate) fn global_actions() -> Vec<HotkeyAction> {
    HOTKEY_ACTIONS
        .iter()
        .copied()
        .filter(|action| matches!(action.scope, HotkeyScope::Global))
        .collect()
}

impl EguiController {
    pub(crate) fn handle_hotkey(&mut self, action: HotkeyAction, focus: FocusContext) {
        match action.command {
            HotkeyCommand::ToggleFocusedSelection => {
                if matches!(focus, FocusContext::SampleBrowser) {
                    self.toggle_focused_selection();
                }
            }
            HotkeyCommand::NormalizeFocusedSample => {
                self.normalize_focused_sample(focus);
            }
            HotkeyCommand::DeleteFocusedSample => {
                self.delete_focused_sample(focus);
            }
            HotkeyCommand::AddFocusedToCollection => {
                if matches!(focus, FocusContext::SampleBrowser) {
                    self.add_focused_sample_to_collection();
                }
            }
            HotkeyCommand::ToggleOverlay => {
                self.ui.hotkeys.overlay_visible = !self.ui.hotkeys.overlay_visible;
            }
            HotkeyCommand::ToggleLoop => {
                self.toggle_loop();
            }
        }
    }

    fn normalize_focused_sample(&mut self, focus: FocusContext) {
        match focus {
            FocusContext::SampleBrowser => {
                if let Some(row) = self.focused_browser_row() {
                    let _ = self.normalize_browser_sample(row);
                } else {
                    self.set_status("Focus a sample to normalize it", StatusTone::Info);
                }
            }
            FocusContext::CollectionSample => {
                if let Some(row) = self.ui.collections.selected_sample {
                    let _ = self.normalize_collection_sample(row);
                } else {
                    self.set_status(
                        "Select a collection sample to normalize it",
                        StatusTone::Info,
                    );
                }
            }
            FocusContext::None => {}
        }
    }

    fn delete_focused_sample(&mut self, focus: FocusContext) {
        match focus {
            FocusContext::SampleBrowser => {
                if let Some(row) = self.focused_browser_row() {
                    let _ = self.delete_browser_sample(row);
                } else {
                    self.set_status("Focus a sample to delete it", StatusTone::Info);
                }
            }
            FocusContext::CollectionSample => {
                if let Some(row) = self.ui.collections.selected_sample {
                    let _ = self.delete_collection_sample(row);
                } else {
                    self.set_status("Select a collection sample to delete it", StatusTone::Info);
                }
            }
            FocusContext::None => {}
        }
    }

    fn add_focused_sample_to_collection(&mut self) {
        let Some(collection_id) = self.current_collection_id() else {
            self.set_status("Select a collection first", StatusTone::Warning);
            return;
        };
        let Some(path) = self.focused_browser_path() else {
            self.set_status("Focus a sample to add it to a collection", StatusTone::Info);
            return;
        };
        if let Err(err) = self.add_sample_to_collection(&collection_id, path.as_path()) {
            self.set_status(err, StatusTone::Error);
        }
    }
}
