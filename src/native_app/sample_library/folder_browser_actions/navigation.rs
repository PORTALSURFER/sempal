use radiant::prelude as ui;
use std::{path::Path, time::Instant};

use crate::native_app::app::{
    GuiMessage, NativeAppState, SampleBrowserDisplayMode, StarmapViewportChange, emit_gui_action,
};
use crate::native_app::sample_library::file_actions::sample_path_label;
use crate::native_app::sample_library::folder_browser::model::PlaybackTypeFilter;
use crate::native_app::sample_library::folder_browser::starmap::StarmapProjection;
use crate::native_app::sample_library::folder_browser::view_contract::{
    COLLECTION_ROW_HEIGHT, COLLECTIONS_LIST_SCROLL_NODE_ID, FOLDER_TREE_EDGE_CONTEXT_ROWS,
    FOLDER_TREE_LIST_ID, FOLDER_TREE_OVERSCAN_ROWS, FOLDER_TREE_PROJECTED_VIEWPORT_ROWS,
    FOLDER_TREE_SELECTION_CONTEXT_ROWS, TREE_ROW_HEIGHT,
};
use crate::native_app::sample_library::folder_browser::{
    BrowserListingRevealReason, commands::FolderBrowserMessage,
};
use crate::native_app::sample_library::sample_list::{
    SAMPLE_BROWSER_EDGE_CONTEXT_ROWS, SAMPLE_BROWSER_LIST_ID, SAMPLE_BROWSER_ROW_HEIGHT,
    SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
};

impl NativeAppState {
    pub(super) fn apply_folder_browser_tag_filter_input(
        &mut self,
        message: radiant::widgets::TextInputMessage,
    ) {
        let previous_visible_ids = self
            .library
            .folder_browser
            .selected_audio_file_ids_matching_tags(&self.metadata.tags_by_file);
        self.library
            .folder_browser
            .apply_message(FolderBrowserMessage::TagFilterInput(message));
        self.library
            .folder_browser
            .reconcile_visible_file_selection_after_tag_filter(
                previous_visible_ids,
                &self.metadata.tags_by_file,
            );
    }

    pub(super) fn toggle_folder_browser_playback_type_filter(
        &mut self,
        filter: PlaybackTypeFilter,
        enabled: bool,
    ) {
        let previous_visible_ids = self
            .library
            .folder_browser
            .selected_audio_file_ids_matching_tags(&self.metadata.tags_by_file);
        self.library
            .folder_browser
            .apply_message(FolderBrowserMessage::TogglePlaybackTypeFilter(
                filter, enabled,
            ));
        self.library
            .folder_browser
            .reconcile_visible_file_selection_after_tag_filter(
                previous_visible_ids,
                &self.metadata.tags_by_file,
            );
    }

    pub(in crate::native_app) fn toggle_similarity_anchor(
        &mut self,
        file_id: String,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        self.library.folder_browser.clear_source_keyboard_focus();
        self.library.folder_browser.hide_keyboard_focus();
        let clearing = self
            .library
            .folder_browser
            .file_is_similarity_anchor(&file_id);
        self.library
            .folder_browser
            .apply_message(FolderBrowserMessage::ToggleSimilarityAnchor(
                file_id.clone(),
            ));
        if clearing {
            self.ui.status.sample = String::from("Similarity mode cleared");
            emit_gui_action(
                "browser.similarity_anchor.clear",
                Some("browser"),
                Some(&sample_path_label(&file_id)),
                "applied",
                started_at,
                None,
            );
        } else {
            context.scroll_into_view_snapped(
                SAMPLE_BROWSER_LIST_ID,
                0.0,
                SAMPLE_BROWSER_ROW_HEIGHT,
                0.0,
                SAMPLE_BROWSER_EDGE_CONTEXT_ROWS as f32 * SAMPLE_BROWSER_ROW_HEIGHT,
                SAMPLE_BROWSER_ROW_HEIGHT,
            );
            self.ui.status.sample = format!(
                "Similarity anchor set to {}",
                sample_path_label(file_id.as_str())
            );
            self.prepare_similarity_for_anchor_path(file_id.as_str(), context);
            self.queue_similarity_score_resolution(file_id.clone(), context);
            emit_gui_action(
                "browser.similarity_anchor.set",
                Some("browser"),
                Some(&sample_path_label(&file_id)),
                "applied",
                started_at,
                None,
            );
        }
    }

