use super::*;
use std::collections::HashSet;
use std::path::Path;

impl EguiController {
    pub(in crate::egui_app::controller) fn rebuild_missing_lookup_for_source(
        &mut self,
        source_id: &SourceId,
    ) {
        let mut missing = HashSet::new();
        if let Some(cache) = self.wav_cache.get(source_id) {
            for entry in cache {
                if entry.missing {
                    missing.insert(entry.relative_path.clone());
                }
            }
        } else if self.selected_source.as_ref() == Some(source_id) {
            for entry in &self.wav_entries {
                if entry.missing {
                    missing.insert(entry.relative_path.clone());
                }
            }
        }
        self.missing.wavs.insert(source_id.clone(), missing);
    }

    pub(in crate::egui_app::controller) fn mark_sample_missing(
        &mut self,
        source: &SampleSource,
        relative_path: &Path,
    ) {
        match self.database_for(source) {
            Ok(db) => {
                let _ = db.set_missing(relative_path, true);
            }
            Err(SourceDbError::InvalidRoot(_)) => {
                self.mark_source_missing(&source.id, "Source folder missing");
            }
            Err(err) => {
                self.set_status(
                    format!("Failed to update missing flag: {err}"),
                    StatusTone::Warning,
                );
            }
        }
        if let Some(cache) = self.wav_cache.get_mut(&source.id)
            && let Some(entry) = cache
                .iter_mut()
                .find(|entry| entry.relative_path == relative_path)
        {
            entry.missing = true;
        }
        if self.selected_source.as_ref() == Some(&source.id)
            && let Some(index) = self.wav_lookup.get(relative_path).copied()
            && let Some(entry) = self.wav_entries.get_mut(index)
        {
            entry.missing = true;
        }
        self.missing
            .wavs
            .entry(source.id.clone())
            .or_default()
            .insert(relative_path.to_path_buf());
        self.invalidate_cached_audio(&source.id, relative_path);
    }

    fn ensure_missing_lookup_for_source(&mut self, source: &SampleSource) -> Result<(), String> {
        if self.missing.wavs.contains_key(&source.id) {
            return Ok(());
        }
        if self.missing.sources.contains(&source.id) {
            self.missing.wavs.entry(source.id.clone()).or_default();
            return Ok(());
        }
        let db = match self.database_for(source) {
            Ok(db) => db,
            Err(err) => {
                if matches!(err, SourceDbError::InvalidRoot(_)) {
                    self.mark_source_missing(&source.id, "Source folder missing");
                }
                return Err(err.to_string());
            }
        };
        let paths = db
            .list_missing_paths()
            .map_err(|err| format!("Failed to read missing files: {err}"))?;
        self.missing
            .wavs
            .insert(source.id.clone(), paths.into_iter().collect());
        Ok(())
    }

    pub(in crate::egui_app::controller) fn sample_missing(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
    ) -> bool {
        if self.missing.sources.contains(source_id) {
            return true;
        }
        if self.selected_source.as_ref() == Some(source_id)
            && let Some(index) = self.wav_lookup.get(relative_path)
            && let Some(entry) = self.wav_entries.get(*index)
        {
            return entry.missing;
        }
        if self.wav_cache.contains_key(source_id) {
            self.ensure_wav_cache_lookup(source_id);
            if let Some(index) = self
                .wav_cache_lookup
                .get(source_id)
                .and_then(|lookup| lookup.get(relative_path))
                .copied()
                && let Some(cache) = self.wav_cache.get(source_id)
                && let Some(entry) = cache.get(index)
            {
                return entry.missing;
            }
        }
        if let Some(set) = self.missing.wavs.get(source_id) {
            return set.contains(relative_path);
        }
        if let Some(source) = self.sources.iter().find(|s| &s.id == source_id).cloned() {
            if let Err(err) = self.ensure_missing_lookup_for_source(&source) {
                self.set_status(err, StatusTone::Warning);
                return true;
            }
            if let Some(set) = self.missing.wavs.get(source_id) {
                return set.contains(relative_path);
            }
        }
        false
    }

    pub(in crate::egui_app::controller) fn show_missing_waveform_notice(
        &mut self,
        relative_path: &Path,
    ) {
        let message = format!("File missing: {}", relative_path.display());
        self.clear_waveform_view();
        self.ui.waveform.notice = Some(message);
    }
}
