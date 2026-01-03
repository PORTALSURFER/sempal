use super::*;
use crate::egui_app::controller::hotkeys::{HotkeyAction, HotkeyCommand};
use crate::egui_app::state::FocusContext;

mod browser;
mod collections;
mod waveform;

pub(crate) trait HotkeysActions {
    fn handle_hotkey(&mut self, action: HotkeyAction, focus: FocusContext);
}

pub(crate) struct HotkeysController<'a> {
    controller: &'a mut EguiController,
}

impl<'a> HotkeysController<'a> {
    pub(crate) fn new(controller: &'a mut EguiController) -> Self {
        Self { controller }
    }
}

impl std::ops::Deref for HotkeysController<'_> {
    type Target = EguiController;

    fn deref(&self) -> &Self::Target {
        self.controller
    }
}

impl std::ops::DerefMut for HotkeysController<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.controller
    }
}

impl HotkeysActions for HotkeysController<'_> {
    fn handle_hotkey(&mut self, action: HotkeyAction, focus: FocusContext) {
        let command = action.command();
        if self.handle_global_command(command) {
            return;
        }
        if self.handle_tagging_command(command, focus) {
            return;
        }
        match focus {
            FocusContext::SampleBrowser => {
                let _ = browser::handle_browser_command(self, command);
            }
            FocusContext::Waveform => {
                let _ = waveform::handle_waveform_command(self, command);
            }
            FocusContext::CollectionsList | FocusContext::CollectionSample => {
                let _ = collections::handle_collections_command(self, command, focus);
            }
            FocusContext::SourceFolders => {
                let _ = self.handle_folders_command(command);
            }
            FocusContext::SourcesList | FocusContext::None => {}
        }
    }
}

impl HotkeysController<'_> {
    fn handle_global_command(&mut self, command: HotkeyCommand) -> bool {
        match command {
            HotkeyCommand::Undo => {
                self.undo();
                true
            }
            HotkeyCommand::Redo => {
                self.redo();
                true
            }
            HotkeyCommand::ToggleOverlay => {
                self.ui.hotkeys.overlay_visible = !self.ui.hotkeys.overlay_visible;
                true
            }
            HotkeyCommand::OpenFeedbackIssuePrompt => {
                self.ui.hotkeys.overlay_visible = false;
                self.open_feedback_issue_prompt();
                true
            }
            HotkeyCommand::CopyStatusLog => {
                self.copy_status_log_to_clipboard();
                true
            }
            HotkeyCommand::ToggleLoop => {
                self.toggle_loop();
                true
            }
            HotkeyCommand::FocusWaveform => {
                self.focus_waveform();
                true
            }
            HotkeyCommand::FocusBrowserSamples => {
                self.focus_browser_list();
                true
            }
            HotkeyCommand::FocusCollectionSamples => {
                self.focus_collection_samples_list();
                true
            }
            HotkeyCommand::FocusSourcesList => {
                self.focus_sources_list();
                true
            }
            HotkeyCommand::FocusCollectionsList => {
                self.focus_collections_list();
                true
            }
            HotkeyCommand::PlayRandomSample => {
                self.play_random_visible_sample();
                true
            }
            HotkeyCommand::PlayPreviousRandomSample => {
                self.play_previous_random_sample();
                true
            }
            HotkeyCommand::ToggleRandomNavigationMode => {
                self.toggle_random_navigation_mode();
                true
            }
            HotkeyCommand::MoveTrashedToFolder => {
                self.move_all_trashed_to_folder();
                true
            }
            _ => false,
        }
    }

    fn handle_folders_command(&mut self, command: HotkeyCommand) -> bool {
        match command {
            HotkeyCommand::ToggleFolderSelection => {
                self.toggle_focused_folder_selection();
                true
            }
            HotkeyCommand::DeleteFocusedFolder => {
                self.delete_focused_folder();
                true
            }
            HotkeyCommand::RenameFocusedFolder => {
                self.start_folder_rename();
                true
            }
            HotkeyCommand::CreateFolder => {
                self.start_new_folder();
                true
            }
            HotkeyCommand::FocusFolderSearch => {
                self.focus_folder_search();
                true
            }
            _ => false,
        }
    }

    fn handle_tagging_command(&mut self, command: HotkeyCommand, focus: FocusContext) -> bool {
        match command {
            HotkeyCommand::TagKeepSelected => {
                if matches!(focus, FocusContext::CollectionSample) {
                    self.tag_selected_collection_sample(SampleTag::Keep);
                } else {
                    self.tag_selected(SampleTag::Keep);
                }
                true
            }
            HotkeyCommand::TagNeutralSelected => {
                if matches!(focus, FocusContext::CollectionSample) {
                    self.tag_selected_collection_sample(SampleTag::Neutral);
                } else {
                    self.tag_selected(SampleTag::Neutral);
                }
                true
            }
            HotkeyCommand::TagTrashSelected => {
                if matches!(focus, FocusContext::CollectionSample) {
                    self.tag_selected_collection_sample(SampleTag::Trash);
                } else {
                    self.tag_selected(SampleTag::Trash);
                }
                true
            }
            _ => false,
        }
    }
}

