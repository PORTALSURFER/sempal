use super::*;

pub(crate) trait CollectionsActions {
    fn select_collection_sample(&mut self, index: usize);
    fn select_collection_by_index(&mut self, index: Option<usize>);
    fn nudge_collection_row(&mut self, offset: isize);
    fn add_collection(&mut self);
    fn delete_collection(&mut self, collection_id: &CollectionId) -> Result<(), String>;
    fn rename_collection(&mut self, collection_id: &CollectionId, new_name: String);
    fn add_sample_to_collection(
        &mut self,
        collection_id: &CollectionId,
        relative_path: &Path,
    ) -> Result<(), String>;
    fn add_sample_to_collection_for_source(
        &mut self,
        collection_id: &CollectionId,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String>;
    fn nudge_collection_sample(&mut self, offset: isize);
}

impl CollectionsActions for CollectionsController<'_> {
    fn select_collection_sample(&mut self, index: usize) {
        let Some(collection) = self.current_collection() else {
            return;
        };
        let Some(member) = collection.members.get(index).cloned() else {
            return;
        };
        self.apply_collection_sample_selection_ui(&collection.id, index);
        let target_path = member.relative_path.clone();
        let Some(source) = self.collection_member_source(&member) else {
            self.set_status("Source not available for this sample", StatusTone::Warning);
            return;
        };
        self.sample_view.wav.selected_wav = None;
        self.clear_waveform_view();
        if self.collection_member_missing(&member) {
            self.show_missing_waveform_notice(&target_path);
            self.set_status(
                format!("File missing: {}", target_path.display()),
                StatusTone::Warning,
            );
            return;
        }
        if let Err(err) = self.load_collection_waveform(&source, &target_path) {
            self.set_status(err, StatusTone::Error);
            return;
        }
        self.maybe_autoplay_selection();
    }

    fn select_collection_by_index(&mut self, index: Option<usize>) {
        if let Some(idx) = index {
            if let Some(collection) = self.library.collections.get(idx).cloned() {
                self.selection_state.ctx.selected_collection = Some(collection.id);
            }
        } else {
            self.selection_state.ctx.selected_collection = None;
        }
        self.ui.collections.selected_sample = None;
        self.ui.collections.scroll_to_sample = None;
        self.clear_focus_context();
        self.ui.browser.autoscroll = false;
        self.refresh_collection_selection_ui();
        self.refresh_collection_samples();
    }

    fn nudge_collection_row(&mut self, offset: isize) {
        if self.library.collections.is_empty() {
            return;
        }
        let current = self.ui.collections.selected.unwrap_or(0) as isize;
        let target =
            (current + offset).clamp(0, self.library.collections.len() as isize - 1) as usize;
        self.select_collection_by_index(Some(target));
        self.focus_collections_list_context();
    }

    fn add_collection(&mut self) {
        if !self.settings.feature_flags.collections_enabled {
            return;
        }
        let name = self.next_collection_name();
        let mut collection = Collection::new(name);
        let id = collection.id.clone();
        collection.members.clear();
        self.library.collections.push(collection);
        self.selection_state.ctx.selected_collection = Some(id);
        let _ = self.persist_config("Failed to save collection");
        self.refresh_collections_ui();
        self.set_status("Collection created", StatusTone::Info);
        if self.settings.collection_export_root.is_none()
            && let Some(current_id) = self.selection_state.ctx.selected_collection.clone()
        {
            self.pick_collection_export_path(&current_id);
            if self
                .library
                .collections
                .iter()
                .any(|c| c.id == current_id && c.export_path.is_none())
            {
                self.set_status(
                    "No export folder chosen; exports disabled",
                    StatusTone::Warning,
                );
            }
        }
    }

    fn delete_collection(&mut self, collection_id: &CollectionId) -> Result<(), String> {
        let result: Result<String, String> = (|| {
            let Some(index) = self
                .library
                .collections
                .iter()
                .position(|c| &c.id == collection_id)
            else {
                return Err("Collection not found".into());
            };
            let removed = self.library.collections.remove(index);
            if self.selection_state.ctx.selected_collection.as_ref() == Some(collection_id) {
                self.selection_state.ctx.selected_collection = None;
                self.ui.collections.selected_sample = None;
            }
            self.ensure_collection_selection();
            self.persist_config("Failed to save collection after delete")?;
            self.refresh_collections_ui();
            Ok(removed.name)
        })();

        match result {
            Ok(name) => {
                self.set_status(format!("Removed collection '{name}'"), StatusTone::Info);
                Ok(())
            }
            Err(err) => {
                self.set_status(err.clone(), StatusTone::Error);
                Err(err)
            }
        }
    }

