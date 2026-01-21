use super::*;
use crate::egui_app::controller::jobs::NormalizationJob;

pub(crate) struct BrowserController<'a> {
    controller: &'a mut EguiController,
}

impl<'a> BrowserController<'a> {
    pub(crate) fn new(controller: &'a mut EguiController) -> Self {
        Self { controller }
    }
}

impl std::ops::Deref for BrowserController<'_> {
    type Target = EguiController;

    fn deref(&self) -> &Self::Target {
        self.controller
    }
}

impl std::ops::DerefMut for BrowserController<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.controller
    }
}

pub(crate) struct TriageSampleContext {
    pub(crate) source: SampleSource,
    pub(crate) entry: WavEntry,
    pub(crate) absolute_path: PathBuf,
}

impl BrowserController<'_> {
    pub(crate) fn try_normalize_browser_sample(&mut self, row: usize) -> Result<(), String> {
        let ctx = self.resolve_browser_sample(row)?;
        self.try_normalize_browser_sample_ctx(&ctx)
    }

    pub(crate) fn try_normalize_browser_sample_ctx(
        &mut self,
        ctx: &TriageSampleContext,
    ) -> Result<(), String> {
        if cfg!(test) {
            return self.normalize_browser_sample_sync(ctx);
        }
        let job = NormalizationJob {
            source: ctx.source.clone(),
            relative_path: ctx.entry.relative_path.clone(),
            absolute_path: ctx.absolute_path.clone(),
        };

        if self.controller.ui.progress.task != Some(ProgressTaskKind::Normalization) {
            self.controller.show_status_progress(
                ProgressTaskKind::Normalization,
                format!("Normalizing {}", ctx.entry.relative_path.display()),
                1,
                false,
            );
        }

        self.controller.runtime.jobs.begin_normalization(job);
        Ok(())
    }

    fn normalize_browser_sample_sync(&mut self, ctx: &TriageSampleContext) -> Result<(), String> {
        let was_playing = self.is_playing();
        let was_looping = self.ui.waveform.loop_enabled;
        let playhead_position = self.ui.waveform.playhead.position;

        let (file_size, modified_ns, tag) = self.normalize_and_save_for_path(
            &ctx.source,
            &ctx.entry.relative_path,
            &ctx.absolute_path,
        )?;
        let entry_index = self.wav_index_for_path(&ctx.entry.relative_path);
        let looped = entry_index
            .and_then(|idx| self.wav_entries.entry(idx))
            .map(|entry| entry.looped)
            .unwrap_or(false);
        let last_played_at = entry_index
            .and_then(|idx| self.wav_entries.entry(idx))
            .and_then(|entry| entry.last_played_at);
        let updated = WavEntry {
            relative_path: ctx.entry.relative_path.clone(),
            file_size,
            modified_ns,
            content_hash: None,
            tag,
            looped,
            missing: false,
            last_played_at,
        };

        let is_currently_loaded = self.sample_view.wav.loaded_audio.as_ref().is_some_and(|audio| {
            audio.source_id == ctx.source.id && audio.relative_path == ctx.entry.relative_path
        });
        if is_currently_loaded && was_playing {
            let start_override = if playhead_position.is_finite() {
                Some(playhead_position.clamp(0.0, 1.0))
            } else {
                None
            };
            self.runtime.jobs.set_pending_playback(Some(PendingPlayback {
                source_id: ctx.source.id.clone(),
                relative_path: ctx.entry.relative_path.clone(),
                looped: was_looping,
                start_override,
            }));
        }

        self.update_cached_entry(&ctx.source, &ctx.entry.relative_path, updated);
        if self.selection_state.ctx.selected_source.as_ref() == Some(&ctx.source.id) {
            self.rebuild_browser_lists();
        }
        self.refresh_waveform_for_sample(&ctx.source, &ctx.entry.relative_path);
        self.set_status(
            format!("Normalized {}", ctx.entry.relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }
    pub(crate) fn next_browser_focus_after_delete(&mut self, rows: &[usize]) -> Option<PathBuf> {
        if rows.is_empty() || self.ui.browser.visible.len() == 0 {
            return None;
        }
        let mut sorted = rows.to_vec();
        sorted.sort_unstable();
        let highest = sorted.last().copied()?;
        let first = sorted.first().copied().unwrap_or(highest);
        let after = highest
            .checked_add(1)
            .and_then(|idx| self.ui.browser.visible.get(idx))
            .and_then(|entry_idx| self.wav_entry(entry_idx))
            .map(|entry| entry.relative_path.clone());
        if after.is_some() {
            return after;
        }
        first
            .checked_sub(1)
            .and_then(|idx| self.ui.browser.visible.get(idx))
            .and_then(|entry_idx| self.wav_entry(entry_idx))
            .map(|entry| entry.relative_path.clone())
    }

    pub(crate) fn try_delete_browser_sample_ctx(
        &mut self,
        ctx: &TriageSampleContext,
    ) -> Result<(), String> {
        std::fs::remove_file(&ctx.absolute_path)
            .map_err(|err| format!("Failed to delete file: {err}"))?;
        let db = self
            .database_for(&ctx.source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.remove_file(&ctx.entry.relative_path)
            .map_err(|err| format!("Failed to drop database row: {err}"))?;
        self.prune_cached_sample(&ctx.source, &ctx.entry.relative_path);
        self.set_status(
            format!("Deleted {}", ctx.entry.relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }

    pub(crate) fn try_remove_dead_link_browser_sample_ctx(
        &mut self,
        ctx: &TriageSampleContext,
    ) -> Result<(), String> {
        let db = self
            .database_for(&ctx.source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.remove_file(&ctx.entry.relative_path)
            .map_err(|err| format!("Failed to drop database row: {err}"))?;
        self.prune_cached_sample(&ctx.source, &ctx.entry.relative_path);
        self.set_status(
            format!("Removed dead link {}", ctx.entry.relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }

    pub(crate) fn try_rename_browser_sample(
        &mut self,
        row: usize,
        new_name: &str,
    ) -> Result<(), String> {
        let ctx = self.resolve_browser_sample(row)?;
        let tag = self.sample_tag_for(&ctx.source, &ctx.entry.relative_path)?;
        let full_name = self.name_with_preserved_extension(&ctx.entry.relative_path, new_name)?;
        let new_relative = self.validate_new_sample_name_in_parent(
            &ctx.entry.relative_path,
            &ctx.source.root,
            &full_name,
        )?;
        self.commit_browser_rename(&ctx, &new_relative, tag)?;
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
        tag: crate::sample_sources::Rating,
    ) -> Result<(), String> {
        let (file_size, modified_ns) = self.apply_triage_rename(ctx, new_relative, tag)?;
        let updated_path = new_relative.to_path_buf();
        self.update_cached_entry(
            &ctx.source,
            &ctx.entry.relative_path,
            WavEntry {
                relative_path: updated_path.clone(),
                file_size,
                modified_ns,
                content_hash: None,
                tag,
                looped: ctx.entry.looped,
                missing: false,
                last_played_at: ctx.entry.last_played_at,
            },
        );

        let was_playing = self.is_playing();
        let was_looping = self.ui.waveform.loop_enabled;
        let playhead_position = self.ui.waveform.playhead.position;
        let is_currently_loaded = self.sample_view.wav.loaded_audio.as_ref().is_some_and(|audio| {
            audio.source_id == ctx.source.id && audio.relative_path == ctx.entry.relative_path
        });

        if is_currently_loaded && was_playing {
            let start_override = if playhead_position.is_finite() {
                Some(playhead_position.clamp(0.0, 1.0))
            } else {
                None
            };
            self.runtime.jobs.set_pending_playback(Some(PendingPlayback {
                source_id: ctx.source.id.clone(),
                relative_path: updated_path.clone(),
                looped: was_looping,
                start_override,
            }));
        }

        self.refresh_waveform_for_sample(&ctx.source, new_relative);
        Ok(())
    }

    fn apply_triage_rename(
        &mut self,
        ctx: &TriageSampleContext,
        new_relative: &Path,
        tag: crate::sample_sources::Rating,
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
}
