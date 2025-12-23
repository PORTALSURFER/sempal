/// Logical focus buckets used to drive contextual keyboard shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusContext {
    /// No UI surface currently owns focus.
    None,
    /// The waveform viewer handles navigation/shortcuts.
    Waveform,
    /// The sample browser rows handle navigation/shortcuts.
    SampleBrowser,
    /// The source folder browser handles navigation/shortcuts.
    SourceFolders,
    /// The collections sample list handles navigation/shortcuts.
    CollectionSample,
    /// The sources list handles navigation/shortcuts.
    SourcesList,
    /// The selected folders list handles navigation/shortcuts.
    SelectedFolders,
    /// The collections list handles navigation/shortcuts.
    CollectionsList,
}

/// Focus metadata shared between the controller and egui renderer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiFocusState {
    pub context: FocusContext,
}

impl UiFocusState {
    /// Update the active focus context.
    pub fn set_context(&mut self, context: FocusContext) {
        self.context = context;
    }
}

impl Default for UiFocusState {
    fn default() -> Self {
        Self {
            context: FocusContext::None,
        }
    }
}
