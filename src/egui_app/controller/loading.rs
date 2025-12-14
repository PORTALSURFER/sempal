use super::*;

impl EguiController {
    fn sync_after_wav_entries_changed(&mut self) {
        self.rebuild_wav_lookup();
        self.browser_search_cache.invalidate();
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
        if let Some(entries) = self.wav_cache.entries.get(&source.id).cloned() {
            self.ensure_wav_cache_lookup(&source.id);
            self.apply_wav_entries(entries, true, Some(source.id.clone()), None);
            return;
        }
        self.wav_entries.clear();
        self.sync_after_wav_entries_changed();
        if self.jobs.pending_source.as_ref() == Some(&source.id) {
            return;
        }
        self.jobs.pending_source = Some(source.id.clone());
        let job = WavLoadJob {
            source_id: source.id.clone(),
            root: source.root.clone(),
        };
        if cfg!(test) {
            let result = load_entries(&job);
            match result {
                Ok(entries) => {
                    self.wav_cache.entries.insert(source.id.clone(), entries.clone());
                    self.rebuild_wav_cache_lookup(&source.id);
                    self.apply_wav_entries(entries, false, Some(source.id.clone()), None);
                }
                Err(err) => self.handle_wav_load_error(&source.id, err),
            }
            self.jobs.pending_source = None;
            return;
        }
        let _ = self.jobs.wav_job_tx.send(job);
        self.set_status(
            format!("Loading wavs for {}", source.root.display()),
            StatusTone::Info,
        );
    }

    pub(super) fn poll_wav_loader(&mut self) {
        while let Ok(message) = self.jobs.wav_job_rx.try_recv() {
            if Some(&message.source_id) != self.selected_source.as_ref() {
                continue;
            }
            match message.result {
                Ok(entries) => {
                    self.wav_cache
                        .entries
                        .insert(message.source_id.clone(), entries.clone());
                    self.rebuild_wav_cache_lookup(&message.source_id);
                    self.apply_wav_entries(
                        entries,
                        false,
                        Some(message.source_id.clone()),
                        Some(message.elapsed),
                    );
                }
                Err(err) => self.handle_wav_load_error(&message.source_id, err),
            }
            self.jobs.pending_source = None;
        }
    }

    fn handle_wav_load_error(&mut self, source_id: &SourceId, err: LoadEntriesError) {
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

    fn apply_wav_entries(
        &mut self,
        entries: Vec<WavEntry>,
        from_cache: bool,
        source_id: Option<SourceId>,
        elapsed: Option<Duration>,
    ) {
        self.wav_entries = entries;
        self.sync_after_wav_entries_changed();
        let mut pending_applied = false;
        if let Some(path) = self.jobs.pending_select_path.take()
            && self.wav_lookup.contains_key(&path)
        {
            self.select_wav_by_path(&path);
            pending_applied = true;
        }
        if !pending_applied
            && self.selected_wav.is_none()
            && self.ui.collections.selected_sample.is_none()
            && !self.wav_entries.is_empty()
        {
            self.suppress_autoplay_once = true;
            self.select_wav_by_index(0);
        }
        if let Some(id) = source_id {
            let needs_labels = !from_cache
                || self
                    .label_cache
                    .get(&id)
                    .map(|cached| cached.len() != self.wav_entries.len())
                    .unwrap_or(true);
            if needs_labels {
                self.label_cache
                    .insert(id.clone(), self.build_label_cache(&self.wav_entries));
            }
            let missing: std::collections::HashSet<std::path::PathBuf> = self
                .wav_entries
                .iter()
                .filter(|entry| entry.missing)
                .map(|entry| entry.relative_path.clone())
                .collect();
            self.missing.wavs.insert(id, missing);
        }
        let prefix = if from_cache { "Cached" } else { "Loaded" };
        let suffix = elapsed
            .map(|d| format!(" in {} ms", d.as_millis()))
            .unwrap_or_default();
        self.set_status(
            format!("{prefix} {} wav files{suffix}", self.wav_entries.len()),
            StatusTone::Info,
        );
    }
}
