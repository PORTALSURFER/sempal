use super::*;
use crate::egui_app::controller::hotkeys::{HotkeyAction, HotkeyCommand};
use crate::egui_app::state::DestructiveSelectionEdit;
use crate::egui_app::state::FocusContext;
use crate::egui_app::state::SampleBrowserTab;
use crate::sample_sources::SampleTag;

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
        match action.command() {
            HotkeyCommand::Undo => {
                self.undo();
            }
            HotkeyCommand::Redo => {
                self.redo();
            }
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
            HotkeyCommand::NormalizeWaveform => {
                if matches!(focus, FocusContext::Waveform) {
                    self.normalize_waveform_selection_or_sample();
                }
            }
            HotkeyCommand::CropSelection => {
                if matches!(focus, FocusContext::Waveform) {
                    let _ = self.request_destructive_selection_edit(
                        DestructiveSelectionEdit::CropSelection,
                    );
                }
            }
            HotkeyCommand::CropSelectionNewSample => {
                if matches!(focus, FocusContext::Waveform) {
                    if let Err(err) = self.crop_waveform_selection_to_new_sample() {
                        self.set_status(err, StatusTone::Error);
                    }
                }
            }
            HotkeyCommand::SaveSelectionToBrowser => {
                if matches!(focus, FocusContext::Waveform) {
                    if !self.ui.waveform.slices.is_empty() {
                        match self.accept_waveform_slices() {
                            Ok(count) => {
                                self.set_status(
                                    format!("Saved {count} slices"),
                                    StatusTone::Info,
                                );
                            }
                            Err(err) => self.set_status(err, StatusTone::Error),
                        }
                    } else if let Err(err) = self.save_waveform_selection_to_browser(true) {
                        self.set_status(err, StatusTone::Error);
                    }
                }
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
            HotkeyCommand::RenameFocusedCollection => {
                if matches!(
                    focus,
                    FocusContext::CollectionsList | FocusContext::CollectionSample
                ) {
                    self.start_collection_rename();
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
                    if matches!(self.ui.browser.active_tab, SampleBrowserTab::Map) {
                        self.ui.map.focus_selected_requested = true;
                    } else {
                        self.focus_browser_search();
                    }
                }
            }
            HotkeyCommand::FindSimilarFocusedSample => {
                if matches!(focus, FocusContext::SampleBrowser) {
                    if matches!(self.ui.browser.active_tab, SampleBrowserTab::Map) {
                        self.ui.browser.active_tab = SampleBrowserTab::List;
                    }
                    if self.ui.browser.similar_query.is_some() {
                        self.clear_similar_filter();
                    } else if let Some(row) = self.focused_browser_row() {
                        if let Err(err) = self.find_similar_for_visible_row(row) {
                            self.set_status(format!("Find similar failed: {err}"), StatusTone::Error);
                        }
                    } else {
                        self.set_status("Focus a sample to find similar", StatusTone::Info);
                    }
                }
            }
            HotkeyCommand::AddFocusedToCollection => {
                if matches!(focus, FocusContext::SampleBrowser) {
                    self.add_focused_sample_to_collection();
                }
            }
            HotkeyCommand::SelectAllBrowser => {
                if matches!(focus, FocusContext::SampleBrowser) {
                    self.select_all_browser_rows();
                }
            }
            HotkeyCommand::ToggleOverlay => {
                self.ui.hotkeys.overlay_visible = !self.ui.hotkeys.overlay_visible;
            }
            HotkeyCommand::OpenFeedbackIssuePrompt => {
                self.ui.hotkeys.overlay_visible = false;
                self.open_feedback_issue_prompt();
            }
            HotkeyCommand::CopyStatusLog => {
                self.copy_status_log_to_clipboard();
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
            HotkeyCommand::TagKeepSelected => {
                if matches!(focus, FocusContext::CollectionSample) {
                    self.tag_selected_collection_sample(SampleTag::Keep);
                } else {
                    self.tag_selected(SampleTag::Keep);
                }
            }
            HotkeyCommand::TagNeutralSelected => {
                if matches!(focus, FocusContext::CollectionSample) {
                    self.tag_selected_collection_sample(SampleTag::Neutral);
                } else {
                    self.tag_selected(SampleTag::Neutral);
                }
            }
            HotkeyCommand::TagTrashSelected => {
                if matches!(focus, FocusContext::CollectionSample) {
                    self.tag_selected_collection_sample(SampleTag::Trash);
                } else {
                    self.tag_selected(SampleTag::Trash);
                }
            }
            HotkeyCommand::TrimSelection => {
                if matches!(focus, FocusContext::Waveform) {
                    let _ = self.request_destructive_selection_edit(
                        DestructiveSelectionEdit::TrimSelection,
                    );
                }
            }
            HotkeyCommand::ReverseSelection => {
                if matches!(focus, FocusContext::Waveform | FocusContext::SampleBrowser) {
                    let _ = self.request_destructive_selection_edit(
                        DestructiveSelectionEdit::ReverseSelection,
                    );
                }
            }
            HotkeyCommand::FadeSelectionLeftToRight => {
                if matches!(focus, FocusContext::Waveform) {
                    let _ = self.request_destructive_selection_edit(
                        DestructiveSelectionEdit::FadeLeftToRight,
                    );
                }
            }
            HotkeyCommand::FadeSelectionRightToLeft => {
                if matches!(focus, FocusContext::Waveform) {
                    let _ = self.request_destructive_selection_edit(
                        DestructiveSelectionEdit::FadeRightToLeft,
                    );
                }
            }
            HotkeyCommand::MuteSelection => {
                if matches!(focus, FocusContext::Waveform) {
                    let _ = self.request_destructive_selection_edit(
                        DestructiveSelectionEdit::MuteSelection,
                    );
                }
            }
        }
    }
}

