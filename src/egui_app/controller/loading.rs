use super::*;

impl EguiController {
    pub(super) fn queue_wav_load(&mut self) {
        let Some(source) = self.current_source() else {
            return;
        };
        if let Some(entries) = self.wav_cache.get(&source.id).cloned() {
            self.apply_wav_entries(entries, true, Some(source.id.clone()), None);
            return;
        }
        self.wav_entries.clear();
        self.rebuild_wav_lookup();
        self.rebuild_triage_lists();
        if self.pending_source.as_ref() == Some(&source.id) {
            return;
        }
        self.pending_source = Some(source.id.clone());
        let job = WavLoadJob {
            source_id: source.id.clone(),
            root: source.root.clone(),
        };
        let _ = self.wav_job_tx.send(job);
        self.set_status(
            format!("Loading wavs for {}", source.root.display()),
            StatusTone::Info,
        );
    }

    pub(super) fn poll_wav_loader(&mut self) {
        while let Ok(message) = self.wav_job_rx.try_recv() {
            if Some(&message.source_id) != self.selected_source.as_ref() {
                continue;
            }
            match message.result {
                Ok(entries) => {
                    self.wav_cache
                        .insert(message.source_id.clone(), entries.clone());
                    self.apply_wav_entries(
                        entries,
                        false,
                        Some(message.source_id.clone()),
                        Some(message.elapsed),
                    );
                }
                Err(err) => {
                    self.set_status(format!("Failed to load wavs: {err}"), StatusTone::Error);
                }
            }
            self.pending_source = None;
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
        self.rebuild_wav_lookup();
        self.rebuild_triage_lists();
        let mut pending_applied = false;
        if let Some(path) = self.pending_select_path.take() {
            if self.wav_lookup.contains_key(&path) {
                self.select_wav_by_path(&path);
                pending_applied = true;
            }
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
            self.label_cache
                .insert(id, self.build_label_cache(&self.wav_entries));
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
