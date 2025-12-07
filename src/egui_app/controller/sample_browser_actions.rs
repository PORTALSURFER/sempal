use super::collection_export;
use super::collection_items_helpers::file_metadata;
use super::*;
use crate::sample_sources::collections::CollectionMember;
use std::path::{Path, PathBuf};

struct TriageSampleContext {
    source: SampleSource,
    entry: WavEntry,
    absolute_path: PathBuf,
}

impl EguiController {
    /// Apply a triage flag to a sample shown in the sample browser.
    pub fn tag_browser_sample(&mut self, row: usize, tag: SampleTag) -> Result<(), String> {
        let result: Result<(), String> = (|| {
            let ctx = self.resolve_browser_sample(row)?;
            self.set_sample_tag_for_source(&ctx.source, &ctx.entry.relative_path, tag, true)?;
            self.set_status(
                format!("Tagged {} as {:?}", ctx.entry.relative_path.display(), tag),
                StatusTone::Info,
            );
            Ok(())
        })();
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Apply a triage flag to all targeted rows.
    pub fn tag_browser_samples(&mut self, rows: &[usize], tag: SampleTag) -> Result<(), String> {
        let mut last_error = None;
        for &row in rows {
            if let Err(err) = self.tag_browser_sample(row, tag) {
                last_error = Some(err);
            }
        }
        if let Some(err) = last_error {
            Err(err)
        } else {
            Ok(())
        }
    }

    /// Normalize a sample browser entry in place.
    pub fn normalize_browser_sample(&mut self, row: usize) -> Result<(), String> {
        let result = self.try_normalize_browser_sample(row);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Normalize all targeted rows in place.
    pub fn normalize_browser_samples(&mut self, rows: &[usize]) -> Result<(), String> {
        let mut last_error = None;
        for &row in rows {
            if let Err(err) = self.normalize_browser_sample(row) {
                last_error = Some(err);
            }
        }
        if let Some(err) = last_error {
            Err(err)
        } else {
            Ok(())
        }
    }

    /// Rename a sample browser entry and keep caches, collections, and exports in sync.
    pub fn rename_browser_sample(&mut self, row: usize, new_name: &str) -> Result<(), String> {
        let result = self.try_rename_browser_sample(row, new_name);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Delete a sample browser entry from disk, database, caches, and any collections.
    pub fn delete_browser_sample(&mut self, row: usize) -> Result<(), String> {
        let result = self.try_delete_browser_sample(row);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Delete all targeted sample browser entries.
    pub fn delete_browser_samples(&mut self, rows: &[usize]) -> Result<(), String> {
        let mut last_error = None;
        for &row in rows {
            if let Err(err) = self.delete_browser_sample(row) {
                last_error = Some(err);
            }
        }
        if let Some(err) = last_error {
            Err(err)
        } else {
            Ok(())
        }
    }

    fn try_normalize_browser_sample(&mut self, row: usize) -> Result<(), String> {
        let ctx = self.resolve_browser_sample(row)?;
        let (file_size, modified_ns, tag) = self.normalize_and_save_for_path(
            &ctx.source,
            &ctx.entry.relative_path,
            &ctx.absolute_path,
        )?;
        self.upsert_metadata_for_source(
            &ctx.source,
            &ctx.entry.relative_path,
            file_size,
            modified_ns,
        )?;
        let updated = WavEntry {
            relative_path: ctx.entry.relative_path.clone(),
            file_size,
            modified_ns,
            tag,
        };
        self.update_cached_entry(&ctx.source, &ctx.entry.relative_path, updated);
        if self.selected_source.as_ref() == Some(&ctx.source.id) {
            self.rebuild_browser_lists();
        }
        self.refresh_waveform_for_sample(&ctx.source, &ctx.entry.relative_path);
        self.reexport_collections_for_sample(&ctx.source.id, &ctx.entry.relative_path);
        self.set_status(
            format!("Normalized {}", ctx.entry.relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }

    fn try_delete_browser_sample(&mut self, row: usize) -> Result<(), String> {
        let ctx = self.resolve_browser_sample(row)?;
        std::fs::remove_file(&ctx.absolute_path)
            .map_err(|err| format!("Failed to delete file: {err}"))?;
        let db = self
            .database_for(&ctx.source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.remove_file(&ctx.entry.relative_path)
            .map_err(|err| format!("Failed to drop database row: {err}"))?;
        self.prune_cached_sample(&ctx.source, &ctx.entry.relative_path);
        let collections_changed =
            self.remove_sample_from_collections(&ctx.source.id, &ctx.entry.relative_path);
        if collections_changed {
            self.persist_config("Failed to save collection after delete")?;
        }
        self.set_status(
            format!("Deleted {}", ctx.entry.relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }

    fn try_rename_browser_sample(&mut self, row: usize, new_name: &str) -> Result<(), String> {
        let ctx = self.resolve_browser_sample(row)?;
        let tag = self.sample_tag_for(&ctx.source, &ctx.entry.relative_path)?;
        let new_relative = self.validate_new_sample_name_in_parent(
            &ctx.entry.relative_path,
            &ctx.source.root,
            new_name,
        )?;
        let collections_changed = self.commit_browser_rename(&ctx, &new_relative, tag)?;
        if collections_changed {
            self.persist_config("Failed to save collection after rename")?;
        }
        self.set_status(
            format!(
                "Renamed {} to {}",
                ctx.entry.relative_path.display(),
                new_relative.display()
            ),
            StatusTone::Info,
        );
        Ok(())
    }

    fn commit_browser_rename(
        &mut self,
        ctx: &TriageSampleContext,
        new_relative: &Path,
        tag: SampleTag,
    ) -> Result<bool, String> {
        let (file_size, modified_ns) = self.apply_triage_rename(&ctx, &new_relative, tag)?;
        let updated_path = new_relative.to_path_buf();
        self.update_cached_entry(
            &ctx.source,
            &ctx.entry.relative_path,
            WavEntry {
                relative_path: updated_path.clone(),
                file_size,
                modified_ns,
                tag,
            },
        );
        self.refresh_waveform_for_sample(&ctx.source, new_relative);
        let collections_changed = self.update_collections_for_rename(
            &ctx.source.id,
            &ctx.entry.relative_path,
            new_relative,
        );
        Ok(collections_changed)
    }

    fn resolve_browser_sample(&self, row: usize) -> Result<TriageSampleContext, String> {
        let source = self
            .current_source()
            .ok_or_else(|| "Select a source first".to_string())?;
        let index = self
            .visible_browser_indices()
            .get(row)
            .copied()
            .ok_or_else(|| "Sample not found".to_string())?;
        let entry = self
            .wav_entries
            .get(index)
            .cloned()
            .ok_or_else(|| "Sample not found".to_string())?;
        let absolute_path = source.root.join(&entry.relative_path);
        Ok(TriageSampleContext {
            source,
            entry,
            absolute_path,
        })
    }

    fn apply_triage_rename(
        &mut self,
        ctx: &TriageSampleContext,
        new_relative: &Path,
        tag: SampleTag,
    ) -> Result<(u64, i64), String> {
        let new_absolute = ctx.source.root.join(new_relative);
        std::fs::rename(&ctx.absolute_path, &new_absolute)
            .map_err(|err| format!("Failed to rename file: {err}"))?;
        let (file_size, modified_ns) = file_metadata(&new_absolute)?;
        if let Err(err) = self.rewrite_db_entry_for_source(
            &ctx.source,
            &ctx.entry.relative_path,
            new_relative,
            file_size,
            modified_ns,
            tag,
        ) {
            let _ = std::fs::rename(&new_absolute, &ctx.absolute_path);
            return Err(err);
        }
        Ok((file_size, modified_ns))
    }

    pub(super) fn prune_cached_sample(&mut self, source: &SampleSource, relative_path: &Path) {
        if let Some(cache) = self.wav_cache.get_mut(&source.id) {
            cache.retain(|entry| entry.relative_path != relative_path);
        }
        if self.selected_source.as_ref() == Some(&source.id) {
            self.wav_entries
                .retain(|entry| entry.relative_path != relative_path);
            self.rebuild_wav_lookup();
            self.rebuild_browser_lists();
            self.label_cache
                .insert(source.id.clone(), self.build_label_cache(&self.wav_entries));
        } else {
            self.label_cache.remove(&source.id);
        }
        self.clear_loaded_sample_if(source, relative_path);
    }

    pub(super) fn clear_loaded_sample_if(&mut self, source: &SampleSource, relative_path: &Path) {
        if self.selected_source.as_ref() == Some(&source.id) {
            if self.selected_wav.as_deref() == Some(relative_path) {
                self.selected_wav = None;
            }
            if self.loaded_wav.as_deref() == Some(relative_path) {
                self.loaded_wav = None;
            }
            if self.ui.loaded_wav.as_deref() == Some(relative_path) {
                self.ui.loaded_wav = None;
            }
        }
        if let Some(audio) = self.loaded_audio.as_ref() {
            if audio.source_id == source.id && audio.relative_path == relative_path {
                self.loaded_audio = None;
                self.decoded_waveform = None;
                self.ui.waveform.image = None;
                self.ui.waveform.playhead = PlayheadState::default();
                self.ui.waveform.selection = None;
                self.selection.clear();
            }
        }
    }

    pub(super) fn refresh_waveform_for_sample(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) {
        let loaded_matches = self.loaded_audio.as_ref().is_some_and(|audio| {
            audio.source_id == source.id && audio.relative_path == relative_path
        });
        let selected_matches = self.selected_source.as_ref() == Some(&source.id)
            && self.selected_wav.as_deref() == Some(relative_path);
        if loaded_matches || selected_matches {
            self.loaded_wav = None;
            self.ui.loaded_wav = None;
            if let Err(err) = self.load_waveform_for_selection(source, relative_path) {
                self.set_status(err, StatusTone::Warning);
            }
        }
    }

    fn reexport_collections_for_sample(&mut self, source_id: &SourceId, relative_path: &Path) {
        let mut targets = Vec::new();
        for collection in self.collections.iter() {
            if collection
                .members
                .iter()
                .any(|m| &m.source_id == source_id && m.relative_path == relative_path)
            {
                targets.push((
                    collection.id.clone(),
                    collection.export_path.clone(),
                    collection_export::collection_folder_name(collection),
                ));
            }
        }
        let member = CollectionMember {
            source_id: source_id.clone(),
            relative_path: relative_path.to_path_buf(),
        };
        for (collection_id, export_root, folder_name) in targets {
            collection_export::delete_exported_file(export_root.clone(), &folder_name, &member);
            if let Err(err) = self.export_member_if_needed(&collection_id, &member) {
                self.set_status(err, StatusTone::Warning);
            }
        }
    }

    fn update_collections_for_rename(
        &mut self,
        source_id: &SourceId,
        old_relative: &Path,
        new_relative: &Path,
    ) -> bool {
        let mut changed = false;
        let mut exports: Vec<(CollectionId, Option<PathBuf>, String)> = Vec::new();
        for collection in self.collections.iter_mut() {
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
                    collection.export_path.clone(),
                    collection_export::collection_folder_name(collection),
                ));
            }
        }
        if changed {
            let member = CollectionMember {
                source_id: source_id.clone(),
                relative_path: new_relative.to_path_buf(),
            };
            for (collection_id, export_root, folder_name) in exports {
                let old_member = CollectionMember {
                    source_id: source_id.clone(),
                    relative_path: old_relative.to_path_buf(),
                };
                collection_export::delete_exported_file(
                    export_root.clone(),
                    &folder_name,
                    &old_member,
                );
                if let Err(err) = self.export_member_if_needed(&collection_id, &member) {
                    self.set_status(err, StatusTone::Warning);
                }
            }
            self.refresh_collections_ui();
        }
        changed
    }

    pub(super) fn remove_sample_from_collections(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
    ) -> bool {
        let mut changed = false;
        for collection in self.collections.iter_mut() {
            let member = CollectionMember {
                source_id: source_id.clone(),
                relative_path: relative_path.to_path_buf(),
            };
            if collection.remove_member(source_id, &member.relative_path) {
                changed = true;
                let folder_name = collection_export::collection_folder_name(collection);
                collection_export::delete_exported_file(
                    collection.export_path.clone(),
                    &folder_name,
                    &member,
                );
            }
        }
        if changed {
            self.refresh_collections_ui();
        }
        changed
    }
}
