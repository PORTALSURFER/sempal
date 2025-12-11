use super::collection_export;
use super::*;
use crate::sample_sources::SampleTag;
use crate::sample_sources::collections::CollectionMember;
use std::path::PathBuf;

impl EguiController {
    /// Select a sample from the collection list and ensure it plays.
    pub fn select_collection_sample(&mut self, index: usize) {
        let Some(collection) = self.current_collection() else {
            return;
        };
        let Some(member) = collection.members.get(index) else {
            return;
        };
        self.selected_collection = Some(collection.id.clone());
        self.ui.collections.selected_sample = Some(index);
        self.focus_collection_context();
        self.ui.browser.selected = None;
        self.ui.browser.autoscroll = false;
        self.refresh_collections_ui();
        let target_source = member.source_id.clone();
        let target_path = member.relative_path.clone();
        let Some(source) = self.sources.iter().find(|s| s.id == target_source).cloned() else {
            self.set_status("Source not available for this sample", StatusTone::Warning);
            return;
        };
        if Some(&target_source) != self.selected_source.as_ref() {
            self.selected_source = Some(target_source.clone());
            self.selected_wav = None;
            self.loaded_wav = None;
            self.refresh_sources_ui();
            self.queue_wav_load();
            let _ = self.persist_config("Failed to save selection");
        }
        self.selected_wav = None;
        self.loaded_wav = None;
        self.ui.loaded_wav = None;
        if self.sample_missing(&target_source, &target_path) {
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
        if self.feature_flags.autoplay_selection {
            let _ = self.play_audio(self.ui.waveform.loop_enabled, None);
        }
    }

    /// Switch selected collection by index.
    pub fn select_collection_by_index(&mut self, index: Option<usize>) {
        if let Some(idx) = index {
            if let Some(collection) = self.collections.get(idx).cloned() {
                self.selected_collection = Some(collection.id);
            }
        } else {
            self.selected_collection = None;
        }
        self.ui.collections.selected_sample = None;
        self.clear_focus_context();
        self.ui.browser.autoscroll = false;
        self.refresh_collections_ui();
    }

    /// Move collection selection up or down by an offset.
    pub fn nudge_collection_row(&mut self, offset: isize) {
        if self.collections.is_empty() {
            return;
        }
        let current = self.ui.collections.selected.unwrap_or(0) as isize;
        let target = (current + offset).clamp(0, self.collections.len() as isize - 1) as usize;
        self.select_collection_by_index(Some(target));
        self.focus_collections_list_context();
    }

    /// Create a new collection and persist.
    pub fn add_collection(&mut self) {
        if !self.feature_flags.collections_enabled {
            return;
        }
        let name = self.next_collection_name();
        let mut collection = Collection::new(name);
        let id = collection.id.clone();
        collection.members.clear();
        self.collections.push(collection);
        self.selected_collection = Some(id);
        let _ = self.persist_config("Failed to save collection");
        self.refresh_collections_ui();
        self.set_status("Collection created", StatusTone::Info);
        if self.collection_export_root.is_none() {
            if let Some(current_id) = self.selected_collection.clone() {
                self.pick_collection_export_path(&current_id);
                if self
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
    }

    /// Remove a collection by id and refresh related UI state.
    pub fn delete_collection(&mut self, collection_id: &CollectionId) -> Result<(), String> {
        let result: Result<String, String> = (|| {
            let Some(index) = self.collections.iter().position(|c| &c.id == collection_id) else {
                return Err("Collection not found".into());
            };
            let removed = self.collections.remove(index);
            if self.selected_collection.as_ref() == Some(collection_id) {
                self.selected_collection = None;
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

    /// Rename a collection and its export folder if configured.
    pub fn rename_collection(&mut self, collection_id: &CollectionId, new_name: String) {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            self.set_status("Collection name cannot be empty", StatusTone::Error);
            return;
        }
        let Some(index) = self.collections.iter().position(|c| &c.id == collection_id) else {
            self.set_status("Collection not found", StatusTone::Error);
            return;
        };
        let old_name = self.collections[index].name.clone();
        let new_folder_name = collection_export::collection_folder_name_from_str(trimmed);
        if let Some(old_folder) = collection_export::resolved_export_dir(
            &self.collections[index],
            self.collection_export_root.as_deref(),
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
                    if self.collections[index].export_path.is_some() {
                        self.collections[index].export_path = Some(new_folder);
                    }
                }
            }
        }
        self.collections[index].name = trimmed.to_string();
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

    /// Add a sample to the given collection id.
    pub fn add_sample_to_collection(
        &mut self,
        collection_id: &CollectionId,
        relative_path: &Path,
    ) -> Result<(), String> {
        if !self.feature_flags.collections_enabled {
            return Err("Collections are disabled".into());
        }
        let Some(source) = self.current_source() else {
            return Err("Select a source first".into());
        };
        self.add_sample_to_collection_for_source(collection_id, &source, relative_path)
    }

    /// Add a sample to the given collection id using an explicit source (used by selection exports).
    pub fn add_sample_to_collection_for_source(
        &mut self,
        collection_id: &CollectionId,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        if !self.feature_flags.collections_enabled {
            return Err("Collections are disabled".into());
        }
        self.add_sample_to_collection_inner(collection_id, source, relative_path)
    }

    pub fn nudge_collection_sample(&mut self, offset: isize) {
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

    pub fn current_collection_id(&self) -> Option<CollectionId> {
        self.selected_collection.clone()
    }

    pub(super) fn refresh_collections_ui(&mut self) {
        let selected_id = self.selected_collection.clone();
        let member_refs: Vec<Vec<(SourceId, PathBuf)>> = self
            .collections
            .iter()
            .map(|collection| {
                collection
                    .members
                    .iter()
                    .map(|member| (member.source_id.clone(), member.relative_path.clone()))
                    .collect()
            })
            .collect();
        let collection_missing: Vec<bool> = member_refs
            .iter()
            .map(|members| {
                members
                    .iter()
                    .any(|(source_id, path)| self.sample_missing(source_id, path))
            })
            .collect();
        self.ui.collections.rows = view_model::collection_rows(
            &self.collections,
            selected_id.as_ref(),
            &collection_missing,
            self.collection_export_root.as_deref(),
        );
        self.ui.collections.selected = selected_id
            .as_ref()
            .and_then(|id| self.collections.iter().position(|c| &c.id == id));
        self.refresh_collection_samples();
    }

    pub(super) fn refresh_collection_samples(&mut self) {
        let selected = self
            .selected_collection
            .as_ref()
            .and_then(|id| self.collections.iter().find(|c| &c.id == id))
            .cloned();
        let sources = self.sources.clone();
        let mut tag_error: Option<String> = None;
        let sample_missing_refs = selected.as_ref().map(|collection| {
            collection
                .members
                .iter()
                .map(|member| (member.source_id.clone(), member.relative_path.clone()))
                .collect::<Vec<_>>()
        });
        let sample_missing_flags = sample_missing_refs.as_ref().map(|refs| {
            refs.iter()
                .map(|(source_id, path)| self.sample_missing(source_id, path))
                .collect::<Vec<bool>>()
        });
        let missing_slice = sample_missing_flags.as_deref();
        self.ui.collections.samples =
            view_model::collection_samples(selected.as_ref(), &sources, missing_slice, |member| {
                match self.tag_for_collection_member(member) {
                    Ok(tag) => tag,
                    Err(err) => {
                        if tag_error.is_none() {
                            tag_error = Some(err);
                        }
                        SampleTag::Neutral
                    }
                }
            });
        if let Some(err) = tag_error {
            self.set_status(err, StatusTone::Warning);
        }
        let len = self.ui.collections.samples.len();
        if len == 0 {
            self.ui.collections.selected_sample = None;
            self.clear_collection_focus_context();
        } else if let Some(selected) = self.ui.collections.selected_sample
            && selected >= len
        {
            self.ui.collections.selected_sample = Some(len.saturating_sub(1));
            self.focus_collection_context();
        }
    }

    fn tag_for_collection_member(
        &mut self,
        member: &CollectionMember,
    ) -> Result<SampleTag, String> {
        let source = self
            .sources
            .iter()
            .find(|s| s.id == member.source_id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "Source not available for {}",
                    member.relative_path.display()
                )
            })?;
        self.sample_tag_for(&source, &member.relative_path)
    }

    pub(super) fn ensure_collection_selection(&mut self) {
        if self.selected_collection.is_some() {
            return;
        }
        if let Some(first) = self.collections.first().cloned() {
            self.selected_collection = Some(first.id);
        }
    }

    /// Make sure the sample exists in the source database before attaching to a collection.
    pub(super) fn ensure_sample_db_entry(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        let full_path = source.root.join(relative_path);
        let metadata = fs::metadata(&full_path)
            .map_err(|err| format!("Missing file for collection: {err}"))?;
        let modified_ns = metadata
            .modified()
            .map_err(|err| format!("Missing mtime for collection: {err}"))?
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| "File modified time is before epoch".to_string())?
            .as_nanos() as i64;
        let file_size = metadata.len();
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.upsert_file(relative_path, file_size, modified_ns)
            .map_err(|err| format!("Failed to sync collection entry: {err}"))
    }

    pub(super) fn current_collection(&self) -> Option<Collection> {
        let selected = self.selected_collection.as_ref()?;
        self.collections.iter().find(|c| &c.id == selected).cloned()
    }

    fn add_sample_to_collection_inner(
        &mut self,
        collection_id: &CollectionId,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        self.ensure_sample_db_entry(source, relative_path)?;
        let mut collections = self.collections.clone();
        let Some(collection) = collections.iter_mut().find(|c| &c.id == collection_id) else {
            return Err("Collection not found".into());
        };
        let new_member = CollectionMember {
            source_id: source.id.clone(),
            relative_path: relative_path.to_path_buf(),
        };
        let added = collection.add_member(
            new_member.source_id.clone(),
            new_member.relative_path.clone(),
        );
        self.collections = collections;
        if !added {
            self.set_status("Already in collection", StatusTone::Info);
            return Ok(());
        }
        self.finalize_collection_add(collection_id, &new_member, relative_path)
    }

    fn finalize_collection_add(
        &mut self,
        collection_id: &CollectionId,
        member: &CollectionMember,
        relative_path: &Path,
    ) -> Result<(), String> {
        self.persist_config("Failed to save collection")?;
        self.refresh_collections_ui();
        if let Err(err) = self.export_member_if_needed(collection_id, member) {
            self.set_status(err, StatusTone::Warning);
        }
        self.set_status(
            format!("Added {} to collection", relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }

    fn next_collection_name(&self) -> String {
        let base = "Collection";
        let mut index = self.collections.len() + 1;
        loop {
            let candidate = format!("{base} {index}");
            if !self.collections.iter().any(|c| c.name == candidate) {
                return candidate;
            }
            index += 1;
        }
    }
}
