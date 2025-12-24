use super::analysis_jobs;
use super::jobs;
use super::*;
use crate::analysis::hdbscan::{HdbscanConfig, HdbscanMethod};
use crate::egui_app::state::ProgressTaskKind;
use std::thread;

pub(super) const DEFAULT_CLUSTER_MIN_SIZE: usize = 10;

impl EguiController {
    pub fn prepare_similarity_for_selected_source(&mut self) {
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
        let scan_completed_at = self.read_source_scan_timestamp(&source);
        let prep_scan_at = self.read_source_prep_timestamp(&source);
        let skip_scan = scan_completed_at.is_some() && scan_completed_at == prep_scan_at;
        let stage = if skip_scan {
            SimilarityPrepStage::AwaitEmbeddings
        } else {
            SimilarityPrepStage::AwaitScan
        };
        self.runtime.similarity_prep = Some(SimilarityPrepState {
            source_id: source.id.clone(),
            stage,
            umap_version: self.ui.map.umap_version.clone(),
            scan_completed_at,
            skip_backfill: skip_scan,
        });
        self.apply_similarity_prep_duration_cap();
        self.apply_similarity_prep_worker_boost();
        self.set_status_message(StatusMessage::PreparingSimilarity {
            source: source.root.display().to_string(),
        });
        self.show_similarity_prep_progress(0, false);
        if skip_scan {
            self.set_similarity_analysis_detail();
            self.refresh_similarity_prep_progress();
        } else {
            self.set_similarity_scan_detail();
            self.request_hard_sync();
        }
    }

    pub(crate) fn similarity_prep_in_progress(&self) -> bool {
        self.runtime.similarity_prep.is_some()
            || self.runtime.jobs.scan_in_progress()
            || self.runtime.jobs.umap_build_in_progress()
            || self.runtime.jobs.umap_cluster_build_in_progress()
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
            let scan_completed_at = self.read_source_scan_timestamp(&source);
            if let Some(state) = self.runtime.similarity_prep.as_mut() {
                state.stage = SimilarityPrepStage::AwaitEmbeddings;
                state.scan_completed_at = scan_completed_at;
                state.skip_backfill = !scan_changed;
            }
            if scan_changed {
                self.ensure_similarity_prep_progress(0, true);
                self.set_similarity_embedding_detail();
                self.enqueue_similarity_backfill(source);
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
            if state.stage != SimilarityPrepStage::AwaitEmbeddings {
                return;
            }
            state.stage = SimilarityPrepStage::Finalizing;
            (state.source_id.clone(), state.umap_version.clone())
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
        self.restore_similarity_prep_worker_count();
        if self.ui.progress.task == Some(ProgressTaskKind::Analysis) {
            self.clear_progress();
        }
        match result.result {
            Ok(outcome) => {
                if let Some(scan_completed_at) = state.and_then(|s| s.scan_completed_at) {
                    self.record_similarity_prep_scan_timestamp(&result.source_id, scan_completed_at);
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
        self.runtime.similarity_prep = None;
        self.restore_similarity_prep_duration_cap();
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
    fn read_source_scan_timestamp(&self, source: &SampleSource) -> Option<i64> {
        let db = SourceDatabase::open(&source.root).ok()?;
        db.get_metadata(crate::sample_sources::db::META_LAST_SCAN_COMPLETED_AT)
            .ok()
            .flatten()
            .and_then(|value| value.parse().ok())
    }

    fn read_source_prep_timestamp(&self, source: &SampleSource) -> Option<i64> {
        let db = SourceDatabase::open(&source.root).ok()?;
        db.get_metadata(crate::sample_sources::db::META_LAST_SIMILARITY_PREP_SCAN_AT)
            .ok()
            .flatten()
            .and_then(|value| value.parse().ok())
    }

    fn record_similarity_prep_scan_timestamp(&self, source_id: &SourceId, scan_completed_at: i64) {
        let Some(source) = self.find_source_by_id(source_id) else {
            return;
        };
        if let Ok(db) = SourceDatabase::open(&source.root) {
            let _ = db.set_metadata(
                crate::sample_sources::db::META_LAST_SIMILARITY_PREP_SCAN_AT,
                &scan_completed_at.to_string(),
            );
        }
    }

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

    fn enqueue_similarity_backfill(&mut self, source: SampleSource) {
        let tx = self.runtime.jobs.message_sender();
        thread::spawn(move || {
            let analysis_result = analysis_jobs::enqueue_jobs_for_source_backfill(&source);
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
                            analysis_jobs::AnalysisJobMessage::Progress(progress),
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

    fn refresh_similarity_prep_progress(&mut self) {
        let Some(state) = self.runtime.similarity_prep.as_ref() else {
            return;
        };
        match state.stage {
            SimilarityPrepStage::AwaitScan => {
                if self.runtime.jobs.scan_in_progress() {
                    self.ensure_similarity_prep_progress(0, false);
                    self.set_similarity_scan_detail();
                    return;
                }
                if self.ui.progress.visible {
                    return;
                }
                self.show_similarity_prep_progress(0, false);
                self.set_similarity_scan_detail();
            }
            SimilarityPrepStage::AwaitEmbeddings => {
                let progress = match analysis_jobs::current_progress() {
                    Ok(progress) => progress,
                    Err(_) => {
                        if !self.ui.progress.visible {
                            self.show_similarity_prep_progress(0, false);
                            self.set_similarity_analysis_detail();
                        }
                        return;
                    }
                };
                if progress.pending == 0 && progress.running == 0 {
                    self.handle_similarity_analysis_progress(&progress);
                    return;
                }
                self.ensure_similarity_prep_progress(progress.total(), true);
                self.ui.progress.total = progress.total();
                self.ui.progress.completed = progress.completed();
                let jobs_completed = progress.completed();
                let jobs_total = progress.total();
                let samples_completed = progress.samples_completed();
                let samples_total = progress.samples_total;
                let mut detail = format!(
                    "Analyzing… Jobs {jobs_completed}/{jobs_total} • Samples {samples_completed}/{samples_total}"
                );
                if progress.failed > 0 {
                    detail.push_str(&format!(" • {} failed", progress.failed));
                }
                self.ui.progress.detail = Some(detail);
            }
            SimilarityPrepStage::Finalizing => {
                self.ensure_similarity_finalize_progress();
                self.set_similarity_finalize_detail();
            }
        }
    }
}

fn run_similarity_finalize(
    source_id: &SourceId,
    umap_version: &str,
) -> Result<jobs::SimilarityPrepOutcome, String> {
    let mut conn = open_library_db_for_similarity()?;
    let sample_id_prefix = format!("{}::%", source_id.as_str());
    crate::analysis::umap::build_umap_layout(
        &mut conn,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
        umap_version,
        0,
        0.95,
    )?;
    let layout_rows = count_umap_layout_rows(
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

fn count_umap_layout_rows(
    conn: &rusqlite::Connection,
    model_id: &str,
    umap_version: &str,
    sample_id_prefix: &str,
) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM layout_umap
         WHERE model_id = ?1 AND umap_version = ?2 AND sample_id LIKE ?3",
        rusqlite::params![model_id, umap_version, sample_id_prefix],
        |row| row.get(0),
    )
    .map_err(|err| format!("Count layout rows failed: {err}"))
}

fn open_library_db_for_similarity() -> Result<rusqlite::Connection, String> {
    let root = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    let path = root.join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
    rusqlite::Connection::open(path).map_err(|err| format!("Open library DB failed: {err}"))
}
