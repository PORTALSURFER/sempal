/// Presentation state for the contextual hotkey overlay.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HotkeyUiState {
    /// Whether the overlay is currently visible.
    pub overlay_visible: bool,
    /// True while the BPM input field is focused to suppress hotkeys during typing.
    pub suppress_for_bpm_input: bool,
}
