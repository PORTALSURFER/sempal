use super::{
    AudioOutputConfig, AudioPlayer, Collection, CollectionId, DecodedWaveform, SampleSource,
    ScanMode, SelectionState, SourceDatabase, SourceDbError, SourceId, WavEntry, WaveformRenderer,
    audio_cache::AudioCache, jobs, source_folders, undo, wavs,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(super) struct RowFlags {
    pub(super) focused: bool,
    pub(super) loaded: bool,
}

pub(super) struct MissingState {
    pub(super) sources: HashSet<SourceId>,
    pub(super) wavs: HashMap<SourceId, HashSet<PathBuf>>,
}

pub(super) struct LibraryState {
    pub(super) sources: Vec<SampleSource>,
    pub(super) collections: Vec<Collection>,
    pub(super) missing: MissingState,
}

pub(super) struct WavCacheState {
    pub(super) entries: HashMap<SourceId, Vec<WavEntry>>,
    pub(super) lookup: HashMap<SourceId, HashMap<PathBuf, usize>>,
}

impl WavCacheState {
    pub(super) fn ensure_lookup(&mut self, source_id: &SourceId) {
        if self.lookup.contains_key(source_id) {
            return;
        }
        let Some(entries) = self.entries.get(source_id) else {
            return;
        };
        let lookup = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.relative_path.clone(), index))
            .collect();
        self.lookup.insert(source_id.clone(), lookup);
    }

    pub(super) fn rebuild_lookup(&mut self, source_id: &SourceId) {
        self.lookup.remove(source_id);
        self.ensure_lookup(source_id);
    }
}

pub(super) struct WavSelectionState {
    pub(super) selected_wav: Option<PathBuf>,
    pub(super) loaded_wav: Option<PathBuf>,
    pub(super) loaded_audio: Option<LoadedAudio>,
}

pub(super) struct ControllerSampleViewState {
    pub(super) renderer: WaveformRenderer,
    pub(super) waveform: WaveformState,
    pub(super) wav: WavSelectionState,
}

pub(super) struct SelectionContextState {
    pub(super) selected_source: Option<SourceId>,
    pub(super) last_selected_browsable_source: Option<SourceId>,
    pub(super) selected_collection: Option<CollectionId>,
}

pub(super) struct AppSettingsState {
    pub(super) feature_flags: crate::sample_sources::config::FeatureFlags,
    pub(super) model: crate::sample_sources::config::ModelSettings,
    pub(super) updates: crate::sample_sources::config::UpdateSettings,
    pub(super) audio_output: AudioOutputConfig,
    pub(super) controls: crate::sample_sources::config::InteractionOptions,
    pub(super) trash_folder: Option<std::path::PathBuf>,
    pub(super) collection_export_root: Option<PathBuf>,
}

pub(super) struct LibraryCacheState {
    pub(super) db: HashMap<SourceId, Rc<SourceDatabase>>,
    pub(super) wav: WavCacheState,
}

impl LibraryCacheState {
    /// Resolve or open the database for `source`, caching the handle.
    pub(super) fn database_for(
        &mut self,
        source: &SampleSource,
    ) -> Result<Rc<SourceDatabase>, SourceDbError> {
        if let Some(existing) = self.db.get(&source.id) {
            return Ok(existing.clone());
        }
        let db = Rc::new(SourceDatabase::open(&source.root)?);
        self.db.insert(source.id.clone(), db.clone());
        Ok(db)
    }
}

pub(super) struct BrowserCacheState {
    pub(super) labels: HashMap<SourceId, Vec<String>>,
    pub(super) analysis_failures: HashMap<SourceId, HashMap<PathBuf, String>>,
    pub(super) search: wavs::BrowserSearchCache,
    pub(super) predictions: HashMap<SourceId, PredictionCache>,
    pub(super) prediction_categories: Option<PredictionCategories>,
    pub(super) prediction_categories_checked: bool,
}

pub(super) struct PredictionCache {
    pub(super) model_id: Option<String>,
    pub(super) rows: Vec<Option<crate::egui_app::state::PredictedCategory>>,
    pub(super) user_overrides: Vec<bool>,
}

pub(super) struct PredictionCategories {
    pub(super) model_id: String,
    pub(super) classes: Vec<String>,
}

