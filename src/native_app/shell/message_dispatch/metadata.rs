use radiant::prelude as ui;
use std::time::Instant;

use crate::native_app::app::{GuiMessage, MetadataMessage, NativeAppState, emit_gui_action};

impl NativeAppState {
    pub(super) fn apply_metadata_message(
        &mut self,
        message: MetadataMessage,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        match message {
            MetadataMessage::FocusMetadataTagInput => {
                self.focus_metadata_tag_input(context);
            }
            MetadataMessage::MetadataTagInput(message) => {
                self.apply_metadata_tag_input(message, context)
            }
            MetadataMessage::CancelMetadataTagEntry => {
                self.cancel_metadata_tag_entry();
            }
            MetadataMessage::MoveMetadataTagCompletion(delta) => {
                self.move_metadata_tag_completion_selection(delta);
            }
            MetadataMessage::HoverMetadataTagCompletion(value) => {
                self.hover_metadata_tag_completion(value);
            }
            MetadataMessage::SelectMetadataTagCompletion(value) => {
                self.submit_selected_metadata_tag_completion(value, context);
            }
            MetadataMessage::ToggleMetadataTagLibrary => self.toggle_metadata_tag_library(),
            MetadataMessage::ToggleMetadataTagCategory(category_id) => {
                self.toggle_metadata_tag_category(category_id);
            }
            MetadataMessage::SelectMetadataTag(tag) => {
                self.select_metadata_tag(tag);
            }
            MetadataMessage::ToggleMetadataTag(tag) => {
                self.toggle_metadata_tag(tag, context);
            }
            MetadataMessage::RemoveMetadataTag(tag) => {
                self.remove_metadata_tag(tag, context);
            }
            #[cfg(test)]
            MetadataMessage::ToggleMetadataTagForFiles { tag, file_ids } => {
                self.toggle_metadata_tag_for_file_ids(tag, file_ids, context);
            }
            MetadataMessage::DragMetadataTag { tag, drag } => {
                self.drag_metadata_tag(tag, drag, context);
            }
            MetadataMessage::HoverMetadataTagDropCategory { category_id } => {
                self.hover_metadata_tag_drop_category(category_id);
            }
            MetadataMessage::ClearMetadataTagDropCategoryUnless { category_id } => {
                self.clear_metadata_tag_drop_category_unless(category_id);
            }
            MetadataMessage::DropMetadataTagOnCategory { category_id } => {
                self.drop_metadata_tag_on_category(category_id, context);
            }
            MetadataMessage::OpenMetadataTagContextMenu { tag, position } => {
                self.open_metadata_tag_context_menu(tag, position);
            }
            MetadataMessage::DeleteContextMetadataTag => {
                self.delete_context_metadata_tag(context);
            }
            MetadataMessage::DeleteSelectedMetadataTag => {
                self.remove_selected_metadata_tag(context);
            }
            MetadataMessage::MetadataTagsPersisted(result) => {
                self.finish_metadata_tag_persist(result);
            }
            MetadataMessage::MetadataTagsLoaded(result) => {
                self.finish_persisted_metadata_tags_load(result);
            }
            MetadataMessage::ToggleSampleNameViewMode => {
                self.metadata.sample_name_view_mode = self.metadata.sample_name_view_mode.toggled();
            }
        }
    }

    fn toggle_metadata_tag_library(&mut self) {
        let started_at = Instant::now();
        self.metadata.tag_library_open = !self.metadata.tag_library_open;
        let outcome = if self.metadata.tag_library_open {
            "opened"
        } else {
            "closed"
        };
        emit_gui_action(
            "metadata_tags.toggle_library",
            Some("folder_browser"),
            None,
            outcome,
            started_at,
            None,
        );
    }

    fn toggle_metadata_tag_category(&mut self, category_id: String) {
        let started_at = Instant::now();
        let source = category_id.clone();
        if !self.metadata.collapsed_tag_categories.remove(&category_id) {
            self.metadata.collapsed_tag_categories.insert(category_id);
        }
        emit_gui_action(
            "metadata_tags.toggle_category",
            Some("tag_editor"),
            Some(source.as_str()),
            "applied",
            started_at,
            None,
        );
    }
}
