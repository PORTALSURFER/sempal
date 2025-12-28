use super::{
    AudioPlayer, Collection, CollectionId, DecodedWaveform, SampleSource, ScanMode,
    SelectionRange, SourceDatabase, SourceDbError, SourceId, WavEntry, WaveformRenderer,
    audio_cache::AudioCache, jobs, source_folders, undo, wavs,
};
use crate::{audio::AudioOutputConfig, selection::SelectionState};
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

impl MissingState {
    pub(super) fn new() -> Self {
        Self {
            sources: HashSet::new(),
            wavs: HashMap::new(),
        }
    }
}

pub(super) struct LibraryState {
    pub(super) sources: Vec<SampleSource>,
    pub(super) collections: Vec<Collection>,
    pub(super) missing: MissingState,
}

impl LibraryState {
    pub(super) fn new() -> Self {
        Self {
            sources: Vec::new(),
            collections: Vec::new(),
            missing: MissingState::new(),
        }
    }
}

pub(super) struct WavCacheState {
    pub(super) entries: HashMap<SourceId, WavEntriesState>,
}

impl WavCacheState {
    pub(super) fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub(super) fn insert_page(
        &mut self,
        source_id: SourceId,
        total: usize,
        page_size: usize,
        page_index: usize,
        entries: Vec<WavEntry>,
    ) {
        let cache = self
            .entries
            .entry(source_id)
            .or_insert_with(|| WavEntriesState::new(total, page_size));
        cache.total = total;
        cache.page_size = page_size;
        cache.insert_page(page_index, entries);
    }
}

pub(super) struct WavSelectionState {
    pub(super) selected_wav: Option<PathBuf>,
    pub(super) loaded_wav: Option<PathBuf>,
    pub(super) loaded_audio: Option<LoadedAudio>,
}

impl WavSelectionState {
    pub(super) fn new() -> Self {
        Self {
            selected_wav: None,
            loaded_wav: None,
            loaded_audio: None,
        }
    }
}

pub(super) struct ControllerSampleViewState {
    pub(super) renderer: WaveformRenderer,
    pub(super) waveform: WaveformState,
    pub(super) wav: WavSelectionState,
}

impl ControllerSampleViewState {
    pub(super) fn new(renderer: WaveformRenderer) -> Self {
        let (waveform_width, waveform_height) = renderer.dimensions();
        Self {
            renderer,
            waveform: WaveformState {
                size: [waveform_width, waveform_height],
                decoded: None,
                render_meta: None,
            },
            wav: WavSelectionState::new(),
        }
    }
}

pub(super) struct SelectionContextState {
    pub(super) selected_source: Option<SourceId>,
    pub(super) last_selected_browsable_source: Option<SourceId>,
    pub(super) selected_collection: Option<CollectionId>,
}

impl SelectionContextState {
    pub(super) fn new() -> Self {
        Self {
            selected_source: None,
            last_selected_browsable_source: None,
            selected_collection: None,
        }
    }
}

pub(super) struct SelectionUndoState {
    pub(super) label: String,
    pub(super) before: Option<SelectionRange>,
}

pub(super) struct AppSettingsState {
    pub(super) feature_flags: crate::sample_sources::config::FeatureFlags,
    pub(super) analysis: crate::sample_sources::config::AnalysisSettings,
    pub(super) updates: crate::sample_sources::config::UpdateSettings,
    pub(super) hints: crate::sample_sources::config::HintSettings,
    pub(super) app_data_dir: Option<std::path::PathBuf>,
    pub(super) audio_output: AudioOutputConfig,
    pub(super) controls: crate::sample_sources::config::InteractionOptions,
    pub(super) trash_folder: Option<std::path::PathBuf>,
    pub(super) collection_export_root: Option<PathBuf>,
}

