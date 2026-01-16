use super::helpers::TriageSampleContext;
use super::*;
use crate::egui_app::state::LoopCrossfadeSettings;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tracing::{info, warn};

pub(crate) trait BrowserActions {
    fn tag_browser_sample(&mut self, row: usize, tag: crate::sample_sources::Rating) -> Result<(), String>;
    fn tag_browser_samples(
        &mut self,
        rows: &[usize],
        tag: crate::sample_sources::Rating,
        primary_visible_row: usize,
    ) -> Result<(), String>;
    fn set_loop_marker_browser_samples(
        &mut self,
        rows: &[usize],
        looped: bool,
        primary_visible_row: usize,
    ) -> Result<(), String>;
    fn set_bpm_browser_samples(
        &mut self,
        rows: &[usize],
        bpm: f32,
        primary_visible_row: usize,
    ) -> Result<(), String>;
    fn normalize_browser_sample(&mut self, row: usize) -> Result<(), String>;
    fn normalize_browser_samples(&mut self, rows: &[usize]) -> Result<(), String>;
    fn loop_crossfade_browser_samples(
        &mut self,
        rows: &[usize],
        settings: LoopCrossfadeSettings,
        primary_visible_row: usize,
    ) -> Result<(), String>;
    fn rename_browser_sample(&mut self, row: usize, new_name: &str) -> Result<(), String>;
    fn delete_browser_sample(&mut self, row: usize) -> Result<(), String>;
    fn delete_browser_samples(&mut self, rows: &[usize]) -> Result<(), String>;
    fn remove_dead_link_browser_samples(&mut self, rows: &[usize]) -> Result<(), String>;
}

