use super::{SampleSource, SampleTag, SourceId, WavCacheState, WavEntry};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub(super) struct TaggingService<'a> {
    selected_source: Option<&'a SourceId>,
    wav_entries: &'a mut [WavEntry],
    wav_lookup: &'a HashMap<PathBuf, usize>,
    wav_cache: &'a mut WavCacheState,
}

impl<'a> TaggingService<'a> {
    pub(super) fn new(
        selected_source: Option<&'a SourceId>,
        wav_entries: &'a mut [WavEntry],
        wav_lookup: &'a HashMap<PathBuf, usize>,
        wav_cache: &'a mut WavCacheState,
    ) -> Self {
        Self {
            selected_source,
            wav_entries,
            wav_lookup,
            wav_cache,
        }
    }

    pub(super) fn apply_sample_tag(
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

        if self.wav_cache.entries.contains_key(&source.id) {
            self.wav_cache.ensure_lookup(&source.id);
            if let Some(index) = self
                .wav_cache
                .lookup
                .get(&source.id)
                .and_then(|lookup| lookup.get(path))
                .copied()
                && let Some(cache) = self.wav_cache.entries.get_mut(&source.id)
                && let Some(entry) = cache.get_mut(index)
            {
                entry.tag = target_tag;
            }
        }

        Ok(())
    }
}
