use super::super::collection_export;
use super::super::*;
use super::CollectionSampleContext;
use crate::sample_sources::collections::CollectionMember;
use std::path::Path;

impl EguiController {
    pub(in crate::egui_app::controller) fn resolve_collection_sample(
        &self,
        row: usize,
    ) -> Result<CollectionSampleContext, String> {
        let collection = self
            .current_collection()
            .ok_or_else(|| "Select a collection first".to_string())?;
        let member = collection
            .members
            .get(row)
            .cloned()
            .ok_or_else(|| "Sample not found".to_string())?;
        let source = if let Some(root) = member.clip_root.as_ref() {
            SampleSource {
                id: member.source_id.clone(),
                root: root.clone(),
            }
        } else {
            self.library
                .sources
                .iter()
                .find(|s| s.id == member.source_id)
                .cloned()
                .ok_or_else(|| "Source not available for this sample".to_string())?
        };
        Ok(CollectionSampleContext {
            collection_id: collection.id,
            absolute_path: source.root.join(&member.relative_path),
            member,
            source,
            row,
        })
    }

    pub(in crate::egui_app::controller) fn drop_collection_member(
        &mut self,
        ctx: &CollectionSampleContext,
    ) -> bool {
        let Some(collection) = self
            .library
            .collections
            .iter_mut()
            .find(|c| c.id == ctx.collection_id)
        else {
            return false;
        };
        let export_dir = collection_export::resolved_export_dir(
            collection,
            self.settings.collection_export_root.as_deref(),
        );
        let removed = collection.remove_member(&ctx.member.source_id, &ctx.member.relative_path);
        if removed {
            collection_export::delete_exported_file(export_dir, &ctx.member);
        }
        removed
    }

    pub(in crate::egui_app::controller) fn update_collection_member_path(
        &mut self,
        ctx: &CollectionSampleContext,
        new_relative: &Path,
    ) -> Result<(), String> {
        let Some(collection) = self
            .library
            .collections
            .iter_mut()
            .find(|c| c.id == ctx.collection_id)
        else {
            return Err("Collection not found".into());
        };
        let Some(member) = collection.members.get_mut(ctx.row) else {
            return Err("Sample not found".into());
        };
        member.relative_path = new_relative.to_path_buf();
        Ok(())
    }

