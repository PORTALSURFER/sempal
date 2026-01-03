use super::HotkeysController;
use crate::egui_app::controller::hotkeys::HotkeyCommand;
use crate::egui_app::controller::StatusTone;
use crate::egui_app::state::FocusContext;
use crate::sample_sources::SampleTag;

pub(super) fn handle_collections_command(
    controller: &mut HotkeysController<'_>,
    command: HotkeyCommand,
    focus: FocusContext,
) -> bool {
    match command {
        HotkeyCommand::RenameFocusedCollection => {
            controller.start_collection_rename();
            true
        }
        HotkeyCommand::NormalizeFocusedSample => {
            if matches!(focus, FocusContext::CollectionSample) {
                controller.normalize_focused_collection_sample();
                true
            } else {
                false
            }
        }
        HotkeyCommand::DeleteFocusedSample => {
            if matches!(focus, FocusContext::CollectionSample) {
                controller.delete_focused_collection_sample();
                true
            } else {
                false
            }
        }
        HotkeyCommand::TagKeepSelected => {
            if matches!(focus, FocusContext::CollectionSample) {
                controller.tag_selected_collection_sample(SampleTag::Keep);
                true
            } else {
                false
            }
        }
        HotkeyCommand::TagNeutralSelected => {
            if matches!(focus, FocusContext::CollectionSample) {
                controller.tag_selected_collection_sample(SampleTag::Neutral);
                true
            } else {
                false
            }
        }
        HotkeyCommand::TagTrashSelected => {
            if matches!(focus, FocusContext::CollectionSample) {
                controller.tag_selected_collection_sample(SampleTag::Trash);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

impl HotkeysController<'_> {
    fn normalize_focused_collection_sample(&mut self) {
        if let Some(row) = self.ui.collections.selected_sample {
            let _ = self.normalize_collection_sample(row);
        } else {
            self.set_status(
                "Select a collection sample to normalize it",
                StatusTone::Info,
            );
        }
    }

    fn delete_focused_collection_sample(&mut self) {
        if let Some(row) = self.ui.collections.selected_sample {
            let _ = self.delete_collection_sample(row);
        } else {
            self.set_status("Select a collection sample to delete it", StatusTone::Info);
        }
    }
}