impl BrowserActions for BrowserController<'_> {
    fn tag_browser_sample(&mut self, row: usize, tag: crate::sample_sources::Rating) -> Result<(), String> {
        info!(row, ?tag, "triage tag: single row");
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
            warn!(row, ?tag, error = %err, "triage tag failed");
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    fn tag_browser_samples(
        &mut self,
        rows: &[usize],
        tag: crate::sample_sources::Rating,
        primary_visible_row: usize,
    ) -> Result<(), String> {
        info!(?rows, ?tag, primary_visible_row, "triage tag: multi row");
        let (contexts, mut last_error) = self.resolve_unique_browser_contexts(rows);
        info!(count = contexts.len(), "triage tag: resolved contexts");
        for ctx in contexts {
            if let Err(err) =
                self.set_sample_tag_for_source(&ctx.source, &ctx.entry.relative_path, tag, true)
            {
                last_error = Some(err);
            } else {
                self.set_status(
                    format!("Tagged {} as {:?}", ctx.entry.relative_path.display(), tag),
                    StatusTone::Info,
                );
            }
        }
        self.refocus_after_filtered_removal(primary_visible_row);
        if let Some(err) = last_error {
            warn!(?rows, ?tag, error = %err, "triage tag failed for multi row");
            Err(err)
        } else {
            Ok(())
        }
    }

    fn set_loop_marker_browser_samples(
        &mut self,
        rows: &[usize],
        looped: bool,
        primary_visible_row: usize,
    ) -> Result<(), String> {
        let (contexts, mut last_error) = self.resolve_unique_browser_contexts(rows);
        let action_label = if looped { "Marked loop" } else { "Cleared loop" };
        for ctx in contexts {
            if let Err(err) = self.set_sample_looped_for_source(
                &ctx.source,
                &ctx.entry.relative_path,
                looped,
                true,
            ) {
                last_error = Some(err);
            } else {
                self.set_status(
                    format!("{action_label} {}", ctx.entry.relative_path.display()),
                    StatusTone::Info,
                );
            }
        }
        self.refocus_after_filtered_removal(primary_visible_row);
        if let Some(err) = last_error {
            warn!(?rows, looped, error = %err, "loop marker failed for multi row");
            Err(err)
        } else {
            Ok(())
        }
    }

    fn set_bpm_browser_samples(
        &mut self,
        rows: &[usize],
        bpm: f32,
        primary_visible_row: usize,
    ) -> Result<(), String> {
        if !bpm.is_finite() || bpm <= 0.0 {
            return Err("BPM must be a positive number".to_string());
        }
        let (contexts, mut last_error) = self.resolve_unique_browser_contexts(rows);
        let mut grouped: HashMap<SourceId, (SampleSource, Vec<PathBuf>)> = HashMap::new();
        for ctx in contexts {
            grouped
                .entry(ctx.source.id.clone())
                .or_insert_with(|| (ctx.source.clone(), Vec::new()))
                .1
                .push(ctx.entry.relative_path.clone());
        }
        let mut updated = 0usize;
        for (source_id, (source, paths)) in grouped {
            let mut conn = match analysis_jobs::open_source_db(&source.root) {
                Ok(conn) => conn,
                Err(err) => {
                    last_error = Some(format!("Failed to open source DB for BPM save: {err}"));
                    continue;
                }
            };
            let sample_ids: Vec<String> = paths
                .iter()
                .map(|path| analysis_jobs::build_sample_id(source_id.as_str(), path))
                .collect();
            match analysis_jobs::update_sample_bpms(&mut conn, &sample_ids, Some(bpm)) {
                Ok(count) => updated = updated.saturating_add(count),
                Err(err) => last_error = Some(err),
            }
            if let Some(cache) = self.ui_cache.browser.bpm_values.get_mut(&source_id) {
                for path in &paths {
                    cache.insert(path.clone(), Some(bpm));
                }
            }
            let loaded_matches = self
                .sample_view
                .wav
                .loaded_audio
                .as_ref()
                .is_some_and(|audio| {
                    audio.source_id == source_id
                        && paths
                            .iter()
                            .any(|path| audio.relative_path == *path)
                });
            if loaded_matches {
                self.set_waveform_bpm_input(Some(bpm));
            }
        }
        if updated > 0 {
            let label = format_bpm_label(bpm);
            let sample_label = if updated == 1 { "sample" } else { "samples" };
            self.set_status(
                format!("Set BPM {label} for {updated} {sample_label}"),
                StatusTone::Info,
            );
        }
        self.refocus_after_filtered_removal(primary_visible_row);
        if let Some(err) = last_error {
            warn!(?rows, bpm, error = %err, "bpm update failed for browser samples");
            Err(err)
        } else {
            Ok(())
        }
    }

    fn normalize_browser_sample(&mut self, row: usize) -> Result<(), String> {
        let result = self.try_normalize_browser_sample(row);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    fn normalize_browser_samples(&mut self, rows: &[usize]) -> Result<(), String> {
        let (contexts, mut last_error) = self.resolve_unique_browser_contexts(rows);
        for ctx in contexts {
            if let Err(err) = self.try_normalize_browser_sample_ctx(&ctx) {
                last_error = Some(err);
            }
        }
        if let Some(err) = last_error {
            Err(err)
        } else {
            Ok(())
        }
    }

    fn loop_crossfade_browser_samples(
        &mut self,
        rows: &[usize],
        settings: LoopCrossfadeSettings,
        primary_visible_row: usize,
    ) -> Result<(), String> {
        let (contexts, mut last_error) = self.resolve_unique_browser_contexts(rows);
        let primary_path = self
            .resolve_browser_sample(primary_visible_row)
            .ok()
            .map(|ctx| ctx.entry.relative_path);
        
        let was_playing = self.is_playing();
        let was_looping = self.ui.waveform.loop_enabled;
        let playhead_position = self.ui.waveform.playhead.position;
        let primary_is_loaded = self.sample_view.wav.loaded_audio.as_ref().is_some_and(|audio| {
            primary_path.as_ref().is_some_and(|path| audio.relative_path == *path)
        });

        let mut primary_new = None;
        let mut primary_source = None;
        for ctx in contexts {
            match self.apply_loop_crossfade_for_sample(
                &ctx.source,
                &ctx.entry.relative_path,
                &ctx.absolute_path,
                &settings,
            ) {
                Ok(new_relative) => {
                    if primary_path
                        .as_ref()
                        .is_some_and(|path| path == &ctx.entry.relative_path)
                    {
                        primary_new = Some(new_relative);
                        primary_source = Some(ctx.source.clone());
                    }
                }
                Err(err) => last_error = Some(err),
            }
        }
        if let Some(path) = primary_new {
            if primary_is_loaded && was_playing {
                if let Some(source) = primary_source {
                    let start_override = if playhead_position.is_finite() {
                        Some(playhead_position.clamp(0.0, 1.0))
                    } else {
                        None
                    };
                    self.runtime.jobs.set_pending_playback(Some(PendingPlayback {
                        source_id: source.id,
                        relative_path: path.clone(),
                        looped: was_looping,
                        start_override,
                    }));
                    self.selection_state.suppress_autoplay_once = true;
                }
            }
            self.select_from_browser(&path);
        }
        if let Some(err) = last_error {
            Err(err)
        } else {
            Ok(())
        }
    }

    fn rename_browser_sample(&mut self, row: usize, new_name: &str) -> Result<(), String> {
        let result = self.try_rename_browser_sample(row, new_name);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    fn delete_browser_sample(&mut self, row: usize) -> Result<(), String> {
        self.delete_browser_samples(&[row])
    }

    fn delete_browser_samples(&mut self, rows: &[usize]) -> Result<(), String> {
        let next_focus = self.next_browser_focus_after_delete(rows);
        let (contexts, mut last_error) = self.resolve_unique_browser_contexts(rows);
        for ctx in contexts {
            if let Err(err) = self.try_delete_browser_sample_ctx(&ctx) {
                last_error = Some(err);
            }
        }
        if let Some(path) = next_focus
            && self.wav_index_for_path(&path).is_some()
        {
            if let Some(row) = self.visible_row_for_path(&path) {
                self.focus_browser_row_only(row);
            } else {
                self.select_wav_by_path_with_rebuild(&path, true);
            }
        }
        if let Some(err) = last_error {
            Err(err)
        } else {
            Ok(())
        }
    }

    fn remove_dead_link_browser_samples(&mut self, rows: &[usize]) -> Result<(), String> {
        let next_focus = self.next_browser_focus_after_delete(rows);
        let (contexts, mut last_error) = self.resolve_unique_browser_contexts(rows);
        let mut removed = 0;
        for ctx in contexts {
            let is_dead_link = ctx.entry.missing || !ctx.absolute_path.exists();
            if !is_dead_link {
                continue;
            }
            if let Err(err) = self.try_remove_dead_link_browser_sample_ctx(&ctx) {
                last_error = Some(err);
            } else {
                removed += 1;
            }
        }
        if let Some(path) = next_focus
            && self.wav_index_for_path(&path).is_some()
        {
            if let Some(row) = self.visible_row_for_path(&path) {
                self.focus_browser_row_only(row);
            } else {
                self.select_wav_by_path_with_rebuild(&path, true);
            }
        }
        if let Some(err) = last_error {
            self.set_status(err.clone(), StatusTone::Error);
            return Err(err);
        }
        if removed == 0 {
            self.set_status("No dead links removed", StatusTone::Info);
        }
        Ok(())
    }
}

impl BrowserController<'_> {
    fn resolve_unique_browser_contexts(
        &mut self,
        rows: &[usize],
    ) -> (Vec<TriageSampleContext>, Option<String>) {
        let mut contexts = Vec::with_capacity(rows.len());
        let mut seen = HashSet::new();
        let mut last_error = None;
        for &row in rows {
            match self.resolve_browser_sample(row) {
                Ok(ctx) => {
                    if seen.insert(ctx.entry.relative_path.clone()) {
                        contexts.push(ctx);
                    }
                }
                Err(err) => last_error = Some(err),
            }
        }
        (contexts, last_error)
    }
}

fn format_bpm_label(bpm: f32) -> String {
    let rounded = bpm.round();
    if (bpm - rounded).abs() < 0.01 {
        format!("{rounded:.0}")
    } else {
        format!("{bpm:.2}")
    }
}
