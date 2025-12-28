use super::helpers::TriageSampleContext;
use super::*;
use crate::egui_app::state::LoopCrossfadeSettings;
use tracing::{info, warn};
use std::collections::HashSet;

pub(crate) trait BrowserActions {
    fn tag_browser_sample(&mut self, row: usize, tag: SampleTag) -> Result<(), String>;
    fn tag_browser_samples(
        &mut self,
        rows: &[usize],
        tag: SampleTag,
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
    fn tag_browser_sample(&mut self, row: usize, tag: SampleTag) -> Result<(), String> {
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
        tag: SampleTag,
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
        let mut primary_new = None;
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
                    }
                }
                Err(err) => last_error = Some(err),
            }
        }
        if let Some(path) = primary_new {
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
        for ctx in contexts {
            let is_dead_link = ctx.entry.missing || !ctx.absolute_path.exists();
            if !is_dead_link {
                continue;
            }
            if let Err(err) = self.try_remove_dead_link_browser_sample_ctx(&ctx) {
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
