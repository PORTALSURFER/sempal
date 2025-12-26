use super::analysis_jobs;
use super::jobs;
use super::state;
use super::store::{DbSimilarityPrepStore, SimilarityPrepStore};
use super::*;
use crate::analysis::hdbscan::{HdbscanConfig, HdbscanMethod};
use crate::egui_app::state::ProgressTaskKind;
use std::thread;

struct SimilarityPrepStartPlan {
    scan_completed_at: Option<i64>,
    needs_embeddings: bool,
    skip_scan: bool,
    state: SimilarityPrepState,
}

fn plan_similarity_prep_start(
    store: &impl SimilarityPrepStore,
    source: &SampleSource,
    umap_version: String,
    force_full_analysis: bool,
) -> SimilarityPrepStartPlan {
    let scan_completed_at = store.read_scan_timestamp(source);
    let prep_scan_at = store.read_prep_timestamp(source);
    let skip_scan = scan_completed_at.is_some() && scan_completed_at == prep_scan_at;
    let needs_embeddings = !store.source_has_embeddings(source);
    let state = state::build_initial_state(state::SimilarityPrepInit {
        source_id: source.id.clone(),
        umap_version,
        scan_completed_at,
        skip_scan,
        needs_embeddings,
        force_full_analysis,
    });
    SimilarityPrepStartPlan {
        scan_completed_at,
        needs_embeddings,
        skip_scan,
        state,
    }
}

fn clear_similarity_prep_state(state: &mut Option<SimilarityPrepState>) -> bool {
    if state.is_some() {
        *state = None;
        true
    } else {
        false
    }
}

impl EguiController {
    pub fn prepare_similarity_for_selected_source(&mut self) {
        let force_full_analysis = self.runtime.similarity_prep_force_full_analysis_next;
        self.runtime.similarity_prep_force_full_analysis_next = false;
        self.prepare_similarity_for_selected_source_with_options(force_full_analysis);
    }

    pub fn prepare_similarity_for_selected_source_with_options(
        &mut self,
        force_full_analysis: bool,
    ) {
        if let Err(err) = crate::analysis::embedding::warmup_panns() {
            tracing::warn!("PANNs warmup failed: {err}");
        }
        if self.runtime.similarity_prep.is_some() {
            self.refresh_similarity_prep_progress();
            self.set_status_message(StatusMessage::SimilarityPrepAlreadyRunning);
            return;
        }
        if self.runtime.jobs.scan_in_progress() {
            self.set_status_message(StatusMessage::SimilarityScanAlreadyRunning);
            return;
        }
        if self.runtime.jobs.umap_build_in_progress() {
            self.set_status_message(StatusMessage::TsneBuildAlreadyRunning);
            return;
        }
        let Some(source) = self.current_source() else {
            self.set_status_message(StatusMessage::SelectSourceFirst {
                tone: StatusTone::Warning,
            });
            return;
        };
        let store = DbSimilarityPrepStore;
        let plan = plan_similarity_prep_start(
            &store,
            &source,
            self.ui.map.umap_version.clone(),
            force_full_analysis,
        );
        self.runtime.similarity_prep = Some(plan.state);
        self.apply_similarity_prep_duration_cap();
        self.apply_similarity_prep_fast_mode();
        self.apply_similarity_prep_full_analysis(force_full_analysis);
        self.apply_similarity_prep_worker_boost();
        self.set_status_message(StatusMessage::PreparingSimilarity {
            source: source.root.display().to_string(),
        });
        self.show_similarity_prep_progress(0, false);
        if plan.skip_scan {
            if plan.needs_embeddings || force_full_analysis {
                self.ensure_similarity_prep_progress(0, true);
                self.set_similarity_embedding_detail();
                self.enqueue_similarity_backfill(source, force_full_analysis);
            } else {
                self.set_similarity_analysis_detail();
                self.refresh_similarity_prep_progress();
            }
        } else {
            self.set_similarity_scan_detail();
            self.request_hard_sync();
        }
    }

    pub fn set_similarity_prep_force_full_analysis_next(&mut self, enabled: bool) {
        self.runtime.similarity_prep_force_full_analysis_next = enabled;
    }

    pub fn similarity_prep_force_full_analysis_next(&self) -> bool {
        self.runtime.similarity_prep_force_full_analysis_next
    }

    pub fn similarity_prep_in_progress(&self) -> bool {
        self.runtime.similarity_prep.is_some()
            || self.runtime.jobs.scan_in_progress()
            || self.runtime.jobs.umap_build_in_progress()
            || self.runtime.jobs.umap_cluster_build_in_progress()
    }

