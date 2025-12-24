use super::*;

impl EguiController {
    /// Apply a keep/trash/neutral tag to a single visible browser row.
    pub fn tag_browser_sample(&mut self, row: usize, tag: SampleTag) -> Result<(), String> {
        self.browser().tag_browser_sample(row, tag)
    }

    /// Apply a keep/trash/neutral tag to multiple visible browser rows.
    pub fn tag_browser_samples(
        &mut self,
        rows: &[usize],
        tag: SampleTag,
        primary_visible_row: usize,
    ) -> Result<(), String> {
        self.browser()
            .tag_browser_samples(rows, tag, primary_visible_row)
    }

    /// Normalize a single visible browser row in-place (overwrites audio).
    pub fn normalize_browser_sample(&mut self, row: usize) -> Result<(), String> {
        self.browser().normalize_browser_sample(row)
    }

    /// Normalize multiple visible browser rows in-place (overwrites audio).
    pub fn normalize_browser_samples(&mut self, rows: &[usize]) -> Result<(), String> {
        self.browser().normalize_browser_samples(rows)
    }

    /// Rename a single visible browser row on disk and refresh dependent state.
    pub fn rename_browser_sample(&mut self, row: usize, new_name: &str) -> Result<(), String> {
        self.browser().rename_browser_sample(row, new_name)
    }

    /// Delete the file for a single visible browser row and prune references.
    pub fn delete_browser_sample(&mut self, row: usize) -> Result<(), String> {
        self.browser().delete_browser_sample(row)
    }

    /// Delete files for multiple visible browser rows and prune references.
    pub fn delete_browser_samples(&mut self, rows: &[usize]) -> Result<(), String> {
        self.browser().delete_browser_samples(rows)
    }

    /// Remove dead-link browser rows (missing samples) from the library without deleting files.
    pub fn remove_dead_link_browser_sample(&mut self, row: usize) -> Result<(), String> {
        self.browser().remove_dead_link_browser_samples(&[row])
    }

    /// Remove dead-link browser rows (missing samples) from the library without deleting files.
    pub fn remove_dead_link_browser_samples(&mut self, rows: &[usize]) -> Result<(), String> {
        self.browser().remove_dead_link_browser_samples(rows)
    }

    pub(in crate::egui_app::controller) fn resolve_browser_sample(
        &mut self,
        row: usize,
    ) -> Result<helpers::TriageSampleContext, String> {
        let source = self
            .current_source()
            .ok_or_else(|| "Select a source first".to_string())?;
        let index = self
            .visible_browser_index(row)
            .ok_or_else(|| "Sample not found".to_string())?;
        let entry = self
            .wav_entry(index)
            .cloned()
            .ok_or_else(|| "Sample not found".to_string())?;
        let absolute_path = source.root.join(&entry.relative_path);
        Ok(helpers::TriageSampleContext {
            source,
            entry,
            absolute_path,
        })
    }