    pub(in crate::native_app) fn focus_rename_input(
        &mut self,
        input_id: u64,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        context.focus(input_id);
        emit_gui_action(
            "folder_browser.rename.focus_input",
            Some("folder_browser"),
            None,
            "success",
            started_at,
            None,
        );
    }

    pub(in crate::native_app) fn select_all_samples(&mut self) {
        let started_at = Instant::now();
        let count = self
            .library
            .folder_browser
            .select_all_audio_files_matching_tags(&self.metadata.tags_by_file);
        self.ui.status.sample = format!(
            "Selected {count} sample{}",
            if count == 1 { "" } else { "s" }
        );
        emit_gui_action(
            "browser.select_all_samples",
            Some("browser"),
            None,
            "success",
            started_at,
            None,
        );
    }

    pub(in crate::native_app) fn toggle_random_navigation_mode(&mut self) {
        let started_at = Instant::now();
        let enabled = self.library.folder_browser.toggle_random_navigation();
        self.ui.status.sample = if enabled {
            String::from("Random sample navigation on")
        } else {
            String::from("Random sample navigation off")
        };
        emit_gui_action(
            "browser.random_navigation.toggle",
            Some("browser"),
            Some(if enabled { "on" } else { "off" }),
            "success",
            started_at,
            None,
        );
    }

    pub(in crate::native_app) fn focus_browser_file_for_playback_navigation(
        &mut self,
        path: &Path,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) -> bool {
        let previous_selection = self
            .library
            .folder_browser
            .selected_file_id()
            .map(str::to_owned);
        if !self
            .library
            .folder_browser
            .focus_file_across_sources_matching_tags_for_reason(
                path,
                &self.metadata.tags_by_file,
                BrowserListingRevealReason::HistoryNavigation,
            )
        {
            return false;
        }

        if self.library.folder_browser.selected_file_id() != previous_selection.as_deref() {
            self.cancel_metadata_tag_entry();
            self.metadata.selected_tag = None;
        }
        self.library.folder_browser.sync_tree_view_to_selection(
            FOLDER_TREE_PROJECTED_VIEWPORT_ROWS,
            FOLDER_TREE_OVERSCAN_ROWS,
            FOLDER_TREE_EDGE_CONTEXT_ROWS,
        );
        self.library.folder_browser.reveal_selected_file_if_hidden(
            &self.metadata.tags_by_file,
            BrowserListingRevealReason::HistoryNavigation,
        );
        if let Some(index) = self.library.folder_browser.selected_folder_visible_index() {
            context.scroll_fixed_row_into_view(
                FOLDER_TREE_LIST_ID,
                index,
                TREE_ROW_HEIGHT,
                FOLDER_TREE_SELECTION_CONTEXT_ROWS,
                FOLDER_TREE_SELECTION_CONTEXT_ROWS,
                0,
            );
        }
        if let Some(index) = self
            .library
            .folder_browser
            .selected_audio_file_index_matching_tags(&self.metadata.tags_by_file)
        {
            context.scroll_fixed_row_into_view(
                SAMPLE_BROWSER_LIST_ID,
                index,
                SAMPLE_BROWSER_ROW_HEIGHT,
                SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
                SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
                0,
            );
        }
        true
    }

