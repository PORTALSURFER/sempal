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

/// Logical identifier for controller-dispatched hotkey commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HotkeyCommand {
    Undo,
    Redo,
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
    FindSimilarFocusedSample,
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
    FocusHistoryPrevious,
    FocusHistoryNext,
    MoveTrashedToFolder,
    TagNeutralSelected,
    IncrementRatingSelected,
    DecrementRatingSelected,
    TrimSelection,
    ReverseSelection,
    FadeSelectionLeftToRight,
    FadeSelectionRightToLeft,
    MuteSelection,
    DeleteSliceMarkers,
    NormalizeWaveform,
    AlignWaveformStartToMarker,
    CropSelection,
    CropSelectionNewSample,
    SaveSelectionToBrowser,
    OpenFeedbackIssuePrompt,
    CopyStatusLog,
    SelectAllBrowser,
    RenameFocusedCollection,
}

/// Hotkey metadata surfaced to the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HotkeyAction {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) gesture: HotkeyGesture,
    pub(crate) scope: HotkeyScope,
    pub(crate) command: HotkeyCommand,
}

impl HotkeyAction {
    pub(crate) fn is_active(&self, focus: FocusContext) -> bool {
        self.scope.matches(focus)
    }

    pub(crate) fn is_global(&self) -> bool {
        matches!(self.scope, HotkeyScope::Global)
    }

    pub(crate) fn command(&self) -> HotkeyCommand {
        self.command
    }
}
