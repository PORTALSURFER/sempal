use super::*;
use crate::egui_app::controller::hotkeys::{HotkeyAction, HotkeyCommand};
use crate::egui_app::state::FocusContext;
use crate::sample_sources::SampleTag;
use crate::egui_app::state::DestructiveSelectionEdit;

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
            HotkeyCommand::TagKeepSelected => {
                self.tag_selected(SampleTag::Keep);
            }
            HotkeyCommand::TagTrashSelected => {
                self.tag_selected(SampleTag::Trash);
            }
            HotkeyCommand::TrimSelection => {
                if matches!(focus, FocusContext::Waveform) {
                    let _ = self.request_destructive_selection_edit(
                        DestructiveSelectionEdit::TrimSelection,
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
        let audio = self
            .loaded_audio
            .as_ref()
            .ok_or_else(|| "Load a sample to normalize it".to_string())?;
        let source = self
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
            tag,
            missing: false,
        };
        self.update_cached_entry(&source, &relative_path, updated);
        if self.selected_source.as_ref() == Some(&source.id) {
            self.rebuild_browser_lists();
        }
        self.refresh_waveform_for_sample(&source, &relative_path);
        self.reexport_collections_for_sample(&source.id, &relative_path);
        self.set_status(format!("Normalized {}", relative_path.display()), StatusTone::Info);
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

impl EguiController {
    pub(crate) fn handle_hotkey(&mut self, action: HotkeyAction, focus: FocusContext) {
        self.hotkeys_ctrl().handle_hotkey(action, focus);
    }
}
