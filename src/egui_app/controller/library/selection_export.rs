use super::*;
use crate::sample_sources::Rating;
use std::fs;
use std::time::SystemTime;

use crate::egui_app::controller::playback::audio_samples::{crop_samples, decode_samples_from_bytes, write_wav};
use rusqlite::params;

impl EguiController {
    pub(crate) fn export_selection_clip(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
        bounds: SelectionRange,
        target_tag: Option<Rating>,
        add_to_browser: bool,
        register_in_source: bool,
    ) -> Result<WavEntry, String> {
        let audio = self.selection_audio(source_id, relative_path)?;
        let source = self
            .library
            .sources
            .iter()
            .find(|s| &s.id == source_id)
            .cloned()
            .ok_or_else(|| "Source not available".to_string())?;
        let target_rel = self.next_selection_path_in_dir(&source.root, &audio.relative_path);
        let target_abs = source.root.join(&target_rel);
        let (samples, spec) = crop_selection_samples(&audio, bounds)?;
        write_wav(&target_abs, &samples, spec.sample_rate, spec.channels)?;
        let (looped, bpm) = self.selection_export_metadata();
        self.record_selection_entry(
            &source,
            target_rel,
            target_tag,
            add_to_browser,
            register_in_source,
            looped,
            bpm,
        )
    }

