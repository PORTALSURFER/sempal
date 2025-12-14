use super::{SourceDatabase, SourceId};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
};

pub(in super) struct SourceCacheInvalidator<'a> {
    db_cache: &'a mut HashMap<SourceId, Rc<SourceDatabase>>,
    wav_cache: &'a mut HashMap<SourceId, Vec<super::WavEntry>>,
    wav_cache_lookup: &'a mut HashMap<SourceId, HashMap<PathBuf, usize>>,
    label_cache: &'a mut HashMap<SourceId, Vec<String>>,
    missing_wavs: &'a mut HashMap<SourceId, HashSet<PathBuf>>,
    folder_browsers: &'a mut HashMap<SourceId, super::source_folders::FolderBrowserModel>,
}

impl<'a> SourceCacheInvalidator<'a> {
    pub(in super) fn new(
        db_cache: &'a mut HashMap<SourceId, Rc<SourceDatabase>>,
        wav_cache: &'a mut HashMap<SourceId, Vec<super::WavEntry>>,
        wav_cache_lookup: &'a mut HashMap<SourceId, HashMap<PathBuf, usize>>,
        label_cache: &'a mut HashMap<SourceId, Vec<String>>,
        missing_wavs: &'a mut HashMap<SourceId, HashSet<PathBuf>>,
        folder_browsers: &'a mut HashMap<SourceId, super::source_folders::FolderBrowserModel>,
    ) -> Self {
        Self {
            db_cache,
            wav_cache,
            wav_cache_lookup,
            label_cache,
            missing_wavs,
            folder_browsers,
        }
    }

    pub(in super) fn invalidate_wav_related(&mut self, source_id: &SourceId) {
        self.wav_cache.remove(source_id);
        self.wav_cache_lookup.remove(source_id);
        self.label_cache.remove(source_id);
        self.missing_wavs.remove(source_id);
    }

    pub(in super) fn invalidate_db_cache(&mut self, source_id: &SourceId) {
        self.db_cache.remove(source_id);
    }

    pub(in super) fn invalidate_folder_browser(&mut self, source_id: &SourceId) {
        self.folder_browsers.remove(source_id);
    }

    pub(in super) fn invalidate_all(&mut self, source_id: &SourceId) {
        self.invalidate_db_cache(source_id);
        self.invalidate_wav_related(source_id);
        self.invalidate_folder_browser(source_id);
    }
}
