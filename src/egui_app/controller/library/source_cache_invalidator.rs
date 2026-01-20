use super::{
    ControllerUiCacheState, LibraryCacheState, MissingState, SourceDatabase, SourceId,
    controller_state::FeatureCache,
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
};

pub(crate) struct SourceCacheInvalidator<'a> {
    db_cache: &'a mut HashMap<SourceId, Rc<SourceDatabase>>,
    wav_cache: &'a mut HashMap<SourceId, super::WavEntriesState>,
    label_cache: &'a mut HashMap<SourceId, Vec<String>>,
    bpm_cache: &'a mut HashMap<SourceId, HashMap<PathBuf, Option<f32>>>,
    duration_cache: &'a mut HashMap<SourceId, HashMap<PathBuf, f32>>,
    analysis_failures_cache: &'a mut HashMap<SourceId, HashMap<PathBuf, String>>,
    feature_cache: &'a mut HashMap<SourceId, FeatureCache>,
    missing_wavs: &'a mut HashMap<SourceId, HashSet<PathBuf>>,
    folder_browsers: &'a mut HashMap<SourceId, crate::egui_app::controller::library::source_folders::FolderBrowserModel>,
}

impl<'a> SourceCacheInvalidator<'a> {
    pub(crate) fn new_from_state(
        cache: &'a mut LibraryCacheState,
        ui_cache: &'a mut ControllerUiCacheState,
        missing: &'a mut MissingState,
    ) -> Self {
        Self::new(
            &mut cache.db,
            &mut cache.wav.entries,
            &mut ui_cache.browser.labels,
            &mut ui_cache.browser.bpm_values,
            &mut ui_cache.browser.durations,
            &mut ui_cache.browser.analysis_failures,
            &mut ui_cache.browser.features,
            &mut missing.wavs,
            &mut ui_cache.folders.models,
        )
    }

    pub(crate) fn new(
        db_cache: &'a mut HashMap<SourceId, Rc<SourceDatabase>>,
        wav_cache: &'a mut HashMap<SourceId, super::WavEntriesState>,
        label_cache: &'a mut HashMap<SourceId, Vec<String>>,
        bpm_cache: &'a mut HashMap<SourceId, HashMap<PathBuf, Option<f32>>>,
        duration_cache: &'a mut HashMap<SourceId, HashMap<PathBuf, f32>>,
        analysis_failures_cache: &'a mut HashMap<SourceId, HashMap<PathBuf, String>>,
        feature_cache: &'a mut HashMap<SourceId, FeatureCache>,
        missing_wavs: &'a mut HashMap<SourceId, HashSet<PathBuf>>,
        folder_browsers: &'a mut HashMap<SourceId, crate::egui_app::controller::library::source_folders::FolderBrowserModel>,
    ) -> Self {
        Self {
            db_cache,
            wav_cache,
            label_cache,
            bpm_cache,
            duration_cache,
            analysis_failures_cache,
            feature_cache,
            missing_wavs,
            folder_browsers,
        }
    }

    pub(crate) fn invalidate_wav_related(&mut self, source_id: &SourceId) {
        self.wav_cache.remove(source_id);
        self.label_cache.remove(source_id);
        self.bpm_cache.remove(source_id);
        self.duration_cache.remove(source_id);
        self.analysis_failures_cache.remove(source_id);
        self.feature_cache.remove(source_id);
        self.missing_wavs.remove(source_id);
    }

    pub(crate) fn invalidate_db_cache(&mut self, source_id: &SourceId) {
        self.db_cache.remove(source_id);
    }

    pub(crate) fn invalidate_folder_browser(&mut self, source_id: &SourceId) {
        self.folder_browsers.remove(source_id);
    }

    pub(crate) fn invalidate_all(&mut self, source_id: &SourceId) {
        self.invalidate_db_cache(source_id);
        self.invalidate_wav_related(source_id);
        self.invalidate_folder_browser(source_id);
    }
}