    pub fn similarity_prep_is_finalizing(&self) -> bool {
        self.runtime
            .similarity_prep
            .as_ref()
            .is_some_and(|state| state.stage == SimilarityPrepStage::Finalizing)
    }

    pub fn similarity_prep_debug_snapshot(&self) -> String {
        let Some(state) = self.runtime.similarity_prep.as_ref() else {
            return "similarity_prep=idle".to_string();
        };
        let mut out = format!(
            "stage={:?} skip_backfill={} scan_in_progress={} umap_in_progress={} clusters_in_progress={}",
            state.stage,
            state.skip_backfill,
            self.runtime.jobs.scan_in_progress(),
            self.runtime.jobs.umap_build_in_progress(),
            self.runtime.jobs.umap_cluster_build_in_progress()
        );
        if let Some(source) = self.find_source_by_id(&state.source_id)
            && let Ok(progress) = analysis_jobs::current_progress_for_source(&source)
        {
            out.push_str(&format!(" analysis_progress={progress}"));
        }
        out
    }

    pub(super) fn handle_similarity_scan_finished(
        &mut self,
        source_id: &SourceId,
        scan_changed: bool,
    ) {
        if !matches_similarity_stage(
            &self.runtime.similarity_prep,
            source_id,
            SimilarityPrepStage::AwaitScan,
        ) {
            return;
        }
        if let Some(source) = self.find_source_by_id(source_id) {
            let store = DbSimilarityPrepStore;
            let scan_completed_at = store.read_scan_timestamp(&source);
            let transition = if let Some(state) = self.runtime.similarity_prep.as_mut() {
                state::apply_scan_finished(state, scan_completed_at, scan_changed)
            } else {
                return;
            };
            if transition.should_enqueue_embeddings {
                self.ensure_similarity_prep_progress(0, true);
                self.set_similarity_embedding_detail();
                self.enqueue_similarity_backfill(source, transition.force_full);
            } else {
                self.refresh_similarity_prep_progress();
            }
        }
    }

    pub(super) fn handle_similarity_analysis_progress(
        &mut self,
        progress: &analysis_jobs::AnalysisProgress,
    ) {
        if progress.pending > 0 || progress.running > 0 {
            return;
        }
        let (source_id, umap_version) = {
            let Some(state) = self.runtime.similarity_prep.as_mut() else {
                return;
            };
            let Some(request) = state::start_finalize_if_ready(state) else {
                return;
            };
            (request.source_id, request.umap_version)
        };
        self.set_status_message(StatusMessage::FinalizingSimilarityPrep);
        self.show_similarity_finalize_progress();
        self.set_similarity_finalize_detail();
        self.start_similarity_finalize(source_id, umap_version);
    }

    pub(super) fn handle_similarity_prep_result(&mut self, result: jobs::SimilarityPrepResult) {
        let state = self.runtime.similarity_prep.take();
        if state.as_ref().map(|s| &s.source_id) != Some(&result.source_id) {
            return;
        }
        self.restore_similarity_prep_duration_cap();
        self.restore_similarity_prep_fast_mode();
        self.restore_similarity_prep_full_analysis();
        self.restore_similarity_prep_worker_count();
        if self.ui.progress.task == Some(ProgressTaskKind::Analysis) {
            self.clear_progress();
        }
        match result.result {
            Ok(outcome) => {
                if let Some(scan_completed_at) = state.as_ref().and_then(|s| s.scan_completed_at) {
                    if let Some(source) = self.find_source_by_id(&result.source_id) {
                        let store = DbSimilarityPrepStore;
                        store.record_prep_scan_timestamp(&source, scan_completed_at);
                    }
                }
                self.ui.map.bounds = None;
                self.ui.map.last_query = None;
                self.ui.map.cached_cluster_centroids_key = None;
                self.ui.map.cached_cluster_centroids = None;
                self.ui.map.auto_cluster_build_requested_key = None;
                self.set_status_message(StatusMessage::SimilarityReady {
                    cluster_count: outcome.cluster_stats.cluster_count,
                    noise_ratio: outcome.cluster_stats.noise_ratio,
                });
            }
            Err(err) => {
                self.set_status_message(StatusMessage::SimilarityPrepFailed { err });
            }
        }
    }

    pub(super) fn cancel_similarity_prep(&mut self, source_id: &SourceId) {
        let matches = self
            .runtime
            .similarity_prep
            .as_ref()
            .is_some_and(|state| &state.source_id == source_id);
        if !matches {
            return;
        }
        clear_similarity_prep_state(&mut self.runtime.similarity_prep);
        self.restore_similarity_prep_duration_cap();
        self.restore_similarity_prep_fast_mode();
        self.restore_similarity_prep_full_analysis();
        self.restore_similarity_prep_worker_count();
        if self.ui.progress.task == Some(ProgressTaskKind::Analysis) {
            self.clear_progress();
        }
    }
}