    pub(in crate::egui_app::controller) fn update_cached_entry(
        &mut self,
        source: &SampleSource,
        old_path: &Path,
        new_entry: WavEntry,
    ) {
        self.update_selection_paths(source, old_path, &new_entry.relative_path);
        self.invalidate_cached_audio(&source.id, old_path);
        if let Some(missing) = self.library.missing.wavs.get_mut(&source.id) {
            let removed = missing.remove(old_path);
            if removed && new_entry.missing {
                missing.insert(new_entry.relative_path.clone());
            }
        }
        if old_path == new_entry.relative_path {
            let mut updated = false;
            if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
                updated |= self
                    .wav_entries
                    .update_entry(old_path, new_entry.clone());
            }
            if let Some(cache) = self.cache.wav.entries.get_mut(&source.id) {
                updated |= cache.update_entry(old_path, new_entry.clone());
            }
            if updated && self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
                self.rebuild_browser_lists();
            }
            return;
        }
        if let Ok(db) = self.database_for(source)
            && matches!(db.index_for_path(old_path), Ok(Some(_)))
        {
            let _ = self.rewrite_db_entry_for_source(
                source,
                old_path,
                &new_entry.relative_path,
                new_entry.file_size,
                new_entry.modified_ns,
                new_entry.tag,
            );
        }
        let mut updated = false;
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
            if let Some(index) = self.wav_entries.lookup.get(old_path).copied()
                && let Some(slot) = self.wav_entries.entry_mut(index)
            {
                *slot = new_entry.clone();
                self.wav_entries.lookup.remove(old_path);
                self.wav_entries
                    .insert_lookup(new_entry.relative_path.clone(), index);
                updated = true;
            }
            if self.ui.browser.last_focused_path.as_deref() == Some(old_path) {
                self.ui.browser.last_focused_path = Some(new_entry.relative_path.clone());
            }
        }
        if let Some(cache) = self.cache.wav.entries.get_mut(&source.id) {
            if let Some(index) = cache.lookup.get(old_path).copied()
                && let Some(slot) = cache.entry_mut(index)
            {
                *slot = new_entry.clone();
                cache.lookup.remove(old_path);
                cache.insert_lookup(new_entry.relative_path.clone(), index);
                updated = true;
            }
        }
        if updated {
            if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
                self.ui_cache.browser.search.invalidate();
                self.rebuild_browser_lists();
            }
            if old_path != new_entry.relative_path {
                self.ui_cache.browser.labels.remove(&source.id);
            }
        } else {
            self.invalidate_wav_entries_for_source_preserve_folders(source);
        }
        self.invalidate_cached_audio(&source.id, &new_entry.relative_path);
    }

    pub(in crate::egui_app::controller) fn insert_cached_entry(
        &mut self,
        source: &SampleSource,
        entry: WavEntry,
    ) {
        self.invalidate_wav_entries_for_source(source);
        self.invalidate_cached_audio(&source.id, &entry.relative_path);
    }

    pub(in crate::egui_app::controller) fn update_selection_paths(
        &mut self,
        source: &SampleSource,
        old_path: &Path,
        new_path: &Path,
    ) {
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
            if !self.ui.browser.selected_paths.is_empty() {
                let mut updated = Vec::with_capacity(self.ui.browser.selected_paths.len());
                let mut replaced = false;
                for path in self.ui.browser.selected_paths.iter() {
                    if path == old_path {
                        replaced = true;
                        if !updated.iter().any(|candidate| candidate == new_path) {
                            updated.push(new_path.to_path_buf());
                        }
                    } else {
                        updated.push(path.clone());
                    }
                }
                if replaced {
                    self.ui.browser.selected_paths = updated;
                }
            }
            if self.sample_view.wav.selected_wav.as_deref() == Some(old_path) {
                self.sample_view.wav.selected_wav = Some(new_path.to_path_buf());
            }
            if self.sample_view.wav.loaded_wav.as_deref() == Some(old_path) {
                self.sample_view.wav.loaded_wav = Some(new_path.to_path_buf());
                self.ui.loaded_wav = Some(new_path.to_path_buf());
            } else if self.ui.loaded_wav.as_deref() == Some(old_path) {
                self.ui.loaded_wav = Some(new_path.to_path_buf());
            }
        }
        if let Some(audio) = self.sample_view.wav.loaded_audio.as_mut()
            && audio.source_id == source.id
            && audio.relative_path == old_path
        {
            audio.relative_path = new_path.to_path_buf();
        }
    }

    pub(in crate::egui_app::controller) fn refresh_waveform_after_change(
        &mut self,
        ctx: &CollectionSampleContext,
        relative_path: &Path,
    ) {
        let loaded_matches = self
            .sample_view
            .wav
            .loaded_audio
            .as_ref()
            .is_some_and(|audio| {
                audio.source_id == ctx.source.id && audio.relative_path == relative_path
            });
        let selected_matches = self
            .selection_state
            .ctx
            .selected_collection
            .as_ref()
            .is_some_and(|id| id == &ctx.collection_id)
            && self.ui.collections.selected_sample == Some(ctx.row);
        if (loaded_matches || selected_matches)
            && let Err(err) = self.load_collection_waveform(&ctx.source, relative_path)
        {
            self.set_status(err, StatusTone::Warning);
        }
    }

    pub(in crate::egui_app::controller) fn update_export_after_change(
        &mut self,
        ctx: &CollectionSampleContext,
        new_relative: &Path,
    ) {
        if let Some(collection) = self
            .library
            .collections
            .iter()
            .find(|c| c.id == ctx.collection_id)
        {
            let export_dir = collection_export::resolved_export_dir(
                collection,
                self.settings.collection_export_root.as_deref(),
            );
            collection_export::delete_exported_file(export_dir, &ctx.member);
        }
        let new_member = CollectionMember {
            source_id: ctx.member.source_id.clone(),
            relative_path: new_relative.to_path_buf(),
            clip_root: ctx.member.clip_root.clone(),
        };
        if let Err(err) = self.export_member_if_needed(&ctx.collection_id, &new_member) {
            self.set_status(err, StatusTone::Warning);
        }
    }
}