    pub(crate) fn export_selection_clip_in_folder(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
        bounds: SelectionRange,
        target_tag: Option<Rating>,
        add_to_browser: bool,
        register_in_source: bool,
        folder: &Path,
    ) -> Result<WavEntry, String> {
        let audio = self.selection_audio(source_id, relative_path)?;
        let source = self
            .library
            .sources
            .iter()
            .find(|s| &s.id == source_id)
            .cloned()
            .ok_or_else(|| "Source not available".to_string())?;
        let name_hint = folder.join(
            audio
                .relative_path
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("selection.wav")),
        );
        let target_rel = self.next_selection_path_in_dir(&source.root, &name_hint);
        let target_abs = source.root.join(&target_rel);
        let (samples, spec) = crop_selection_samples(&audio, bounds)?;
        write_wav(&target_abs, &samples, spec.sample_rate, spec.channels)?;
        let (looped, bpm) = self.selection_export_metadata();
        self.record_selection_entry(
            &source,
            target_rel,
            target_tag,
            add_to_browser,
            register_in_source,
            looped,
            bpm,
        )
    }

    pub(crate) fn save_waveform_selection_to_browser(
        &mut self,
        keep_source_focused: bool,
    ) -> Result<(), String> {
        let selection = self
            .ui
            .waveform
            .selection
            .filter(|range| range.width() >= MIN_SELECTION_WIDTH)
            .ok_or_else(|| "Create a selection first".to_string())?;
        let audio = self
            .sample_view
            .wav
            .loaded_audio
            .as_ref()
            .ok_or_else(|| "Load a sample first".to_string())?;
        let source_id = audio.source_id.clone();
        let relative_path = audio.relative_path.clone();
        let folder_override = self
            .selection_state
            .ctx
            .selected_source
            .as_ref()
            .is_some_and(|selected| selected == &source_id)
            .then(|| {
                self.ui.sources.folders.focused.and_then(|idx| {
                    self.ui
                        .sources
                        .folders
                        .rows
                        .get(idx)
                        .map(|row| row.path.clone())
                })
            })
            .flatten()
            .filter(|path| !path.as_os_str().is_empty());
        let export = if let Some(folder) = folder_override.as_deref() {
            self.export_selection_clip_in_folder(
                &source_id,
                &relative_path,
                selection,
                None,
                true,
                true,
                folder,
            )
        } else {
            self.export_selection_clip(&source_id, &relative_path, selection, None, true, true)
        };
        match export {
            Ok(entry) => {
                if !keep_source_focused {
                    self.ui.browser.autoscroll = true;
                    self.selection_state.suppress_autoplay_once = true;
                    self.select_from_browser(&entry.relative_path);
                }
                self.set_status(
                    format!("Saved clip {}", entry.relative_path.display()),
                    StatusTone::Info,
                );
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub(crate) fn export_selection_clip_to_root(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
        bounds: SelectionRange,
        target_tag: Option<Rating>,
        clip_root: &Path,
        name_hint: &Path,
    ) -> Result<WavEntry, String> {
        let audio = self.selection_audio(source_id, relative_path)?;
        let target_rel = self.next_selection_path_in_dir(clip_root, name_hint);
        let target_abs = clip_root.join(&target_rel);
        let (samples, spec) = crop_selection_samples(&audio, bounds)?;
        write_wav(&target_abs, &samples, spec.sample_rate, spec.channels)?;
        let source = SampleSource {
            id: SourceId::new(),
            root: clip_root.to_path_buf(),
        };
        // Collection-owned clips are not inserted into browser or source DB.
        let (looped, bpm) = self.selection_export_metadata();
        self.record_selection_entry(
            &source,
            target_rel,
            target_tag,
            false,
            false,
            looped,
            bpm,
        )
    }

    pub(crate) fn selection_audio(
        &self,
        source_id: &SourceId,
        relative_path: &Path,
    ) -> Result<LoadedAudio, String> {
        let Some(audio) = self.sample_view.wav.loaded_audio.as_ref() else {
            return Err("Selection audio not available; load a sample first".into());
        };
        if &audio.source_id != source_id || audio.relative_path != relative_path {
            return Err("Selection no longer matches the loaded sample".into());
        }
        Ok(audio.clone())
    }

    fn selection_export_metadata(&self) -> (bool, Option<f32>) {
        let looped = self.ui.waveform.loop_enabled;
        let bpm = self
            .ui
            .waveform
            .bpm_value
            .filter(|value| value.is_finite() && *value > 0.0);
        (looped, if looped { bpm } else { None })
    }

    fn next_selection_path_in_dir(&self, root: &Path, original: &Path) -> PathBuf {
        let parent = original.parent().unwrap_or_else(|| Path::new(""));
        let stem = original
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("selection");
        let stem = Self::strip_selection_suffix(stem);
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

    fn strip_selection_suffix(stem: &str) -> &str {
        if let Some((prefix, suffix)) = stem.rsplit_once("_sel_")
            && !prefix.is_empty()
            && !suffix.is_empty()
            && suffix.chars().all(|c| c.is_ascii_digit())
        {
            return prefix;
        }
        if let Some(prefix) = stem.strip_suffix("_sel")
            && !prefix.is_empty()
        {
            return prefix;
        }
        stem
    }

    /// Register a newly exported clip in the browser and source database.
    /// When `looped` is true, the entry is flagged as a loop and any provided BPM is persisted.
    pub(crate) fn record_selection_entry(
        &mut self,
        source: &SampleSource,
        relative_path: PathBuf,
        target_tag: Option<Rating>,
        add_to_browser: bool,
        register_in_source: bool,
        looped: bool,
        bpm: Option<f32>,
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
            content_hash: None,
            tag: target_tag.unwrap_or(Rating::NEUTRAL),
            looped,
            missing: false,
            last_played_at: None,
        };
        if register_in_source {
            let db = self
                .database_for(source)
                .map_err(|err| format!("Database unavailable: {err}"))?;
            db.upsert_file(&entry.relative_path, entry.file_size, entry.modified_ns)
                .map_err(|err| format!("Failed to register clip: {err}"))?;
            if entry.tag != Rating::NEUTRAL {
                db.set_tag(&entry.relative_path, entry.tag)
                    .map_err(|err| format!("Failed to tag clip: {err}"))?;
            }
            if looped {
                db.set_looped(&entry.relative_path, true)
                    .map_err(|err| format!("Failed to mark clip as looped: {err}"))?;
            }
            if let Some(bpm) = bpm {
                self.persist_selection_bpm(source, &entry, bpm)?;
            }
            if add_to_browser {
                if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id)
                    && let Some(selected) = self.sample_view.wav.selected_wav.clone()
                {
                    self.runtime.jobs.set_pending_select_path(Some(selected));
                }
                self.insert_new_wav_entry(source, entry.clone());
            }
            self.enqueue_similarity_for_new_sample(
                source,
                &entry.relative_path,
                entry.file_size,
                entry.modified_ns,
            );
        }
        Ok(entry)
    }

    fn insert_new_wav_entry(&mut self, source: &SampleSource, _entry: WavEntry) {
        self.invalidate_wav_entries_for_source(source);
    }

    fn persist_selection_bpm(
        &self,
        source: &SampleSource,
        entry: &WavEntry,
        bpm: f32,
    ) -> Result<(), String> {
        if !bpm.is_finite() || bpm <= 0.0 {
            return Ok(());
        }
        let size = i64::try_from(entry.file_size)
            .map_err(|_| "Clip size exceeds database limits".to_string())?;
        let content_hash = fast_content_hash(entry.file_size, entry.modified_ns);
        let conn = analysis_jobs::open_source_db(&source.root)
            .map_err(|err| format!("Failed to open analysis database: {err}"))?;
        let sample_id =
            analysis_jobs::build_sample_id(source.id.as_str(), &entry.relative_path);
        conn.execute(
            "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version, bpm)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL, ?5)
             ON CONFLICT(sample_id) DO UPDATE SET
                 content_hash = excluded.content_hash,
                 size = excluded.size,
                 mtime_ns = excluded.mtime_ns,
                 bpm = excluded.bpm",
            params![
                sample_id,
                content_hash,
                size,
                entry.modified_ns,
                bpm as f64
            ],
        )
        .map_err(|err| format!("Failed to store clip BPM: {err}"))?;
        Ok(())
    }
}

fn crop_selection_samples(
    audio: &LoadedAudio,
    bounds: SelectionRange,
) -> Result<(Vec<f32>, hound::WavSpec), String> {
    let decoded = decode_samples_from_bytes(&audio.bytes)?;
    let cropped = crop_samples(&decoded.samples, decoded.channels, bounds)?;
    let spec = hound::WavSpec {
        channels: decoded.channels.max(1),
        sample_rate: decoded.sample_rate.max(1),
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    Ok((cropped, spec))
}

fn fast_content_hash(file_size: u64, modified_ns: i64) -> String {
    format!("fast-{}-{}", file_size, modified_ns)
}

#[cfg(test)]
mod selection_export_tests;