impl HotkeysController<'_> {
    fn normalize_waveform_selection_or_sample(&mut self) {
        if self
            .ui
            .waveform
            .selection
            .is_some_and(|selection| selection.width() > 0.0)
        {
            let _ = self
                .request_destructive_selection_edit(DestructiveSelectionEdit::NormalizeSelection);
            return;
        }
        if let Err(err) = self.normalize_loaded_sample_like_browser() {
            self.set_status(err, StatusTone::Error);
        }
    }

    fn normalize_loaded_sample_like_browser(&mut self) -> Result<(), String> {
        let preserved_view = self.ui.waveform.view;
        let preserved_cursor = self.ui.waveform.cursor;
        let preserved_selection = self.ui.waveform.selection;
        let was_playing = self.is_playing();
        let was_looping = self.ui.waveform.loop_enabled;
        let playhead_position = self.ui.waveform.playhead.position;
        let audio = self
            .sample_view
            .wav
            .loaded_audio
            .as_ref()
            .ok_or_else(|| "Load a sample to normalize it".to_string())?;
        let source = self
            .library
            .sources
            .iter()
            .find(|s| s.id == audio.source_id)
            .cloned()
            .ok_or_else(|| "Source not available for loaded sample".to_string())?;
        let relative_path = audio.relative_path.clone();
        let absolute_path = source.root.join(&relative_path);
        let (file_size, modified_ns, tag) =
            self.normalize_and_save_for_path(&source, &relative_path, &absolute_path)?;
        self.upsert_metadata_for_source(&source, &relative_path, file_size, modified_ns)?;
        let updated = WavEntry {
            relative_path: relative_path.clone(),
            file_size,
            modified_ns,
            content_hash: None,
            tag,
            missing: false,
        };
        self.update_cached_entry(&source, &relative_path, updated);
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
            self.rebuild_browser_lists();
        }
        self.refresh_waveform_for_sample(&source, &relative_path);
        self.reexport_collections_for_sample(&source.id, &relative_path);
        self.ui.waveform.view = preserved_view.clamp();
        self.ui.waveform.cursor = preserved_cursor;
        self.selection_state.range.set_range(preserved_selection);
        self.apply_selection(preserved_selection);
        if was_playing {
            let start_override = if playhead_position.is_finite() {
                Some(playhead_position.clamp(0.0, 1.0))
            } else {
                None
            };
            if let Err(err) = self.play_audio(was_looping, start_override) {
                self.set_status(err, StatusTone::Error);
            }
        }
        self.set_status(
            format!("Normalized {}", relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
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
                let selected_paths = self.ui.browser.selected_paths.clone();
                let mut rows: Vec<usize> = selected_paths
                    .iter()
                    .filter_map(|path| self.visible_row_for_path(path))
                    .collect();
                if let Some(row) = self.focused_browser_row() {
                    if rows.is_empty() {
                        rows = self.action_rows_from_primary(row);
                    } else if !rows.contains(&row) {
                        rows.push(row);
                    }
                }
                if rows.is_empty() {
                    self.set_status("Focus a sample to delete it", StatusTone::Info);
                    return;
                }
                rows.sort_unstable();
                rows.dedup();
                let _ = self.delete_browser_samples(&rows);
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

impl EguiController {
    pub(crate) fn handle_hotkey(&mut self, action: HotkeyAction, focus: FocusContext) {
        self.hotkeys_ctrl().handle_hotkey(action, focus);
    }
}