    fn rename_collection(&mut self, collection_id: &CollectionId, new_name: String) {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            self.set_status("Collection name cannot be empty", StatusTone::Error);
            return;
        }
        let Some(index) = self
            .library
            .collections
            .iter()
            .position(|c| &c.id == collection_id)
        else {
            self.set_status("Collection not found", StatusTone::Error);
            return;
        };
        let old_name = self.library.collections[index].name.clone();
        let new_folder_name = collection_export::collection_folder_name_from_str(trimmed);
        let mut clip_root_update: Option<(std::path::PathBuf, std::path::PathBuf)> = None;
        if let Some(old_folder) = collection_export::resolved_export_dir(
            &self.library.collections[index],
            self.settings.collection_export_root.as_deref(),
        ) {
            if let Some(parent) = old_folder.parent() {
                let new_folder = parent.join(&new_folder_name);
                if old_folder != new_folder {
                    if new_folder.exists() {
                        self.set_status(
                            format!("Export folder already exists: {}", new_folder.display()),
                            StatusTone::Error,
                        );
                        return;
                    }
                    if old_folder.exists() {
                        if let Err(err) = std::fs::rename(&old_folder, &new_folder) {
                            self.set_status(
                                format!("Failed to rename export folder: {err}"),
                                StatusTone::Error,
                            );
                            return;
                        }
                    } else if let Err(err) = std::fs::create_dir_all(&new_folder) {
                        self.set_status(
                            format!("Failed to create export folder: {err}"),
                            StatusTone::Error,
                        );
                        return;
                    }
                    if self.library.collections[index].export_path.is_some() {
                        self.library.collections[index].export_path = Some(new_folder.clone());
                    }
                    clip_root_update = Some((old_folder, new_folder));
                }
            }
        }
        self.library.collections[index].name = trimmed.to_string();
        if let Some((old_root, new_root)) = clip_root_update.as_ref() {
            for member in self.library.collections[index].members.iter_mut() {
                if member.clip_root.as_ref() == Some(old_root) {
                    member.clip_root = Some(new_root.clone());
                }
            }
        }
        if let Err(err) = self.persist_config("Failed to save collection") {
            self.set_status(err, StatusTone::Error);
            return;
        }
        self.refresh_collections_ui();
        self.set_status(
            format!("Renamed collection '{old_name}' to '{}'", trimmed),
            StatusTone::Info,
        );
    }

    fn add_sample_to_collection(
        &mut self,
        collection_id: &CollectionId,
        relative_path: &Path,
    ) -> Result<(), String> {
        if !self.settings.feature_flags.collections_enabled {
            return Err("Collections are disabled".into());
        }
        let Some(source) = self.current_source() else {
            return Err("Select a source first".into());
        };
        self.add_sample_to_collection_for_source(collection_id, &source, relative_path)
    }

    fn add_sample_to_collection_for_source(
        &mut self,
        collection_id: &CollectionId,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        if !self.settings.feature_flags.collections_enabled {
            return Err("Collections are disabled".into());
        }
        self.add_sample_to_collection_inner(collection_id, source, relative_path)
    }

    fn nudge_collection_sample(&mut self, offset: isize) {
        let Some(selected_row) = self.ui.collections.selected_sample else {
            return;
        };
        let total = self.ui.collections.samples.len();
        if total == 0 {
            return;
        }
        self.ui.browser.autoscroll = false;
        self.ui.browser.selected = None;
        let current = selected_row as isize;
        let next = (current + offset).clamp(0, total as isize - 1) as usize;
        self.select_collection_sample(next);
    }
}

impl CollectionsController<'_> {
    fn apply_collection_sample_selection_ui(&mut self, collection_id: &CollectionId, index: usize) {
        self.selection_state.ctx.selected_collection = Some(collection_id.clone());
        self.ui.collections.selected_sample = Some(index);
        self.ui.collections.scroll_to_sample = Some(index);
        self.focus_collection_context();
        self.ui.browser.selected = None;
        self.ui.browser.autoscroll = false;
        self.refresh_collection_selection_ui();
    }

    fn maybe_autoplay_selection(&mut self) {
        if !self.settings.feature_flags.autoplay_selection {
            return;
        }
        let looped = self.ui.waveform.loop_enabled;
        let _ = self.play_audio(looped, None);
    }
}
