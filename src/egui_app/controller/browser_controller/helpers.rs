use super::*;

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

pub(in crate::egui_app::controller) struct TriageSampleContext {
    pub(in crate::egui_app::controller) source: SampleSource,
    pub(in crate::egui_app::controller) entry: WavEntry,
    pub(in crate::egui_app::controller) absolute_path: PathBuf,
}

impl BrowserController<'_> {
    pub(super) fn try_normalize_browser_sample(&mut self, row: usize) -> Result<(), String> {
        let ctx = self.resolve_browser_sample(row)?;
        self.try_normalize_browser_sample_ctx(&ctx)
    }

    pub(super) fn try_normalize_browser_sample_ctx(
        &mut self,
        ctx: &TriageSampleContext,
    ) -> Result<(), String> {
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
            content_hash: None,
            tag,
            missing: false,
        };
        self.update_cached_entry(&ctx.source, &ctx.entry.relative_path, updated);
        if self.selection_state.ctx.selected_source.as_ref() == Some(&ctx.source.id) {
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

    pub(super) fn next_browser_focus_after_delete(&mut self, rows: &[usize]) -> Option<PathBuf> {
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

    pub(super) fn try_delete_browser_sample_ctx(
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

    pub(super) fn try_remove_dead_link_browser_sample_ctx(
        &mut self,
        ctx: &TriageSampleContext,
    ) -> Result<(), String> {
        let db = self
            .database_for(&ctx.source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.remove_file(&ctx.entry.relative_path)
            .map_err(|err| format!("Failed to drop database row: {err}"))?;
        self.prune_cached_sample(&ctx.source, &ctx.entry.relative_path);
        let collections_changed =
            self.remove_sample_from_collections(&ctx.source.id, &ctx.entry.relative_path);
        if collections_changed {
            self.persist_config("Failed to save collection after removing dead link")?;
        }
        self.set_status(
            format!("Removed dead link {}", ctx.entry.relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }

    pub(super) fn try_rename_browser_sample(
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
                missing: false,
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
}
