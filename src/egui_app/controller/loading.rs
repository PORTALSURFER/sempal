use super::*;
use crate::egui_app::state::ProgressTaskKind;

impl EguiController {
    fn sync_after_wav_entries_changed(&mut self) {
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
        if let Some(entries) = self.cache.wav.entries.get(&source.id).cloned() {
            self.ensure_wav_cache_lookup(&source.id);
            self.apply_wav_entries(entries, true, Some(source.id.clone()), None);
            return;
        }
        self.wav_entries.entries.clear();
        self.sync_after_wav_entries_changed();
        if self.runtime.jobs.wav_load_pending_for(&source.id) {
            return;
        }
        self.runtime.jobs.mark_wav_load_pending(source.id.clone());
        let job = WavLoadJob {
            source_id: source.id.clone(),
            root: source.root.clone(),
        };
        if cfg!(test) {
            let result = wav_entries_loader::load_entries(&job);
            match result {
                Ok(entries) => {
                    self.cache
                        .wav
                        .entries
                        .insert(source.id.clone(), entries.clone());
                    self.rebuild_wav_cache_lookup(&source.id);
                    self.apply_wav_entries(entries, false, Some(source.id.clone()), None);
                }
                Err(err) => self.handle_wav_load_error(&source.id, err),
            }
            self.runtime.jobs.clear_wav_load_pending();
            return;
        }
        self.runtime.jobs.send_wav_job(job);
        if !self.ui.progress.visible || self.ui.progress.task == Some(ProgressTaskKind::WavLoad) {
            self.show_status_progress(ProgressTaskKind::WavLoad, "Loading samples", 0, false);
            self.update_progress_detail(format!("Loading wavs for {}", source.root.display()));
        }
        self.set_status(
            format!("Loading wavs for {}", source.root.display()),
            StatusTone::Info,
        );
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
        from_cache: bool,
        source_id: Option<SourceId>,
        elapsed: Option<Duration>,
    ) {
        self.wav_entries.entries = entries;
        self.sync_after_wav_entries_changed();
        let mut pending_applied = false;
        if let Some(path) = self.runtime.jobs.take_pending_select_path()
            && self.wav_entries.lookup.contains_key(&path)
        {
            self.select_wav_by_path(&path);
            pending_applied = true;
        }
        if !pending_applied
            && self.sample_view.wav.selected_wav.is_none()
            && self.ui.collections.selected_sample.is_none()
            && !self.wav_entries.entries.is_empty()
        {
            self.selection_state.suppress_autoplay_once = true;
            self.select_wav_by_index(0);
        }
        if let Some(id) = source_id {
            let needs_labels = !from_cache
                || self
                    .ui_cache
                    .browser
                    .labels
                    .get(&id)
                    .map(|cached| cached.len() != self.wav_entries.entries.len())
                    .unwrap_or(true);
            if needs_labels {
                self.ui_cache.browser.labels.insert(
                    id.clone(),
                    self.build_label_cache(&self.wav_entries.entries),
                );
            }
            let needs_failures = !from_cache
                || !self
                    .ui_cache
                    .browser
                    .analysis_failures
                    .contains_key(&id);
            if needs_failures {
                if let Ok(failures) = super::analysis_jobs::failed_samples_for_source(&id) {
                    self.ui_cache.browser.analysis_failures.insert(id.clone(), failures);
                } else {
                    self.ui_cache.browser.analysis_failures.remove(&id);
                }
            }
            let missing: std::collections::HashSet<std::path::PathBuf> = self
                .wav_entries
                .entries
                .iter()
                .filter(|entry| entry.missing)
                .map(|entry| entry.relative_path.clone())
                .collect();
            self.library.missing.wavs.insert(id, missing);
        }
        let prefix = if from_cache { "Cached" } else { "Loaded" };
        let suffix = elapsed
            .map(|d| format!(" in {} ms", d.as_millis()))
            .unwrap_or_default();
        self.set_status(
            format!(
                "{prefix} {} wav files{suffix}",
                self.wav_entries.entries.len()
            ),
            StatusTone::Info,
        );
    }
}