pub(super) struct FolderBrowsersState {
    pub(super) models: HashMap<SourceId, source_folders::FolderBrowserModel>,
}

pub(super) struct ControllerUiCacheState {
    pub(super) browser: BrowserCacheState,
    pub(super) folders: FolderBrowsersState,
}

pub(super) struct ControllerSelectionState {
    pub(super) ctx: SelectionContextState,
    pub(super) range: SelectionState,
    pub(super) suppress_autoplay_once: bool,
}

pub(super) struct ControllerAudioState {
    pub(super) player: Option<Rc<RefCell<AudioPlayer>>>,
    pub(super) cache: AudioCache,
    pub(super) pending_loop_disable_at: Option<Instant>,
}

pub(super) struct ControllerRuntimeState {
    pub(super) jobs: jobs::ControllerJobs,
    pub(super) analysis: super::analysis_jobs::AnalysisWorkerPool,
    #[cfg(test)]
    pub(super) progress_cancel_after: Option<usize>,
}

pub(super) struct ControllerHistoryState {
    pub(super) undo_stack: undo::UndoStack<super::EguiController>,
    pub(super) random_history: RandomHistoryState,
}

pub(super) struct WavEntriesState {
    pub(super) entries: Vec<WavEntry>,
    pub(super) lookup: HashMap<PathBuf, usize>,
}

pub(super) struct WaveformState {
    pub(super) size: [u32; 2],
    pub(super) decoded: Option<DecodedWaveform>,
    pub(super) render_meta: Option<wavs::WaveformRenderMeta>,
}

#[derive(Clone)]
pub(super) struct RandomHistoryEntry {
    pub(super) source_id: SourceId,
    pub(super) relative_path: PathBuf,
}

pub(super) struct RandomHistoryState {
    pub(super) entries: VecDeque<RandomHistoryEntry>,
    pub(super) cursor: Option<usize>,
}

pub(super) struct WavLoadJob {
    pub(super) source_id: SourceId,
    pub(super) root: PathBuf,
}

pub(super) struct WavLoadResult {
    pub(super) source_id: SourceId,
    pub(super) result: Result<Vec<WavEntry>, LoadEntriesError>,
    pub(super) elapsed: Duration,
}

#[derive(Clone)]
pub(super) struct PendingAudio {
    pub(super) request_id: u64,
    pub(super) source_id: SourceId,
    pub(super) root: PathBuf,
    pub(super) relative_path: PathBuf,
    pub(super) intent: AudioLoadIntent,
}

#[derive(Clone)]
pub(super) struct PendingPlayback {
    pub(super) source_id: SourceId,
    pub(super) relative_path: PathBuf,
    pub(super) looped: bool,
    pub(super) start_override: Option<f32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AudioLoadIntent {
    Selection,
    CollectionPreview,
}

pub(super) struct ScanResult {
    pub(super) source_id: SourceId,
    pub(super) mode: ScanMode,
    pub(super) result: Result<
        crate::sample_sources::scanner::ScanStats,
        crate::sample_sources::scanner::ScanError,
    >,
}

pub(super) enum ScanJobMessage {
    Progress {
        completed: usize,
        detail: Option<String>,
    },
    Finished(ScanResult),
}

#[derive(Clone)]
pub(super) struct UpdateCheckResult {
    pub(super) result: Result<crate::updater::UpdateCheckOutcome, String>,
}

#[derive(Debug)]
pub(super) enum LoadEntriesError {
    Db(SourceDbError),
    Message(String),
}

impl std::fmt::Display for LoadEntriesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadEntriesError::Db(err) => write!(f, "{err}"),
            LoadEntriesError::Message(msg) => f.write_str(msg),
        }
    }
}

impl From<String> for LoadEntriesError {
    fn from(value: String) -> Self {
        LoadEntriesError::Message(value)
    }
}

#[derive(Clone)]
pub(super) struct LoadedAudio {
    pub(super) source_id: SourceId,
    pub(super) relative_path: PathBuf,
    pub(super) bytes: Vec<u8>,
    pub(super) duration_seconds: f32,
    pub(super) sample_rate: u32,
    pub(super) channels: u16,
}
