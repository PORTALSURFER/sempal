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
    pub(crate) first: KeyPress,
    pub(crate) chord: Option<KeyPress>,
}

/// A single keypress plus modifier state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KeyPress {
    pub(crate) key: Key,
    pub(crate) command: bool,
    pub(crate) shift: bool,
    pub(crate) alt: bool,
}

impl HotkeyGesture {
    pub const fn new(key: Key) -> Self {
        Self {
            first: KeyPress::new(key),
            chord: None,
        }
    }

    pub const fn with_command(key: Key) -> Self {
        Self {
            first: KeyPress::with_command(key),
            chord: None,
        }
    }

    pub const fn with_shift(key: Key) -> Self {
        Self {
            first: KeyPress::with_shift(key),
            chord: None,
        }
    }

    pub const fn with_chord(first: KeyPress, second: KeyPress) -> Self {
        Self {
            first,
            chord: Some(second),
        }
    }
}

impl KeyPress {
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

    pub const fn with_shift(key: Key) -> Self {
        Self {
            key,
            command: false,
            shift: true,
            alt: false,
        }
    }

    pub const fn with_alt(key: Key) -> Self {
        Self {
            key,
            command: false,
            shift: false,
            alt: true,
        }
    }
}

/// Render a keypress in a user-friendly format (e.g. "Ctrl + G").
pub(crate) fn format_keypress(press: &KeyPress) -> String {
    let mut parts: Vec<&'static str> = Vec::new();
    if press.command {
        parts.push(command_label());
    }
    if press.shift {
        parts.push("Shift");
    }
    if press.alt {
        parts.push("Alt");
    }
    parts.push(key_label(press.key));
    parts.join(" + ")
}

fn command_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    }
}

fn key_label(key: Key) -> &'static str {
    match key {
        egui::Key::X => "X",
        egui::Key::N => "N",
        egui::Key::D => "D",
        egui::Key::C => "C",
        egui::Key::R => "R",
        egui::Key::Slash => "/",
        egui::Key::G => "G",
        egui::Key::S => "S",
        egui::Key::W => "W",
        egui::Key::L => "L",
        egui::Key::P => "P",
        egui::Key::F => "F",
        egui::Key::OpenBracket => "[",
        egui::Key::CloseBracket => "]",
        egui::Key::ArrowLeft => "Left",
        egui::Key::ArrowRight => "Right",
        egui::Key::ArrowUp => "Up",
        egui::Key::ArrowDown => "Down",
        _ => "Key",
    }
}