fn matches_similarity_stage(
    state: &Option<SimilarityPrepState>,
    source_id: &SourceId,
    stage: SimilarityPrepStage,
) -> bool {
    state
        .as_ref()
        .is_some_and(|entry| entry.source_id == *source_id && entry.stage == stage)
}

impl EguiController {
    fn apply_similarity_prep_duration_cap(&mut self) {
        let max_duration = if self.similarity_prep_duration_cap_enabled() {
            self.settings.analysis.max_analysis_duration_seconds
        } else {
            0.0
        };
        self.runtime
            .analysis
            .set_max_analysis_duration_seconds(max_duration);
    }

    fn restore_similarity_prep_duration_cap(&mut self) {
        self.runtime
            .analysis
            .set_max_analysis_duration_seconds(self.settings.analysis.max_analysis_duration_seconds);
    }

    fn apply_similarity_prep_fast_mode(&mut self) {
        if !self.similarity_prep_fast_mode_enabled() {
            return;
        }
        let sample_rate = self.similarity_prep_fast_sample_rate();
        let version = crate::analysis::version::analysis_version_for_sample_rate(sample_rate);
        self.runtime.analysis.set_analysis_sample_rate(sample_rate);
        self.runtime
            .analysis
            .set_analysis_version_override(Some(version));
    }

    fn restore_similarity_prep_fast_mode(&mut self) {
        self.runtime
            .analysis
            .set_analysis_sample_rate(crate::analysis::audio::ANALYSIS_SAMPLE_RATE);
        self.runtime.analysis.set_analysis_version_override(None);
    }

    fn apply_similarity_prep_full_analysis(&mut self, force_full_analysis: bool) {
        if !force_full_analysis {
            return;
        }
        self.runtime.analysis.set_analysis_cache_enabled(false);
    }

    fn restore_similarity_prep_full_analysis(&mut self) {
        self.runtime.analysis.set_analysis_cache_enabled(true);
    }

    fn apply_similarity_prep_worker_boost(&mut self) {
        if self.settings.analysis.analysis_worker_count != 0 {
            return;
        }
        let boosted = thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(1)
            .max(1)
            .min(64);
        self.runtime.performance.idle_worker_override = Some(boosted);
        self.runtime.analysis.set_worker_count(boosted);
    }

    fn restore_similarity_prep_worker_count(&mut self) {
        self.runtime.performance.idle_worker_override = None;
        self.runtime
            .analysis
            .set_worker_count(self.settings.analysis.analysis_worker_count);
    }

    fn enqueue_similarity_backfill(&mut self, source: SampleSource, force_full_analysis: bool) {
        let tx = self.runtime.jobs.message_sender();
        thread::spawn(move || {
            let analysis_result = if force_full_analysis {
                analysis_jobs::enqueue_jobs_for_source_backfill_full(&source)
            } else {
                analysis_jobs::enqueue_jobs_for_source_backfill(&source)
            };
            match analysis_result {
                Ok((inserted, progress)) => {
                    if inserted > 0 {
                        let _ = tx.send(jobs::JobMessage::Analysis(
                            analysis_jobs::AnalysisJobMessage::EnqueueFinished {
                                inserted,
                                progress,
                            },
                        ));
                    }
                }
                Err(err) => {
                    let _ = tx.send(jobs::JobMessage::Analysis(
                        analysis_jobs::AnalysisJobMessage::EnqueueFailed(err),
                    ));
                }
            }

            let embed_result = analysis_jobs::enqueue_jobs_for_embedding_backfill(&source);
            match embed_result {
                Ok((inserted, progress)) => {
                    if inserted > 0 {
                        let _ = tx.send(jobs::JobMessage::Analysis(
                            analysis_jobs::AnalysisJobMessage::EmbeddingBackfillEnqueueFinished {
                                inserted,
                                progress,
                            },
                        ));
                    } else {
                        let _ = tx.send(jobs::JobMessage::Analysis(
                            analysis_jobs::AnalysisJobMessage::Progress {
                                source_id: Some(source.id.clone()),
                                progress,
                            },
                        ));
                    }
                }
                Err(err) => {
                    let _ = tx.send(jobs::JobMessage::Analysis(
                        analysis_jobs::AnalysisJobMessage::EmbeddingBackfillEnqueueFailed(err),
                    ));
                }
            }
        });
    }