impl AppSettingsState {
    pub(super) fn new() -> Self {
        Self {
            feature_flags: crate::sample_sources::config::FeatureFlags::default(),
            analysis: crate::sample_sources::config::AnalysisSettings::default(),
            updates: crate::sample_sources::config::UpdateSettings::default(),
            hints: crate::sample_sources::config::HintSettings::default(),
            app_data_dir: None,
            audio_output: AudioOutputConfig::default(),
            controls: crate::sample_sources::config::InteractionOptions::default(),
            trash_folder: None,
            collection_export_root: None,
        }
    }
}

pub(super) struct LibraryCacheState {
    pub(super) db: HashMap<SourceId, Rc<SourceDatabase>>,
    pub(super) wav: WavCacheState,
}

impl LibraryCacheState {
    pub(super) fn new() -> Self {
        Self {
            db: HashMap::new(),
            wav: WavCacheState::new(),
        }
    }

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
    pub(super) features: HashMap<SourceId, FeatureCache>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnalysisJobStatus {
    Pending,
    Running,
    Done,
    Failed,
    Canceled,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct FeatureStatus {
    pub(crate) has_features_v1: bool,
    pub(crate) duration_seconds: Option<f32>,
    pub(crate) sr_used: Option<i64>,
    pub(crate) analysis_status: Option<AnalysisJobStatus>,
}

pub(crate) struct FeatureCache {
    pub(crate) rows: Vec<Option<FeatureStatus>>,
}

pub(super) struct FolderBrowsersState {
    pub(super) models: HashMap<SourceId, source_folders::FolderBrowserModel>,
}

pub(super) struct ControllerUiCacheState {
    pub(super) browser: BrowserCacheState,
    pub(super) folders: FolderBrowsersState,
}

impl ControllerUiCacheState {
    pub(super) fn new() -> Self {
        Self {
            browser: BrowserCacheState {
                labels: HashMap::new(),
                analysis_failures: HashMap::new(),
                search: wavs::BrowserSearchCache::default(),
                features: HashMap::new(),
            },
            folders: FolderBrowsersState {
                models: HashMap::new(),
            },
        }
    }
}

pub(super) struct ControllerSelectionState {
    pub(super) ctx: SelectionContextState,
    pub(super) range: SelectionState,
    pub(super) pending_undo: Option<SelectionUndoState>,
    pub(super) suppress_autoplay_once: bool,
    pub(super) bpm_scale_beats: Option<f32>,
}

impl ControllerSelectionState {
    pub(super) fn new() -> Self {
        Self {
            ctx: SelectionContextState::new(),
            range: SelectionState::new(),
            pending_undo: None,
            suppress_autoplay_once: false,
            bpm_scale_beats: None,
        }
    }
}

pub(super) struct ControllerAudioState {
    pub(super) player: Option<Rc<RefCell<AudioPlayer>>>,
    pub(super) cache: AudioCache,
    pub(super) pending_loop_disable_at: Option<Instant>,
}

impl ControllerAudioState {
    pub(super) fn new(
        player: Option<Rc<RefCell<AudioPlayer>>>,
        cache_capacity: usize,
        history_limit: usize,
    ) -> Self {
        Self {
            player,
            cache: AudioCache::new(cache_capacity, history_limit),
            pending_loop_disable_at: None,
        }
    }
}

pub(super) struct ControllerRuntimeState {
    pub(super) jobs: jobs::ControllerJobs,
    pub(super) analysis: super::analysis_jobs::AnalysisWorkerPool,
    pub(super) performance: PerformanceGovernorState,
    pub(super) similarity_prep: Option<SimilarityPrepState>,
    pub(super) similarity_prep_last_error: Option<String>,
    pub(super) similarity_prep_force_full_analysis_next: bool,
    #[cfg(test)]
    pub(super) progress_cancel_after: Option<usize>,
}

impl ControllerRuntimeState {
    pub(super) fn new(
        jobs: jobs::ControllerJobs,
        analysis: super::analysis_jobs::AnalysisWorkerPool,
    ) -> Self {
        Self {
            jobs,
            analysis,
            performance: PerformanceGovernorState::new(),
            similarity_prep: None,
            similarity_prep_last_error: None,
            similarity_prep_force_full_analysis_next: false,
            #[cfg(test)]
            progress_cancel_after: None,
        }
    }
}

pub(super) struct PerformanceGovernorState {
    pub(super) last_user_activity_at: Option<Instant>,
    pub(super) last_slow_frame_at: Option<Instant>,
    pub(super) last_frame_at: Option<Instant>,
    pub(super) last_worker_count: Option<u32>,
    pub(super) idle_worker_override: Option<u32>,
}

impl PerformanceGovernorState {
    pub(super) fn new() -> Self {
        Self {
            last_user_activity_at: None,
            last_slow_frame_at: None,
            last_frame_at: None,
            last_worker_count: None,
            idle_worker_override: None,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct SimilarityPrepState {
    pub(super) source_id: SourceId,
    pub(super) stage: SimilarityPrepStage,
    pub(super) umap_version: String,
    pub(super) scan_completed_at: Option<i64>,
    pub(super) skip_backfill: bool,
    pub(super) force_full_analysis: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SimilarityPrepStage {
    AwaitScan,
    AwaitEmbeddings,
    Finalizing,
}

pub(super) struct ControllerHistoryState {
    pub(super) undo_stack: undo::UndoStack<super::EguiController>,
    pub(super) random_history: RandomHistoryState,
}

impl ControllerHistoryState {
    pub(super) fn new(undo_limit: usize) -> Self {
        Self {
            undo_stack: undo::UndoStack::new(undo_limit),
            random_history: RandomHistoryState::new(),
        }
    }
}

pub(super) struct WavEntriesState {
    pub(super) total: usize,
    pub(super) page_size: usize,
    pub(super) pages: HashMap<usize, Vec<WavEntry>>,
    pub(super) lookup: HashMap<PathBuf, usize>,
}

impl WavEntriesState {
    pub(super) fn new(total: usize, page_size: usize) -> Self {
        Self {
            total,
            page_size: page_size.max(1),
            pages: HashMap::new(),
            lookup: HashMap::new(),
        }
    }

    pub(super) fn clear(&mut self) {
        self.total = 0;
        self.pages.clear();
        self.lookup.clear();
    }

    pub(super) fn insert_page(&mut self, page_index: usize, entries: Vec<WavEntry>) {
        let offset = page_index * self.page_size;
        for (idx, entry) in entries.iter().enumerate() {
            self.insert_lookup(entry.relative_path.clone(), offset + idx);
        }
        self.pages.insert(page_index, entries);
    }

    pub(super) fn entry(&self, index: usize) -> Option<&WavEntry> {
        let page_index = index / self.page_size;
        let in_page = index % self.page_size;
        self.pages.get(&page_index).and_then(|page| page.get(in_page))
    }

    pub(super) fn entry_mut(&mut self, index: usize) -> Option<&mut WavEntry> {
        let page_index = index / self.page_size;
        let in_page = index % self.page_size;
        self.pages
            .get_mut(&page_index)
            .and_then(|page| page.get_mut(in_page))
    }

    pub(super) fn insert_lookup(&mut self, path: PathBuf, index: usize) {
        self.lookup.insert(path.clone(), index);
        let path_str = path.to_string_lossy();
        if path_str.contains('\\') {
            let normalized = path_str.replace('\\', "/");
            self.lookup
                .entry(PathBuf::from(normalized))
                .or_insert(index);
        }
        if path_str.contains('/') {
            let normalized = path_str.replace('/', "\\");
            self.lookup
                .entry(PathBuf::from(normalized))
                .or_insert(index);
        }
    }
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

impl RandomHistoryState {
    pub(super) fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            cursor: None,
        }
    }
}

pub(super) struct WavLoadJob {
    pub(super) source_id: SourceId,
    pub(super) root: PathBuf,
    pub(super) page_size: usize,
}

pub(super) struct WavLoadResult {
    pub(super) source_id: SourceId,
    pub(super) result: Result<Vec<WavEntry>, LoadEntriesError>,
    pub(super) elapsed: Duration,
    pub(super) total: usize,
    pub(super) page_index: usize,
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
