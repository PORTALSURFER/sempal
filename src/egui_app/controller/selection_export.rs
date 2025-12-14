use super::*;
use crate::sample_sources::SampleTag;
use hound::SampleFormat;
use std::io::Cursor;
use std::time::SystemTime;

impl EguiController {
    pub(super) fn export_selection_clip(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
        bounds: SelectionRange,
        target_tag: Option<SampleTag>,
        add_to_browser: bool,
        register_in_source: bool,
    ) -> Result<WavEntry, String> {
        let audio = self.selection_audio(source_id, relative_path)?;
        let source = self
            .sources
            .iter()
            .find(|s| &s.id == source_id)
            .cloned()
            .ok_or_else(|| "Source not available".to_string())?;
        let target_rel = self.next_selection_path_in_dir(&source.root, &audio.relative_path);
        let target_abs = source.root.join(&target_rel);
        let (samples, spec) = crop_selection_samples(&audio, bounds)?;
        write_selection_wav(&target_abs, &samples, spec)?;
        self.record_selection_entry(
            &source,
            target_rel,
            target_tag,
            add_to_browser,
            register_in_source,
        )
    }

    pub(super) fn export_selection_clip_in_folder(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
        bounds: SelectionRange,
        target_tag: Option<SampleTag>,
        add_to_browser: bool,
        register_in_source: bool,
        folder: &Path,
    ) -> Result<WavEntry, String> {
        let audio = self.selection_audio(source_id, relative_path)?;
        let source = self
            .sources
            .iter()
            .find(|s| &s.id == source_id)
            .cloned()
            .ok_or_else(|| "Source not available".to_string())?;
        let name_hint = folder.join(
            audio.relative_path
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("selection.wav")),
        );
        let target_rel = self.next_selection_path_in_dir(&source.root, &name_hint);
        let target_abs = source.root.join(&target_rel);
        let (samples, spec) = crop_selection_samples(&audio, bounds)?;
        write_selection_wav(&target_abs, &samples, spec)?;
        self.record_selection_entry(
            &source,
            target_rel,
            target_tag,
            add_to_browser,
            register_in_source,
        )
    }

    pub(super) fn export_selection_clip_to_root(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
        bounds: SelectionRange,
        target_tag: Option<SampleTag>,
        clip_root: &Path,
        name_hint: &Path,
    ) -> Result<WavEntry, String> {
        let audio = self.selection_audio(source_id, relative_path)?;
        let target_rel = self.next_selection_path_in_dir(clip_root, name_hint);
        let target_abs = clip_root.join(&target_rel);
        let (samples, spec) = crop_selection_samples(&audio, bounds)?;
        write_selection_wav(&target_abs, &samples, spec)?;
        let source = SampleSource {
            id: SourceId::new(),
            root: clip_root.to_path_buf(),
        };
        // Collection-owned clips are not inserted into browser or source DB.
        self.record_selection_entry(&source, target_rel, target_tag, false, false)
    }

    pub(super) fn selection_audio(
        &self,
        source_id: &SourceId,
        relative_path: &Path,
    ) -> Result<LoadedAudio, String> {
        let Some(audio) = self.wav_selection.loaded_audio.as_ref() else {
            return Err("Selection audio not available; load a sample first".into());
        };
        if &audio.source_id != source_id || audio.relative_path != relative_path {
            return Err("Selection no longer matches the loaded sample".into());
        }
        Ok(audio.clone())
    }

    fn next_selection_path_in_dir(&self, root: &Path, original: &Path) -> PathBuf {
        let parent = original.parent().unwrap_or_else(|| Path::new(""));
        let stem = original
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("selection");
        let mut counter = 1;
        loop {
            let suffix = if counter == 1 {
                "sel".to_string()
            } else {
                format!("sel_{counter}")
            };
            let candidate = parent.join(format!("{stem}_{suffix}.wav"));
            let absolute = root.join(&candidate);
            if !absolute.exists() {
                return candidate;
            }
            counter += 1;
        }
    }

    fn record_selection_entry(
        &mut self,
        source: &SampleSource,
        relative_path: PathBuf,
        target_tag: Option<SampleTag>,
        add_to_browser: bool,
        register_in_source: bool,
    ) -> Result<WavEntry, String> {
        let metadata = fs::metadata(source.root.join(&relative_path))
            .map_err(|err| format!("Failed to read saved clip: {err}"))?;
        let modified_ns = metadata
            .modified()
            .map_err(|err| format!("Missing modified time for clip: {err}"))?
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| "Clip modified time is before epoch".to_string())?
            .as_nanos() as i64;
        let entry = WavEntry {
            relative_path,
            file_size: metadata.len(),
            modified_ns,
            tag: target_tag.unwrap_or(SampleTag::Neutral),
            missing: false,
        };
        if register_in_source {
            let db = self
                .database_for(source)
                .map_err(|err| format!("Database unavailable: {err}"))?;
            db.upsert_file(&entry.relative_path, entry.file_size, entry.modified_ns)
                .map_err(|err| format!("Failed to register clip: {err}"))?;
            if entry.tag != SampleTag::Neutral {
                db.set_tag(&entry.relative_path, entry.tag)
                    .map_err(|err| format!("Failed to tag clip: {err}"))?;
            }
            if add_to_browser {
                self.insert_new_wav_entry(source, entry.clone());
            }
        }
        Ok(entry)
    }

    fn insert_new_wav_entry(&mut self, source: &SampleSource, entry: WavEntry) {
        let cache = self.cache.wav.entries.entry(source.id.clone()).or_default();
        cache.push(entry.clone());
        cache.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        self.rebuild_wav_cache_lookup(&source.id);

        if self.selection_state.ctx.selected_source.as_ref() != Some(&source.id) {
            return;
        }
        self.wav_entries.entries.push(entry);
        self.wav_entries
            .entries
            .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        self.sync_browser_after_wav_entries_mutation_keep_search_cache(&source.id);
        self.rebuild_missing_lookup_for_source(&source.id);
    }
}