    fn start_similarity_finalize(&mut self, source_id: SourceId, umap_version: String) {
        let tx = self.runtime.jobs.message_sender();
        thread::spawn(move || {
            let result = run_similarity_finalize(&source_id, &umap_version);
            let _ = tx.send(jobs::JobMessage::SimilarityPrepared(
                jobs::SimilarityPrepResult { source_id, result },
            ));
        });
    }

    fn find_source_by_id(&self, source_id: &SourceId) -> Option<SampleSource> {
        self.library
            .sources
            .iter()
            .find(|source| &source.id == source_id)
            .cloned()
    }
}

fn run_similarity_finalize(
    source_id: &SourceId,
    umap_version: &str,
) -> Result<jobs::SimilarityPrepOutcome, String> {
    let store = DbSimilarityPrepStore;
    let mut conn = store.open_source_db_for_similarity(source_id)?;
    let sample_id_prefix = format!("{}::%", source_id.as_str());
    crate::analysis::umap::build_umap_layout(
        &mut conn,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
        umap_version,
        0,
        0.95,
    )?;
    let layout_rows = store.count_umap_layout_rows(
        &conn,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
        umap_version,
        &sample_id_prefix,
    )?;
    if layout_rows == 0 {
        return Err(format!(
            "No t-SNE layout rows for source {} (check embeddings)",
            source_id.as_str()
        ));
    }
    let cluster_stats = crate::analysis::hdbscan::build_hdbscan_clusters_for_sample_id_prefix(
        &mut conn,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
        HdbscanMethod::Umap,
        Some(umap_version),
        Some(sample_id_prefix.as_str()),
        HdbscanConfig {
            min_cluster_size: DEFAULT_CLUSTER_MIN_SIZE,
            min_samples: None,
            allow_single_cluster: false,
        },
    )?;
    crate::analysis::flush_ann_index(&conn)?;
    Ok(jobs::SimilarityPrepOutcome {
        cluster_stats,
        umap_version: umap_version.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_sources::SampleSource;
    use std::path::PathBuf;

    struct FakeStore {
        scan_completed_at: Option<i64>,
        prep_completed_at: Option<i64>,
        has_embeddings: bool,
    }

    impl SimilarityPrepStore for FakeStore {
        fn read_scan_timestamp(&self, _source: &SampleSource) -> Option<i64> {
            self.scan_completed_at
        }

        fn read_prep_timestamp(&self, _source: &SampleSource) -> Option<i64> {
            self.prep_completed_at
        }

        fn source_has_embeddings(&self, _source: &SampleSource) -> bool {
            self.has_embeddings
        }

        fn record_prep_scan_timestamp(&self, _source: &SampleSource, _scan_completed_at: i64) {}

        fn current_analysis_progress(
            &self,
            _source: &SampleSource,
        ) -> Result<analysis_jobs::AnalysisProgress, String> {
            Err("not needed".to_string())
        }

        fn open_source_db_for_similarity(
            &self,
            _source_id: &SourceId,
        ) -> Result<rusqlite::Connection, String> {
            Err("not needed".to_string())
        }

        fn count_umap_layout_rows(
            &self,
            _conn: &rusqlite::Connection,
            _model_id: &str,
            _umap_version: &str,
            _sample_id_prefix: &str,
        ) -> Result<i64, String> {
            Err("not needed".to_string())
        }
    }

    fn sample_source() -> SampleSource {
        SampleSource::new(PathBuf::from("/tmp/source"))
    }

    #[test]
    fn plan_initial_state_tracks_skip_and_backfill() {
        let store = FakeStore {
            scan_completed_at: Some(10),
            prep_completed_at: Some(10),
            has_embeddings: true,
        };
        let plan = plan_similarity_prep_start(
            &store,
            &sample_source(),
            "v1".to_string(),
            false,
        );
        assert!(plan.skip_scan);
        assert!(plan.state.skip_backfill);
        assert!(!plan.needs_embeddings);

        let store = FakeStore {
            scan_completed_at: Some(10),
            prep_completed_at: Some(10),
            has_embeddings: false,
        };
        let plan = plan_similarity_prep_start(
            &store,
            &sample_source(),
            "v1".to_string(),
            false,
        );
        assert!(plan.skip_scan);
        assert!(!plan.state.skip_backfill);
        assert!(plan.needs_embeddings);
    }

    #[test]
    fn clear_state_reports_reset() {
        let mut state = None;
        assert!(!clear_similarity_prep_state(&mut state));
        state = Some(state::build_initial_state(state::SimilarityPrepInit {
            source_id: SourceId::new(),
            umap_version: "v1".to_string(),
            scan_completed_at: None,
            skip_scan: false,
            needs_embeddings: false,
            force_full_analysis: false,
        }));
        assert!(clear_similarity_prep_state(&mut state));
        assert!(state.is_none());
    }
}
