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
        self.invalidate_wav_entries_for_source(source);
        self.update_selection_paths(source, old_path, &new_entry.relative_path);
        self.invalidate_cached_audio(&source.id, old_path);
        if old_path != new_entry.relative_path {
            self.invalidate_cached_audio(&source.id, &new_entry.relative_path);
        }
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