fn crop_selection_samples(
    audio: &LoadedAudio,
    bounds: SelectionRange,
) -> Result<(Vec<f32>, hound::WavSpec), String> {
    let mut reader = hound::WavReader::new(Cursor::new(audio.bytes.as_slice()))
        .map_err(|err| format!("Invalid wav: {err}"))?;
    let spec = reader.spec();
    let channels = audio.channels.max(1) as usize;
    let samples = decode_samples(
        &mut reader,
        spec.sample_format,
        spec.bits_per_sample,
        channels,
    )?;
    let total_frames = samples.len() / channels;
    if total_frames == 0 {
        return Err("No audio data to export".into());
    }
    let (start_frame, end_frame) = frame_bounds(total_frames, bounds);
    let cropped = slice_frames(&samples, channels, start_frame, end_frame);
    let spec = hound::WavSpec {
        channels: audio.channels.max(1),
        sample_rate: audio.sample_rate.max(1),
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    Ok((cropped, spec))
}

fn decode_samples(
    reader: &mut hound::WavReader<Cursor<&[u8]>>,
    format: SampleFormat,
    bits_per_sample: u16,
    _channels: usize,
) -> Result<Vec<f32>, String> {
    match format {
        SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.map_err(|err| format!("Sample error: {err}")))
            .collect::<Result<Vec<_>, _>>(),
        SampleFormat::Int => {
            let scale = (1i64 << bits_per_sample.saturating_sub(1)).max(1) as f32;
            reader
                .samples::<i32>()
                .map(|s| {
                    s.map(|v| v as f32 / scale)
                        .map_err(|err| format!("Sample error: {err}"))
                })
                .collect::<Result<Vec<_>, _>>()
        }
    }
}

fn frame_bounds(total_frames: usize, bounds: SelectionRange) -> (usize, usize) {
    let start_frame = ((bounds.start() * total_frames as f32).floor() as usize)
        .min(total_frames.saturating_sub(1));
    let mut end_frame = ((bounds.end() * total_frames as f32).ceil() as usize).min(total_frames);
    if end_frame <= start_frame {
        end_frame = (start_frame + 1).min(total_frames);
    }
    (start_frame, end_frame)
}

fn slice_frames(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
) -> Vec<f32> {
    let mut cropped = Vec::with_capacity((end_frame - start_frame) * channels);
    for frame in start_frame..end_frame {
        let offset = frame * channels;
        cropped.extend_from_slice(&samples[offset..offset + channels]);
    }
    cropped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_app::controller::test_support::write_test_wav;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn export_selection_clip_to_root_can_flatten_name_hint() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let clip_root = temp.path().join("export");
        std::fs::create_dir_all(source_root.join("drums")).unwrap();
        std::fs::create_dir_all(&clip_root).unwrap();

        let renderer = crate::waveform::WaveformRenderer::new(12, 12);
        let mut controller = EguiController::new(renderer, None);
        let source = SampleSource::new(source_root.clone());
        controller.sources.push(source.clone());

        let orig = source_root.join("drums").join("clip.wav");
        write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
        controller
            .load_waveform_for_selection(&source, Path::new("drums/clip.wav"))
            .unwrap();

        let entry = controller
            .export_selection_clip_to_root(
                &source.id,
                Path::new("drums/clip.wav"),
                SelectionRange::new(0.25, 0.75),
                None,
                &clip_root,
                Path::new("clip.wav"),
            )
            .unwrap();

        assert!(entry.relative_path.parent().is_none_or(|p| p.as_os_str().is_empty()));
        assert!(clip_root.join(&entry.relative_path).is_file());
        assert!(!clip_root.join("drums").join(&entry.relative_path).exists());
    }
}

fn write_selection_wav(target: &Path, samples: &[f32], spec: hound::WavSpec) -> Result<(), String> {
    if let Some(parent) = target.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create folder {}: {err}", parent.display()))?;
    }
    let mut writer = hound::WavWriter::create(target, spec)
        .map_err(|err| format!("Failed to create clip: {err}"))?;
    for sample in samples {
        writer
            .write_sample(*sample)
            .map_err(|err| format!("Failed to write clip: {err}"))?;
    }
    writer
        .finalize()
        .map_err(|err| format!("Failed to finalize clip: {err}"))
}