impl EguiController {
    pub(crate) fn handle_hotkey(&mut self, action: HotkeyAction, focus: FocusContext) {
        self.hotkeys_ctrl().handle_hotkey(action, focus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_app::controller::hotkeys;
    use crate::egui_app::controller::test_support::{
        load_waveform_selection, prepare_with_source_and_wav_entries, sample_entry,
    };
    use crate::egui_app::state::{CollectionActionPrompt, FocusContext};
    use crate::sample_sources::SampleTag;
    use crate::selection::SelectionRange;

    fn action_for(command: HotkeyCommand) -> HotkeyAction {
        hotkeys::iter_actions()
            .find(|action| action.command() == command)
            .expect("missing hotkey action")
    }

    #[test]
    fn waveform_hotkey_respects_focus() {
        let (mut controller, source) = prepare_with_source_and_wav_entries(vec![sample_entry(
            "one.wav",
            SampleTag::Neutral,
        )]);
        load_waveform_selection(
            &mut controller,
            &source,
            "one.wav",
            &[0.1, -0.2, 0.3, -0.4],
            SelectionRange::new(0.0, 0.5),
        );
        let action = action_for(HotkeyCommand::CropSelection);

        controller.handle_hotkey(action, FocusContext::Waveform);
        assert!(controller.ui.waveform.pending_destructive.is_some());

        controller.ui.waveform.pending_destructive = None;
        controller.handle_hotkey(action, FocusContext::SampleBrowser);
        assert!(controller.ui.waveform.pending_destructive.is_none());
    }

    #[test]
    fn browser_hotkey_respects_focus() {
        let renderer = crate::waveform::WaveformRenderer::new(4, 4);
        let mut controller = EguiController::new(renderer, None);
        let action = action_for(HotkeyCommand::FocusBrowserSearch);

        controller.handle_hotkey(action, FocusContext::SampleBrowser);
        assert!(controller.ui.browser.search_focus_requested);

        controller.ui.browser.search_focus_requested = false;
        controller.handle_hotkey(action, FocusContext::Waveform);
        assert!(!controller.ui.browser.search_focus_requested);
    }

    #[test]
    fn collections_hotkey_respects_focus() {
        let renderer = crate::waveform::WaveformRenderer::new(4, 4);
        let mut controller = EguiController::new(renderer, None);
        let collection = crate::sample_sources::Collection::new("Test");
        let id = collection.id.clone();
        controller.library.collections.push(collection);
        controller.selection_state.ctx.selected_collection = Some(id);
        let action = action_for(HotkeyCommand::RenameFocusedCollection);

        controller.handle_hotkey(action, FocusContext::CollectionsList);
        assert!(matches!(
            controller.ui.collections.pending_action,
            Some(CollectionActionPrompt::Rename { .. })
        ));

        controller.ui.collections.pending_action = None;
        controller.handle_hotkey(action, FocusContext::SampleBrowser);
        assert!(controller.ui.collections.pending_action.is_none());
    }
}