    pub(in crate::native_app) fn focus_visible_browser_file_after_filter_change(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let previous_selection = self
            .library
            .folder_browser
            .selected_file_id()
            .map(str::to_owned);
        let target = if self
            .library
            .folder_browser
            .selected_audio_file_index_matching_tags(&self.metadata.tags_by_file)
            .is_some()
        {
            previous_selection.clone()
        } else {
            self.library
                .folder_browser
                .selected_audio_files_matching_tags(&self.metadata.tags_by_file)
                .first()
                .map(|file| file.id.clone())
        };
        let Some(target) = target else {
            return;
        };

        if self.library.folder_browser.selected_file_id() != Some(target.as_str()) {
            self.library
                .folder_browser
                .focus_file_preserving_selection_matching_tags(
                    target.clone(),
                    &self.metadata.tags_by_file,
                );
        }
        if let Some(index) = self
            .library
            .folder_browser
            .selected_audio_file_index_matching_tags(&self.metadata.tags_by_file)
        {
            context.scroll_fixed_row_into_view(
                SAMPLE_BROWSER_LIST_ID,
                index,
                SAMPLE_BROWSER_ROW_HEIGHT,
                SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
                SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
                0,
            );
        }
        if self.library.folder_browser.selected_file_id() != previous_selection.as_deref() {
            self.cancel_metadata_tag_entry();
            self.metadata.selected_tag = None;
            self.load_navigation_sample(target, context);
        }
    }

    pub(in crate::native_app) fn toggle_focused_browser_selection(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if self.library.folder_browser.selected_file_id().is_none() {
            self.toggle_focused_folder_selection(context);
            return;
        }
        let started_at = Instant::now();
        let previous_focus = self
            .library
            .folder_browser
            .selected_file_id()
            .map(str::to_owned);
        let Some(result) = self
            .library
            .folder_browser
            .toggle_focused_sample_selection(&self.metadata.tags_by_file)
        else {
            self.ui.status.sample = String::from("Select a sample to mark");
            emit_gui_action(
                "browser.toggle_sample_selection",
                Some("browser"),
                None,
                "short_circuit",
                started_at,
                None,
            );
            return;
        };
        if result.toggled_selected {
            self.library
                .folder_browser
                .flash_marked_item(result.toggled_id.clone());
        } else {
            self.library
                .folder_browser
                .clear_marked_item_flash(&result.toggled_id);
        }

        if self.library.folder_browser.selected_file_id() != previous_focus.as_deref() {
            self.cancel_metadata_tag_entry();
            self.metadata.selected_tag = None;
        }
        if let Some(index) = self
            .library
            .folder_browser
            .selected_audio_file_index_matching_tags(&self.metadata.tags_by_file)
        {
            context.scroll_fixed_row_into_view(
                SAMPLE_BROWSER_LIST_ID,
                index,
                SAMPLE_BROWSER_ROW_HEIGHT,
                SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
                SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
                1,
            );
        }
        self.focus_selected_starmap_node_after_browser_navigation();

        let action = if result.toggled_selected {
            "Marked"
        } else {
            "Unmarked"
        };
        let count = self.library.folder_browser.selected_audio_file_count();
        self.ui.status.sample = format!(
            "{action} {} ({count} selected)",
            sample_path_label(&result.toggled_id)
        );
        emit_gui_action(
            "browser.toggle_sample_selection",
            Some("browser"),
            Some(&sample_path_label(&result.toggled_id)),
            "success",
            started_at,
            None,
        );
    }

