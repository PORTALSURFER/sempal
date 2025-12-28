use super::*;
impl EguiController {
    pub(super) fn sync_after_wav_entries_changed(&mut self) {
        self.rebuild_wav_lookup();
        self.ui_cache.browser.search.invalidate();
        self.refresh_folder_browser();
        self.rebuild_browser_lists();
    }

    pub(super) fn queue_wav_load(&mut self) {
        let Some(source) = self.current_source() else {
            return;
        };
        if !source.root.is_dir() {
            self.mark_source_missing(&source.id, "Source folder missing");
            return;
        }
        self.clear_source_missing(&source.id);
        if let Some(cache) = self.cache.wav.entries.get(&source.id) {
            if let Some(entries) = cache.pages.get(&0).cloned() {
                self.apply_wav_entries(
                    entries,
                    cache.total,
                    cache.page_size,
                    0,
                    true,
                    Some(source.id.clone()),
                    None,
                );
                return;
            }
        }
        self.wav_entries.clear();
        self.sync_after_wav_entries_changed();
        if self.runtime.jobs.wav_load_pending_for(&source.id) {
            return;
        }
        self.runtime.jobs.mark_wav_load_pending(source.id.clone());
        let job = WavLoadJob {
            source_id: source.id.clone(),
            root: source.root.clone(),
            page_size: self.wav_entries.page_size,
        };
        if cfg!(test) {
            let (result, total) = wav_entries_loader::load_entries(&job);
            match result {
                Ok(entries) => {
                    self.cache.wav.insert_page(
                        source.id.clone(),
                        total,
                        job.page_size,
                        0,
                        entries.clone(),
                    );
                    self.apply_wav_entries(
                        entries,
                        total,
                        job.page_size,
                        0,
                        false,
                        Some(source.id.clone()),
                        None,
                    );
                }
                Err(err) => self.handle_wav_load_error(&source.id, err),
            }
            self.runtime.jobs.clear_wav_load_pending();
            return;
        }
        self.runtime.jobs.send_wav_job(job);
        self.ensure_wav_load_progress(&source);
    }

    pub(super) fn handle_wav_load_error(&mut self, source_id: &SourceId, err: LoadEntriesError) {
        match err {
            LoadEntriesError::Db(SourceDbError::InvalidRoot(_)) => {
                self.mark_source_missing(source_id, "Source folder missing");
            }
            LoadEntriesError::Db(db_err) => {
                self.set_status(format!("Failed to load wavs: {db_err}"), StatusTone::Error);
            }
            LoadEntriesError::Message(msg) => {
                if msg.contains("not a directory") {
                    self.mark_source_missing(source_id, "Source folder missing");
                } else {
                    self.set_status(format!("Failed to load wavs: {msg}"), StatusTone::Error);
                }
            }
        }
    }

    pub(super) fn apply_wav_entries(
        &mut self,
        entries: Vec<WavEntry>,
        total: usize,
        page_size: usize,
        page_index: usize,
        from_cache: bool,
        source_id: Option<SourceId>,
        elapsed: Option<Duration>,
    ) {
        self.wav_entries.total = total;
        self.wav_entries.page_size = page_size.max(1);
        if page_index == 0 {
            self.wav_entries.pages.clear();
            self.wav_entries.lookup.clear();
        }
        self.wav_entries.insert_page(page_index, entries);
        self.sync_after_wav_entries_changed();
        let mut pending_applied = false;
        if let Some(path) = self.runtime.jobs.take_pending_select_path() {
            if self.sample_view.wav.selected_wav.as_ref() == Some(&path) {
                // Preserve current selection without reloading audio.
                pending_applied = true;
            } else if self.wav_index_for_path(&path).is_some() {
                self.select_wav_by_path(&path);
                pending_applied = true;
            }
        }
        if !pending_applied
            && self.sample_view.wav.selected_wav.is_none()
            && self.ui.collections.selected_sample.is_none()
            && self.wav_entries.total > 0
        {
            self.selection_state.suppress_autoplay_once = true;
            self.select_wav_by_index(0);
        }
        if let Some(ref id) = source_id {
            if !from_cache {
                self.ui_cache.browser.labels.remove(id);
            }
            let needs_failures =
                !from_cache || !self.ui_cache.browser.analysis_failures.contains_key(id);
            if needs_failures {
                if let Some(source) = self.library.sources.iter().find(|s| &s.id == id)
                    && let Ok(failures) = super::analysis_jobs::failed_samples_for_source(source)
                {
                    self.ui_cache
                        .browser
                        .analysis_failures
                        .insert(id.clone(), failures);
                } else {
                    self.ui_cache.browser.analysis_failures.remove(id);
                }
            }
            self.sync_missing_from_db(id);
        }
        let prefix = if from_cache { "Cached" } else { "Loaded" };
        let suffix = elapsed
            .map(|d| format!(" in {} ms", d.as_millis()))
            .unwrap_or_default();
        self.set_status(
            format!(
                "{prefix} {} wav files{suffix}",
                self.wav_entries.total
            ),
            StatusTone::Info,
        );
        if let Some(source_id) = source_id.as_ref() {
            self.maybe_refresh_source_db_in_background(source_id, from_cache);
        }
    }

    fn maybe_refresh_source_db_in_background(&self, source_id: &SourceId, from_cache: bool) {
        if !from_cache || self.runtime.jobs.scan_in_progress() {
            return;
        }
        let Some(source) = self.library.sources.iter().find(|s| &s.id == source_id) else {
            return;
        };
        if !source.root.is_dir() {
            return;
        }
        let _ = crate::sample_sources::scanner::scan_in_background(source.root.clone());
    }

    pub(super) fn invalidate_wav_entries_for_source(&mut self, source: &SampleSource) {
        self.cache.wav.entries.remove(&source.id);
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
            self.wav_entries.clear();
            self.sync_after_wav_entries_changed();
            self.queue_wav_load();
        } else {
            self.ui_cache.browser.labels.remove(&source.id);
        }
        self.rebuild_missing_lookup_for_source(&source.id);
    }

    pub(super) fn invalidate_wav_entries_for_source_preserve_folders(
        &mut self,
        source: &SampleSource,
    ) {
        self.cache.wav.entries.remove(&source.id);
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
            self.wav_entries.clear();
            self.rebuild_wav_lookup();
            self.ui_cache.browser.search.invalidate();
            self.rebuild_browser_lists();
            self.queue_wav_load();
        } else {
            self.ui_cache.browser.labels.remove(&source.id);
        }
        self.rebuild_missing_lookup_for_source(&source.id);
    }
}
