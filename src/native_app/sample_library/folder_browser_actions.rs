mod collection;
mod drag_preview;
mod folder_tree;
mod layout;
mod navigation;
mod source;

#[cfg(test)]
pub(in crate::native_app) use folder_tree::{
    FOLDER_ACTIVATION_PREP_INTENTS, FOLDER_ACTIVATION_PREP_REASON,
};
pub(in crate::native_app) use navigation::file_navigation_reveal_direction;
#[cfg(test)]
pub(in crate::native_app) use source::{
    SOURCE_SELECTION_PREP_INTENTS, SOURCE_SELECTION_PREP_REASON,
};

use radiant::prelude as ui;

use crate::native_app::app::{GuiMessage, NativeAppState, emit_gui_action};
use crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage;
use crate::native_app::sample_library::folder_browser::view_contract::{
    FOLDER_TREE_EDGE_CONTEXT_ROWS, FOLDER_TREE_OVERSCAN_ROWS, FOLDER_TREE_PROJECTED_VIEWPORT_ROWS,
};

impl NativeAppState {
    pub(in crate::native_app) fn apply_folder_browser_message(
        &mut self,
        message: FolderBrowserMessage,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        match message {
            FolderBrowserMessage::AddSource => self.add_source_from_dialog(context),
            FolderBrowserMessage::SelectSource(id) => {
                self.select_folder_browser_source(id, context)
            }
            FolderBrowserMessage::NavigateSource(delta) => {
                self.navigate_folder_browser_source(delta, context)
            }
            FolderBrowserMessage::DragSource(source_id, message) => {
                let started_at = std::time::Instant::now();
                if self.drag_source_row(source_id.clone(), message, context) {
                    self.persist_user_configuration(
                        "folder_browser.source.reorder.persist",
                        started_at,
                    );
                    emit_gui_action(
                        "folder_browser.source.reorder",
                        Some("folder_browser"),
                        Some(source_id.as_str()),
                        "applied",
                        started_at,
                        None,
                    );
                }
            }
            FolderBrowserMessage::OpenSourceContextMenu(source_id, position) => {
                self.open_source_context_menu(source_id, position);
            }
            FolderBrowserMessage::BeginRenameSelected => self.begin_folder_browser_rename(context),
            FolderBrowserMessage::CancelRename => {
                self.library.folder_browser.cancel_rename();
            }
            FolderBrowserMessage::BeginCreateSubfolder => {
                self.begin_folder_browser_subfolder_creation(context);
            }
            FolderBrowserMessage::RenameInput(message) => {
                self.apply_folder_browser_rename_input(message, context);
            }
            FolderBrowserMessage::TagFilterInput(message) => {
                self.apply_folder_browser_tag_filter_input(message);
            }
            FolderBrowserMessage::TogglePlaybackTypeFilter(filter, enabled) => {
                self.toggle_folder_browser_playback_type_filter(filter, enabled);
            }
            FolderBrowserMessage::SetCurationScope(scope, enabled) => {
                let previous_visible_ids = self
                    .library
                    .folder_browser
                    .selected_audio_file_ids_matching_tags(&self.metadata.tags_by_file);
                self.library
                    .folder_browser
                    .set_curation_scope(scope, enabled);
                self.library
                    .folder_browser
                    .reconcile_visible_file_selection_after_tag_filter(
                        previous_visible_ids,
                        &self.metadata.tags_by_file,
                    );
                self.focus_visible_browser_file_after_filter_change(context);
            }
            FolderBrowserMessage::DropOnFolder(folder_id) => {
                self.drop_on_folder_browser_folder(folder_id, context);
            }
            FolderBrowserMessage::DropOnSource(source_id) => {
                self.drop_on_folder_browser_source(source_id, context);
            }
            FolderBrowserMessage::DropOnCollection(collection) => {
                self.drop_on_folder_browser_collection(collection, context);
            }
            FolderBrowserMessage::OpenFolderContextMenu(folder_id, position) => {
                self.open_folder_context_menu(folder_id, position);
            }
            FolderBrowserMessage::OpenCollectionContextMenu(collection, position) => {
                self.open_collection_context_menu(collection, position);
            }
            FolderBrowserMessage::ActivateFolder(folder_id, modifiers) => {
                self.activate_folder_browser_folder(folder_id, modifiers, context);
            }
            FolderBrowserMessage::DragFolder(folder_id, drag) => {
                self.drag_folder_browser_folder(folder_id, drag, context);
            }
            FolderBrowserMessage::ActivateCollection(collection) => {
                self.activate_folder_browser_collection(collection, context);
            }
            FolderBrowserMessage::RenameCollection(collection) => {
                self.begin_collection_rename(collection, context);
            }
            FolderBrowserMessage::DragFileColumn(column_id, message) => {
                self.drag_file_column(column_id, message, context);
            }
            FolderBrowserMessage::CancelFileColumnDrag => {
                self.cancel_file_column_drag(context);
            }
            FolderBrowserMessage::ToggleSimilarityAnchor(file_id) => {
                self.toggle_similarity_anchor(file_id, context);
            }
            FolderBrowserMessage::ToggleEmptyFolderVisibility => {
                let enabled = self.library.folder_browser.toggle_empty_folder_visibility();
                self.library.folder_browser.sync_tree_view_to_selection(
                    FOLDER_TREE_PROJECTED_VIEWPORT_ROWS,
                    FOLDER_TREE_OVERSCAN_ROWS,
                    FOLDER_TREE_EDGE_CONTEXT_ROWS,
                );
                if enabled {
                    self.queue_selected_source_folder_tree_refresh(
                        context,
                        "folder_browser.show_empty_folders_refresh",
                        "gui-show-empty-folder-tree-refresh",
                    );
                }
            }
            FolderBrowserMessage::ResizeCollectionsPanel(message) => {
                let before = self.library.folder_browser.collections_panel_height();
                self.library
                    .folder_browser
                    .resize_collections_panel(message);
                let after = self.library.folder_browser.collections_panel_height();
                if (after - before).abs() > f32::EPSILON {
                    self.ui.chrome.overflow_fades.arm();
                }
            }
            FolderBrowserMessage::ResizeFilterPanel(message) => {
                let before = self.library.folder_browser.filter_panel_height();
                self.library.folder_browser.resize_filter_panel(message);
                let after = self.library.folder_browser.filter_panel_height();
                if (after - before).abs() > f32::EPSILON {
                    self.ui.chrome.overflow_fades.arm();
                }
            }
            FolderBrowserMessage::ResizeMetadataPanel(message) => {
                let before = self.library.folder_browser.metadata_panel_height();
                self.library.folder_browser.resize_metadata_panel(message);
                let after = self.library.folder_browser.metadata_panel_height();
                if (after - before).abs() > f32::EPSILON {
                    self.ui.chrome.overflow_fades.arm();
                }
            }
            message => self.library.folder_browser.apply_message(message),
        }
    }
}