    pub(in crate::native_app) fn navigate_browser(
        &mut self,
        delta: i32,
        extend: bool,
        preserve_selection: bool,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let direction = if delta < 0 { "previous" } else { "next" };
        self.library.folder_browser.clear_source_keyboard_focus();
        if self
            .library
            .folder_browser
            .collection_keyboard_focus_active()
        {
            if let Some(index) = self
                .library
                .folder_browser
                .navigate_selected_collection(delta)
            {
                context.scroll_fixed_row_into_view(
                    COLLECTIONS_LIST_SCROLL_NODE_ID,
                    index,
                    COLLECTION_ROW_HEIGHT,
                    1,
                    1,
                    delta,
                );
                emit_gui_action(
                    "folder_browser.navigate",
                    Some("collections"),
                    Some(direction),
                    "selected_collection",
                    started_at,
                    None,
                );
            } else {
                emit_gui_action(
                    "folder_browser.navigate",
                    Some("collections"),
                    Some(direction),
                    "edge",
                    started_at,
                    None,
                );
            }
            return;
        }
        let previous_selection = self
            .library
            .folder_browser
            .selected_file_id()
            .map(str::to_owned);
        let previous_file_index = self.fast_or_full_selected_audio_file_index();
        let previous_folder = self
            .library
            .folder_browser
            .selected_folder_id()
            .map(str::to_owned);
        if self.ui.chrome.sample_browser_display == SampleBrowserDisplayMode::Map
            && self.library.folder_browser.selected_file_id().is_some()
        {
            let Some(path) = self.library.folder_browser.navigate_starmap_matching_tags(
                delta,
                extend,
                &self.metadata.tags_by_file,
                &self.waveform.cache.instant_audition_sample_paths,
            ) else {
                emit_gui_action(
                    "folder_browser.navigate",
                    Some("starmap"),
                    Some(direction),
                    "edge",
                    started_at,
                    None,
                );
                return;
            };

            self.focus_selected_starmap_node_after_browser_navigation();
            emit_gui_action(
                "folder_browser.navigate",
                Some("starmap"),
                Some(direction),
                "selected",
                started_at,
                None,
            );
            if self.library.folder_browser.selected_file_id() != previous_selection.as_deref() {
                self.cancel_metadata_tag_entry();
                self.metadata.selected_tag = None;
            }
            self.load_navigation_sample(path, context);
            return;
        }
        let Some(path) = self.library.folder_browser.navigate_vertical_matching_tags(
            delta,
            extend,
            preserve_selection,
            &self.metadata.tags_by_file,
        ) else {
            if self
                .library
                .folder_browser
                .selected_folder_id()
                .is_some_and(|folder| Some(folder) != previous_folder.as_deref())
            {
                self.library.folder_browser.sync_tree_view_to_selection(
                    FOLDER_TREE_PROJECTED_VIEWPORT_ROWS,
                    FOLDER_TREE_OVERSCAN_ROWS,
                    FOLDER_TREE_EDGE_CONTEXT_ROWS,
                );
                if let Some(index) = self.library.folder_browser.selected_folder_visible_index() {
                    context.scroll_fixed_row_into_view(
                        FOLDER_TREE_LIST_ID,
                        index,
                        TREE_ROW_HEIGHT,
                        FOLDER_TREE_SELECTION_CONTEXT_ROWS,
                        FOLDER_TREE_SELECTION_CONTEXT_ROWS,
                        delta,
                    );
                }
                emit_gui_action(
                    "folder_browser.navigate",
                    Some("folder_browser"),
                    Some(direction),
                    "selected_folder",
                    started_at,
                    None,
                );
                return;
            }
            emit_gui_action(
                "folder_browser.navigate",
                Some("browser"),
                Some(direction),
                "edge",
                started_at,
                None,
            );
            return;
        };

        if let Some(index) = self.fast_or_full_selected_audio_file_index() {
            let reveal_direction =
                file_navigation_reveal_direction(previous_file_index, index, delta);
            context.scroll_fixed_row_into_view(
                SAMPLE_BROWSER_LIST_ID,
                index,
                SAMPLE_BROWSER_ROW_HEIGHT,
                SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
                SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
                reveal_direction,
            );
        }
        self.focus_selected_starmap_node_after_browser_navigation();
        emit_gui_action(
            "folder_browser.navigate",
            Some("browser"),
            Some(direction),
            "selected",
            started_at,
            None,
        );
        if self.library.folder_browser.selected_file_id() != previous_selection.as_deref() {
            self.cancel_metadata_tag_entry();
            self.metadata.selected_tag = None;
        }
        self.load_navigation_sample(path, context);
    }

