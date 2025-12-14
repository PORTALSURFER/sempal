use super::collection_export;
use super::*;
use crate::sample_sources::collections::CollectionMember;
use hound::SampleFormat;
use std::path::{Path, PathBuf};

pub(super) struct CollectionSampleContext {
    pub(super) collection_id: CollectionId,
    pub(super) member: CollectionMember,
    pub(super) source: SampleSource,
    pub(super) absolute_path: PathBuf,
    pub(super) row: usize,
}

impl EguiController {
    pub(super) fn validate_new_sample_name(
        &self,
        ctx: &CollectionSampleContext,
        new_name: &str,
    ) -> Result<PathBuf, String> {
        self.validate_new_sample_name_in_parent(
            &ctx.member.relative_path,
            &ctx.source.root,
            new_name,
        )
    }

    pub(super) fn validate_new_sample_name_in_parent(
        &self,
        relative_path: &Path,
        root: &Path,
        new_name: &str,
    ) -> Result<PathBuf, String> {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err("Name cannot be empty".into());
        }
        if trimmed.contains(['/', '\\']) {
            return Err("Name cannot contain path separators".into());
        }
        let parent = relative_path.parent().unwrap_or(Path::new(""));
        let new_relative = parent.join(trimmed);
        let new_absolute = root.join(&new_relative);
        if new_absolute.exists() {
            return Err(format!(
                "A file named {} already exists",
                new_relative.display()
            ));
        }
        Ok(new_relative)
    }

    /// Build a sanitized sample name that keeps the existing file extension.
    pub(super) fn name_with_preserved_extension(
        &self,
        current_relative: &Path,
        new_name: &str,
    ) -> Result<String, String> {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err("Name cannot be empty".into());
        }
        let Some(ext) = current_relative.extension().and_then(|ext| ext.to_str()) else {
            return Ok(trimmed.to_string());
        };
        let ext_lower = ext.to_ascii_lowercase();
        let should_strip_suffix = |suffix: &str| -> bool {
            let suffix_lower = suffix.to_ascii_lowercase();
            suffix_lower == ext_lower
                || matches!(
                    suffix_lower.as_str(),
                    "wav" | "wave" | "flac" | "aif" | "aiff" | "mp3" | "ogg" | "opus"
                )
        };
        let stem = if let Some((stem, suffix)) = trimmed.rsplit_once('.') {
            if !stem.is_empty() && should_strip_suffix(suffix) {
                stem
            } else {
                trimmed
            }
        } else {
            trimmed
        };
        let stem = stem.trim_end_matches('.');
        if stem.trim().is_empty() {
            return Err("Name cannot be empty".into());
        }
        Ok(format!("{stem}.{ext}"))
    }

    pub(super) fn apply_rename(
        &mut self,
        ctx: &CollectionSampleContext,
        new_relative: &Path,
        tag: SampleTag,
    ) -> Result<(u64, i64), String> {
        let new_absolute = ctx.source.root.join(new_relative);
        std::fs::rename(&ctx.absolute_path, &new_absolute)
            .map_err(|err| format!("Failed to rename file: {err}"))?;
        let (file_size, modified_ns) = file_metadata(&new_absolute)?;
        if let Err(err) = self.rewrite_db_entry(ctx, new_relative, file_size, modified_ns, tag) {
            let _ = std::fs::rename(&new_absolute, &ctx.absolute_path);
            return Err(err);
        }
        Ok((file_size, modified_ns))
    }

    pub(super) fn resolve_collection_sample(
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
            self.sources
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

    pub(super) fn drop_collection_member(&mut self, ctx: &CollectionSampleContext) -> bool {
        let Some(collection) = self
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

    pub(super) fn rewrite_db_entry(
        &mut self,
        ctx: &CollectionSampleContext,
        new_relative: &Path,
        file_size: u64,
        modified_ns: i64,
        tag: SampleTag,
    ) -> Result<(), String> {
        self.rewrite_db_entry_for_source(
            &ctx.source,
            &ctx.member.relative_path,
            new_relative,
            file_size,
            modified_ns,
            tag,
        )
    }

    pub(super) fn rewrite_db_entry_for_source(
        &mut self,
        source: &SampleSource,
        old_relative: &Path,
        new_relative: &Path,
        file_size: u64,
        modified_ns: i64,
        tag: SampleTag,
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        let mut batch = db
            .write_batch()
            .map_err(|err| format!("Failed to start database update: {err}"))?;
        batch
            .remove_file(old_relative)
            .map_err(|err| format!("Failed to drop old entry: {err}"))?;
        batch
            .upsert_file(new_relative, file_size, modified_ns)
            .map_err(|err| format!("Failed to register renamed file: {err}"))?;
        batch
            .set_tag(new_relative, tag)
            .map_err(|err| format!("Failed to copy tag: {err}"))?;
        batch
            .commit()
            .map_err(|err| format!("Failed to save rename: {err}"))
    }

    pub(super) fn upsert_metadata(
        &mut self,
        ctx: &CollectionSampleContext,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<(), String> {
        self.upsert_metadata_for_source(
            &ctx.source,
            &ctx.member.relative_path,
            file_size,
            modified_ns,
        )
    }

    pub(super) fn upsert_metadata_for_source(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.upsert_file(relative_path, file_size, modified_ns)
            .map_err(|err| format!("Failed to refresh metadata: {err}"))
    }

    pub(super) fn normalize_and_save(
        &mut self,
        ctx: &CollectionSampleContext,
    ) -> Result<(u64, i64, SampleTag), String> {
        self.normalize_and_save_for_path(&ctx.source, &ctx.member.relative_path, &ctx.absolute_path)
    }

    pub(super) fn normalize_and_save_for_path(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
        absolute_path: &Path,
    ) -> Result<(u64, i64, SampleTag), String> {
        let (samples, spec) = read_samples_for_normalization(absolute_path)?;
        if samples.is_empty() {
            return Err("No audio data to normalize".into());
        }
        let peak = samples
            .iter()
            .fold(0.0_f32, |acc, sample| acc.max(sample.abs()));
        if peak <= f32::EPSILON {
            return Err("Cannot normalize silent audio".into());
        }
        let scale = 1.0 / peak;
        let normalized: Vec<f32> = samples
            .iter()
            .map(|s| (s * scale).clamp(-1.0, 1.0))
            .collect();
        let target_spec = hound::WavSpec {
            channels: spec.channels.max(1),
            sample_rate: spec.sample_rate.max(1),
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        write_normalized_wav(absolute_path, &normalized, target_spec)?;
        let (file_size, modified_ns) = file_metadata(absolute_path)?;
        let tag = self.sample_tag_for(source, relative_path)?;
        Ok((file_size, modified_ns, tag))
    }

    pub(super) fn sample_tag_for(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) -> Result<SampleTag, String> {
        if self.cache.wav.entries.contains_key(&source.id) {
            self.ensure_wav_cache_lookup(&source.id);
            if let Some(lookup) = self.cache.wav.lookup.get(&source.id)
                && let Some(index) = lookup.get(relative_path).copied()
                && let Some(cache) = self.cache.wav.entries.get(&source.id)
                && let Some(entry) = cache.get(index)
            {
                return Ok(entry.tag);
            }
        }
        if self.selection_ctx.selected_source.as_ref() == Some(&source.id)
            && let Some(entry) = self
                .wav_entries
                .entries
                .iter()
                .find(|entry| entry.relative_path == relative_path)
        {
            return Ok(entry.tag);
        }
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        let entries = db
            .list_files()
            .map_err(|err| format!("Failed to read database: {err}"))?;
        self.cache.wav.entries.insert(source.id.clone(), entries.clone());
        self.rebuild_wav_cache_lookup(&source.id);
        self.missing.wavs.insert(
            source.id.clone(),
            entries
                .iter()
                .filter(|entry| entry.missing)
                .map(|entry| entry.relative_path.clone())
                .collect::<std::collections::HashSet<_>>(),
        );
        entries
            .iter()
            .find(|entry| entry.relative_path == relative_path)
            .map(|entry| entry.tag)
            .ok_or_else(|| "Sample not found in database".to_string())
    }

    pub(super) fn update_collection_member_path(
        &mut self,
        ctx: &CollectionSampleContext,
        new_relative: &Path,
    ) -> Result<(), String> {
        let Some(collection) = self
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

    pub(super) fn update_cached_entry(
        &mut self,
        source: &SampleSource,
        old_path: &Path,
        new_entry: WavEntry,
    ) {
        if let Some(cache) = self.cache.wav.entries.get_mut(&source.id) {
            replace_entry(cache, old_path, &new_entry);
            self.rebuild_wav_cache_lookup(&source.id);
        }
        if self.selection_ctx.selected_source.as_ref() == Some(&source.id) {
            replace_entry(&mut self.wav_entries.entries, old_path, &new_entry);
            self.sync_browser_after_wav_entries_mutation(&source.id);
        }
        self.rebuild_missing_lookup_for_source(&source.id);
        self.update_selection_paths(source, old_path, &new_entry.relative_path);
        self.invalidate_cached_audio(&source.id, old_path);
        if old_path != new_entry.relative_path {
            self.invalidate_cached_audio(&source.id, &new_entry.relative_path);
        }
    }

    pub(super) fn insert_cached_entry(&mut self, source: &SampleSource, entry: WavEntry) {
        if let Some(cache) = self.cache.wav.entries.get_mut(&source.id) {
            cache.push(entry.clone());
            cache.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
            self.rebuild_wav_cache_lookup(&source.id);
        }
        if self.selection_ctx.selected_source.as_ref() == Some(&source.id) {
            self.wav_entries.entries.push(entry.clone());
            self.wav_entries
                .entries
                .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
            self.sync_browser_after_wav_entries_mutation(&source.id);
        }
        self.rebuild_missing_lookup_for_source(&source.id);
        self.invalidate_cached_audio(&source.id, &entry.relative_path);
    }

    pub(super) fn update_selection_paths(
        &mut self,
        source: &SampleSource,
        old_path: &Path,
        new_path: &Path,
    ) {
        if self.selection_ctx.selected_source.as_ref() == Some(&source.id) {
            if self.wav_selection.selected_wav.as_deref() == Some(old_path) {
                self.wav_selection.selected_wav = Some(new_path.to_path_buf());
            }
            if self.wav_selection.loaded_wav.as_deref() == Some(old_path) {
                self.wav_selection.loaded_wav = Some(new_path.to_path_buf());
                self.ui.loaded_wav = Some(new_path.to_path_buf());
            } else if self.ui.loaded_wav.as_deref() == Some(old_path) {
                self.ui.loaded_wav = Some(new_path.to_path_buf());
            }
        }
        if let Some(audio) = self.wav_selection.loaded_audio.as_mut()
            && audio.source_id == source.id
            && audio.relative_path == old_path
        {
            audio.relative_path = new_path.to_path_buf();
        }
    }

    pub(super) fn refresh_waveform_after_change(
        &mut self,
        ctx: &CollectionSampleContext,
        relative_path: &Path,
    ) {
        let loaded_matches = self.wav_selection.loaded_audio.as_ref().is_some_and(|audio| {
            audio.source_id == ctx.source.id && audio.relative_path == relative_path
        });
        let selected_matches = self
            .selection_ctx
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

    pub(super) fn update_export_after_change(
        &mut self,
        ctx: &CollectionSampleContext,
        new_relative: &Path,
    ) {
        if let Some(collection) = self.collections.iter().find(|c| c.id == ctx.collection_id) {
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

pub(super) fn read_samples_for_normalization(
    path: &Path,
) -> Result<(Vec<f32>, hound::WavSpec), String> {
    let bytes = crate::wav_sanitize::read_sanitized_wav_bytes(path)?;
    let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes))
        .map_err(|err| format!("Invalid wav: {err}"))?;
    let spec = reader.spec();
    let samples = match spec.sample_format {
        SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.map_err(|err| format!("Sample error: {err}")))
            .collect::<Result<Vec<_>, _>>()?,
        SampleFormat::Int => {
            let scale = (1i64 << spec.bits_per_sample.saturating_sub(1)).max(1) as f32;
            reader
                .samples::<i32>()
                .map(|s| {
                    s.map(|value| value as f32 / scale)
                        .map_err(|err| format!("Sample error: {err}"))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    Ok((samples, spec))
}

pub(super) fn write_normalized_wav(
    path: &Path,
    samples: &[f32],
    spec: hound::WavSpec,
) -> Result<(), String> {
    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|err| format!("Failed to write wav: {err}"))?;
    for sample in samples {
        writer
            .write_sample(*sample)
            .map_err(|err| format!("Failed to write sample: {err}"))?;
    }
    writer
        .finalize()
        .map_err(|err| format!("Failed to finalize wav: {err}"))
}

pub(super) fn replace_entry(list: &mut Vec<WavEntry>, old_path: &Path, new_entry: &WavEntry) {
    if let Some(pos) = list
        .iter()
        .position(|entry| entry.relative_path == old_path)
    {
        list[pos] = new_entry.clone();
    } else {
        list.push(new_entry.clone());
    }
    list.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
}

pub(super) fn file_metadata(path: &Path) -> Result<(u64, i64), String> {
    let metadata = std::fs::metadata(path)
        .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    let modified_ns = metadata
        .modified()
        .map_err(|err| format!("Missing modified time for {}: {err}", path.display()))?
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map_err(|_| "File modified time is before epoch".to_string())?
        .as_nanos() as i64;
    Ok((metadata.len(), modified_ns))
}