/// Logical identifier for controller-dispatched hotkey commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HotkeyCommand {
    ToggleFocusedSelection,
    ToggleFolderSelection,
    NormalizeFocusedSample,
    DeleteFocusedSample,
    DeleteFocusedFolder,
    RenameFocusedFolder,
    RenameFocusedSample,
    CreateFolder,
    FocusFolderSearch,
    FocusBrowserSearch,
    AddFocusedToCollection,
    ToggleOverlay,
    ToggleLoop,
    FocusWaveform,
    FocusBrowserSamples,
    FocusCollectionSamples,
    FocusSourcesList,
    FocusCollectionsList,
    PlayRandomSample,
    PlayPreviousRandomSample,
    ToggleRandomNavigationMode,
    MoveTrashedToFolder,
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
        id: "toggle-folder-select",
        label: "Toggle folder selection",
        gesture: HotkeyGesture::new(Key::X),
        scope: HotkeyScope::Focus(FocusContext::SourceFolders),
        command: HotkeyCommand::ToggleFolderSelection,
    },
    HotkeyAction {
        id: "delete-folder",
        label: "Delete folder",
        gesture: HotkeyGesture::new(Key::D),
        scope: HotkeyScope::Focus(FocusContext::SourceFolders),
        command: HotkeyCommand::DeleteFocusedFolder,
    },
    HotkeyAction {
        id: "rename-folder",
        label: "Rename folder",
        gesture: HotkeyGesture::new(Key::R),
        scope: HotkeyScope::Focus(FocusContext::SourceFolders),
        command: HotkeyCommand::RenameFocusedFolder,
    },
    HotkeyAction {
        id: "rename-sample",
        label: "Rename sample",
        gesture: HotkeyGesture::new(Key::R),
        scope: HotkeyScope::Focus(FocusContext::SampleBrowser),
        command: HotkeyCommand::RenameFocusedSample,
    },
    HotkeyAction {
        id: "new-folder",
        label: "New folder",
        gesture: HotkeyGesture::new(Key::N),
        scope: HotkeyScope::Focus(FocusContext::SourceFolders),
        command: HotkeyCommand::CreateFolder,
    },
    HotkeyAction {
        id: "search-folders",
        label: "Search folders",
        gesture: HotkeyGesture::new(Key::F),
        scope: HotkeyScope::Focus(FocusContext::SourceFolders),
        command: HotkeyCommand::FocusFolderSearch,
    },
    HotkeyAction {
        id: "search-browser",
        label: "Search samples",
        gesture: HotkeyGesture::new(Key::F),
        scope: HotkeyScope::Focus(FocusContext::SampleBrowser),
        command: HotkeyCommand::FocusBrowserSearch,
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
    HotkeyAction {
        id: "focus-waveform",
        label: "Focus waveform",
        gesture: HotkeyGesture::with_chord(KeyPress::new(Key::G), KeyPress::new(Key::W)),
        scope: HotkeyScope::Global,
        command: HotkeyCommand::FocusWaveform,
    },
    HotkeyAction {
        id: "focus-browser",
        label: "Focus source samples",
        gesture: HotkeyGesture::with_chord(KeyPress::new(Key::G), KeyPress::new(Key::S)),
        scope: HotkeyScope::Global,
        command: HotkeyCommand::FocusBrowserSamples,
    },
    HotkeyAction {
        id: "focus-collection-samples",
        label: "Focus collection samples",
        gesture: HotkeyGesture::with_chord(KeyPress::new(Key::G), KeyPress::new(Key::C)),
        scope: HotkeyScope::Global,
        command: HotkeyCommand::FocusCollectionSamples,
    },
    HotkeyAction {
        id: "focus-sources-list",
        label: "Focus sources list",
        gesture: HotkeyGesture::with_chord(KeyPress::new(Key::G), KeyPress::with_shift(Key::S)),
        scope: HotkeyScope::Global,
        command: HotkeyCommand::FocusSourcesList,
    },
    HotkeyAction {
        id: "focus-collections-list",
        label: "Focus collections list",
        gesture: HotkeyGesture::with_chord(KeyPress::new(Key::G), KeyPress::with_shift(Key::C)),
        scope: HotkeyScope::Global,
        command: HotkeyCommand::FocusCollectionsList,
    },
    HotkeyAction {
        id: "play-random-sample",
        label: "Play random sample",
        gesture: HotkeyGesture {
            first: KeyPress::with_shift(Key::R),
            chord: None,
        },
        scope: HotkeyScope::Global,
        command: HotkeyCommand::PlayRandomSample,
    },
    HotkeyAction {
        id: "toggle-random-navigation-mode",
        label: "Toggle random navigation mode",
        gesture: HotkeyGesture {
            first: KeyPress::with_alt(Key::R),
            chord: None,
        },
        scope: HotkeyScope::Global,
        command: HotkeyCommand::ToggleRandomNavigationMode,
    },
    HotkeyAction {
        id: "play-previous-random-sample",
        label: "Play previous random sample",
        gesture: HotkeyGesture {
            first: KeyPress {
                key: Key::R,
                command: true,
                shift: true,
                alt: false,
            },
            chord: None,
        },
        scope: HotkeyScope::Global,
        command: HotkeyCommand::PlayPreviousRandomSample,
    },
    HotkeyAction {
        id: "move-trashed-to-folder",
        label: "Move trashed samples to folder",
        gesture: HotkeyGesture::new(Key::P),
        scope: HotkeyScope::Global,
        command: HotkeyCommand::MoveTrashedToFolder,
    },
    HotkeyAction {
        id: "move-trashed-to-folder-shift",
        label: "Move trashed samples to folder",
        gesture: HotkeyGesture::with_shift(Key::P),
        scope: HotkeyScope::Global,
        command: HotkeyCommand::MoveTrashedToFolder,
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
            HotkeyCommand::ToggleFolderSelection => {
                if matches!(focus, FocusContext::SourceFolders) {
                    self.toggle_focused_folder_selection();
                }
            }
            HotkeyCommand::NormalizeFocusedSample => {
                self.normalize_focused_sample(focus);
            }
            HotkeyCommand::DeleteFocusedSample => {
                self.delete_focused_sample(focus);
            }
            HotkeyCommand::DeleteFocusedFolder => {
                if matches!(focus, FocusContext::SourceFolders) {
                    self.delete_focused_folder();
                }
            }
            HotkeyCommand::RenameFocusedFolder => {
                if matches!(focus, FocusContext::SourceFolders) {
                    self.start_folder_rename();
                }
            }
            HotkeyCommand::RenameFocusedSample => {
                if matches!(focus, FocusContext::SampleBrowser) {
                    self.start_browser_rename();
                }
            }
            HotkeyCommand::CreateFolder => {
                if matches!(focus, FocusContext::SourceFolders) {
                    self.start_new_folder();
                }
            }
            HotkeyCommand::FocusFolderSearch => {
                if matches!(focus, FocusContext::SourceFolders) {
                    self.focus_folder_search();
                }
            }
            HotkeyCommand::FocusBrowserSearch => {
                if matches!(focus, FocusContext::SampleBrowser) {
                    self.focus_browser_search();
                }
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
            HotkeyCommand::FocusWaveform => {
                self.focus_waveform();
            }
            HotkeyCommand::FocusBrowserSamples => {
                self.focus_browser_list();
            }
            HotkeyCommand::FocusCollectionSamples => {
                self.focus_collection_samples_list();
            }
            HotkeyCommand::FocusSourcesList => {
                self.focus_sources_list();
            }
            HotkeyCommand::FocusCollectionsList => {
                self.focus_collections_list();
            }
            HotkeyCommand::PlayRandomSample => {
                self.play_random_visible_sample();
            }
            HotkeyCommand::PlayPreviousRandomSample => {
                self.play_previous_random_sample();
            }
            HotkeyCommand::ToggleRandomNavigationMode => {
                self.toggle_random_navigation_mode();
            }
            HotkeyCommand::MoveTrashedToFolder => {
                self.move_all_trashed_to_folder();
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
            FocusContext::None
            | FocusContext::Waveform
            | FocusContext::SourceFolders
            | FocusContext::SourcesList
            | FocusContext::CollectionsList => {}
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
            FocusContext::None
            | FocusContext::Waveform
            | FocusContext::SourceFolders
            | FocusContext::SourcesList
            | FocusContext::CollectionsList => {}
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