    fn fast_or_full_selected_audio_file_index(&self) -> Option<usize> {
        self.library
            .folder_browser
            .selected_audio_file_index_fast_matching_tags(&self.metadata.tags_by_file)
            .unwrap_or_else(|| {
                self.library
                    .folder_browser
                    .selected_audio_file_index_matching_tags(&self.metadata.tags_by_file)
            })
    }

    fn focus_selected_starmap_node_after_browser_navigation(&mut self) {
        if self.ui.chrome.sample_browser_display != SampleBrowserDisplayMode::Map {
            return;
        }
        self.library
            .folder_browser
            .prepare_starmap_layout(&self.metadata.tags_by_file);
        let Some((x, y)) =
            self.library
                .folder_browser
                .selected_starmap_position(StarmapProjection {
                    tags_by_file: &self.metadata.tags_by_file,
                    instant_audition_sample_paths: &self
                        .waveform
                        .cache
                        .instant_audition_sample_paths,
                    preview_audition_sample_paths: self
                        .waveform
                        .cache
                        .preview_audition_sample_paths(),
                })
        else {
            return;
        };
        self.ui
            .chrome
            .starmap_viewport
            .apply_change(StarmapViewportChange::Center { x, y });
    }

    fn toggle_focused_folder_selection(&mut self, context: &mut ui::UiUpdateContext<GuiMessage>) {
        let started_at = Instant::now();
        let Some(result) = self
            .library
            .folder_browser
            .toggle_focused_folder_selection()
        else {
            self.ui.status.sample = String::from("Select a folder to mark");
            emit_gui_action(
                "folder_browser.toggle_folder_selection",
                Some("folder_browser"),
                None,
                "short_circuit",
                started_at,
                None,
            );
            return;
        };
        if result.selected {
            self.library
                .folder_browser
                .flash_marked_item(result.folder_id.clone());
        } else {
            self.library
                .folder_browser
                .clear_marked_item_flash(&result.folder_id);
        }
        if let Some(index) = self.library.folder_browser.selected_folder_visible_index() {
            context.scroll_fixed_row_into_view(
                FOLDER_TREE_LIST_ID,
                index,
                TREE_ROW_HEIGHT,
                FOLDER_TREE_SELECTION_CONTEXT_ROWS,
                FOLDER_TREE_SELECTION_CONTEXT_ROWS,
                1,
            );
        }
        let action = if result.selected {
            "Marked"
        } else {
            "Unmarked"
        };
        self.ui.status.sample = format!(
            "{action} {} ({} selected)",
            sample_path_label(&result.folder_id),
            result.selected_count
        );
        emit_gui_action(
            "folder_browser.toggle_folder_selection",
            Some("folder_browser"),
            Some(&sample_path_label(&result.folder_id)),
            "success",
            started_at,
            None,
        );
    }
}

pub(in crate::native_app) fn file_navigation_reveal_direction(
    previous_index: Option<usize>,
    selected_index: usize,
    fallback_direction: i32,
) -> i32 {
    let Some(previous_index) = previous_index else {
        return fallback_direction;
    };
    if selected_index < previous_index {
        -1
    } else if selected_index > previous_index {
        1
    } else {
        fallback_direction
    }
}

#[cfg(test)]
mod tests {
    use super::file_navigation_reveal_direction;

    #[test]
    fn file_navigation_reveal_direction_follows_actual_row_movement() {
        assert_eq!(file_navigation_reveal_direction(Some(80), 12, 1), -1);
        assert_eq!(file_navigation_reveal_direction(Some(12), 80, -1), 1);
        assert_eq!(file_navigation_reveal_direction(Some(12), 12, 1), 1);
        assert_eq!(file_navigation_reveal_direction(None, 12, -1), -1);
    }
}
