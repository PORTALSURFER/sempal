use super::{SampleSource, SampleTag, SourceId, WavEntry};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub(in super) struct TaggingService<'a> {
    selected_source: Option<&'a SourceId>,
    wav_entries: &'a mut [WavEntry],
    wav_lookup: &'a HashMap<PathBuf, usize>,
    wav_cache: &'a mut HashMap<SourceId, Vec<WavEntry>>,
    wav_cache_lookup: &'a mut HashMap<SourceId, HashMap<PathBuf, usize>>,
}

impl<'a> TaggingService<'a> {
    pub(in super) fn new(
        selected_source: Option<&'a SourceId>,
        wav_entries: &'a mut [WavEntry],
        wav_lookup: &'a HashMap<PathBuf, usize>,
        wav_cache: &'a mut HashMap<SourceId, Vec<WavEntry>>,
        wav_cache_lookup: &'a mut HashMap<SourceId, HashMap<PathBuf, usize>>,
    ) -> Self {
        Self {
            selected_source,
            wav_entries,
            wav_lookup,
            wav_cache,
            wav_cache_lookup,
        }
    }

    pub(in super) fn apply_sample_tag(
        &mut self,
        source: &SampleSource,
        path: &Path,
        target_tag: SampleTag,
        require_present: bool,
    ) -> Result<(), String> {
        if self.selected_source == Some(&source.id) {
            if let Some(index) = self.wav_lookup.get(path).copied() {
                if let Some(entry) = self.wav_entries.get_mut(index) {
                    entry.tag = target_tag;
                }
            } else if require_present {
                return Err("Sample not found".into());
            }
        }

        if self.wav_cache.contains_key(&source.id) {
            self.ensure_wav_cache_lookup(&source.id);
            if let Some(index) = self
                .wav_cache_lookup
                .get(&source.id)
                .and_then(|lookup| lookup.get(path))
                .copied()
                && let Some(cache) = self.wav_cache.get_mut(&source.id)
                && let Some(entry) = cache.get_mut(index)
            {
                entry.tag = target_tag;
            }
        }

        Ok(())
    }

    fn ensure_wav_cache_lookup(&mut self, source_id: &SourceId) {
        if self.wav_cache_lookup.contains_key(source_id) {
            return;
        }
        let Some(entries) = self.wav_cache.get(source_id) else {
            return;
        };
        let lookup = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.relative_path.clone(), index))
            .collect();
        self.wav_cache_lookup.insert(source_id.clone(), lookup);
    }
}