    pub(in crate::egui_app::controller) fn prune_cached_sample(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) {
        if let Some(cache) = self.cache.wav.entries.get_mut(&source.id) {
            cache.clear();
        }
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
            self.wav_entries.clear();
            self.sync_after_wav_entries_changed();
            self.queue_wav_load();
        } else {
            self.ui_cache.browser.labels.remove(&source.id);
        }
        self.rebuild_missing_lookup_for_source(&source.id);
        self.clear_loaded_sample_if(source, relative_path);
    }

    pub(in crate::egui_app::controller) fn clear_loaded_sample_if(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) {
        self.invalidate_cached_audio(&source.id, relative_path);
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
            if self.sample_view.wav.selected_wav.as_deref() == Some(relative_path) {
                self.sample_view.wav.selected_wav = None;
            }
            if self.sample_view.wav.loaded_wav.as_deref() == Some(relative_path) {
                self.sample_view.wav.loaded_wav = None;
            }
            if self.ui.loaded_wav.as_deref() == Some(relative_path) {
                self.ui.loaded_wav = None;
            }
        }
        if let Some(audio) = self.sample_view.wav.loaded_audio.as_ref()
            && audio.source_id == source.id
            && audio.relative_path == relative_path
        {
            self.clear_loaded_audio_and_waveform_visuals();
        }
    }

    pub(in crate::egui_app::controller) fn refresh_waveform_for_sample(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) {
        self.reload_waveform_for_selection_if_active(source, relative_path);
    }

    pub(in crate::egui_app::controller) fn reexport_collections_for_sample(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
    ) {
        let mut targets = Vec::new();
        for collection in self.library.collections.iter() {
            if collection
                .members
                .iter()
                .any(|m| &m.source_id == source_id && m.relative_path == relative_path)
            {
                targets.push((
                    collection.id.clone(),
                    collection_export::resolved_export_dir(
                        collection,
                        self.settings.collection_export_root.as_deref(),
                    ),
                ));
            }
        }
        let member = CollectionMember {
            source_id: source_id.clone(),
            relative_path: relative_path.to_path_buf(),
            clip_root: None,
        };
        for (collection_id, export_dir) in targets {
            collection_export::delete_exported_file(export_dir.clone(), &member);
            if let Err(err) = self.export_member_if_needed(&collection_id, &member) {
                self.set_status(err, StatusTone::Warning);
            }
        }
    }

    pub(in crate::egui_app::controller) fn update_collections_for_rename(
        &mut self,
        source_id: &SourceId,
        old_relative: &Path,
        new_relative: &Path,
    ) -> bool {
        let mut changed = false;
        let mut exports: Vec<(CollectionId, Option<PathBuf>)> = Vec::new();
        for collection in self.library.collections.iter_mut() {
            let mut touched = false;
            for member in collection.members.iter_mut() {
                if &member.source_id == source_id && member.relative_path == old_relative {
                    member.relative_path = new_relative.to_path_buf();
                    touched = true;
                    changed = true;
                }
            }
            if touched {
                exports.push((
                    collection.id.clone(),
                    collection_export::resolved_export_dir(
                        collection,
                        self.settings.collection_export_root.as_deref(),
                    ),
                ));
            }
        }
        if changed {
            let member = CollectionMember {
                source_id: source_id.clone(),
                relative_path: new_relative.to_path_buf(),
                clip_root: None,
            };
            for (collection_id, export_dir) in exports {
                let old_member = CollectionMember {
                    source_id: source_id.clone(),
                    relative_path: old_relative.to_path_buf(),
                    clip_root: None,
                };
                collection_export::delete_exported_file(export_dir.clone(), &old_member);
                if let Err(err) = self.export_member_if_needed(&collection_id, &member) {
                    self.set_status(err, StatusTone::Warning);
                }
            }
            self.refresh_collections_ui();
        }
        changed
    }

    pub(in crate::egui_app::controller) fn remove_sample_from_collections(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
    ) -> bool {
        let mut changed = false;
        for collection in self.library.collections.iter_mut() {
            let member = CollectionMember {
                source_id: source_id.clone(),
                relative_path: relative_path.to_path_buf(),
                clip_root: None,
            };
            if collection.remove_member(source_id, &member.relative_path) {
                changed = true;
                let export_dir = collection_export::resolved_export_dir(
                    collection,
                    self.settings.collection_export_root.as_deref(),
                );
                collection_export::delete_exported_file(export_dir, &member);
            }
        }
        if changed {
            self.refresh_collections_ui();
        }
        changed
    }

    pub(in crate::egui_app::controller) fn refocus_after_filtered_removal(
        &mut self,
        primary_visible_row: usize,
    ) {
        if matches!(self.ui.browser.filter, TriageFlagFilter::All) {
            return;
        }
        if self.ui.browser.visible.len() == 0 || self.ui.browser.selected_visible.is_some() {
            return;
        }
        if self.random_navigation_mode_enabled() {
            self.focus_random_visible_sample();
            return;
        }
        let target_row = primary_visible_row.min(self.ui.browser.visible.len().saturating_sub(1));
        self.focus_browser_row_only(target_row);
    }
}
