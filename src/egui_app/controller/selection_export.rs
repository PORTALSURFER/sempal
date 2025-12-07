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
    ) -> Result<WavEntry, String> {
        let audio = self.selection_audio(source_id, relative_path)?;
        let source = self
            .sources
            .iter()
            .find(|s| &s.id == source_id)
            .cloned()
            .ok_or_else(|| "Source not available".to_string())?;
        let target_rel = self.next_selection_path(&source, &audio.relative_path);
        let target_abs = source.root.join(&target_rel);
        let (samples, spec) = crop_selection_samples(&audio, bounds)?;
        write_selection_wav(&target_abs, &samples, spec)?;
        self.record_selection_entry(&source, target_rel, target_tag)
    }

    fn selection_audio(
        &self,
        source_id: &SourceId,
        relative_path: &Path,
    ) -> Result<LoadedAudio, String> {
        let Some(audio) = self.loaded_audio.as_ref() else {
            return Err("Selection audio not available; load a sample first".into());
        };
        if &audio.source_id != source_id || audio.relative_path != relative_path {
            return Err("Selection no longer matches the loaded sample".into());
        }
        Ok(audio.clone())
    }

    fn next_selection_path(&self, source: &SampleSource, original: &Path) -> PathBuf {
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
            let absolute = source.root.join(&candidate);
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
        };
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.upsert_file(&entry.relative_path, entry.file_size, entry.modified_ns)
            .map_err(|err| format!("Failed to register clip: {err}"))?;
        if entry.tag != SampleTag::Neutral {
            db.set_tag(&entry.relative_path, entry.tag)
                .map_err(|err| format!("Failed to tag clip: {err}"))?;
        }
        self.insert_new_wav_entry(source, entry.clone());
        Ok(entry)
    }

    fn insert_new_wav_entry(&mut self, source: &SampleSource, entry: WavEntry) {
        let cache = self
            .wav_cache
            .entry(source.id.clone())
            .or_insert_with(Vec::new);
        cache.push(entry.clone());
        cache.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

        if self.selected_source.as_ref() != Some(&source.id) {
            return;
        }
        self.wav_entries.push(entry);
        self.wav_entries
            .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        self.rebuild_wav_lookup();
        self.rebuild_triage_lists();
        self.label_cache
            .insert(source.id.clone(), self.build_label_cache(&self.wav_entries));
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

fn write_selection_wav(target: &Path, samples: &[f32], spec: hound::WavSpec) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create folder {}: {err}", parent.display()))?;
        }
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
