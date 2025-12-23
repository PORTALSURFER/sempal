use super::*;
use super::analysis_jobs;
use super::jobs;
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
        self.runtime.similarity_prep = Some(SimilarityPrepState {
            source_id: source.id.clone(),
            stage: SimilarityPrepStage::AwaitScan,
            umap_version: self.ui.map.umap_version.clone(),
        });
        self.set_status_message(StatusMessage::PreparingSimilarity {
            source: source.root.display().to_string(),
        });
        self.request_hard_sync();
    }

    pub(super) fn handle_similarity_scan_finished(&mut self, source_id: &SourceId) {
        if !matches_similarity_stage(&self.runtime.similarity_prep, source_id, SimilarityPrepStage::AwaitScan) {
            return;
        }
        if let Some(source) = self.find_source_by_id(source_id) {
            self.runtime.similarity_prep.as_mut().expect("checked").stage =
                SimilarityPrepStage::AwaitEmbeddings;
            self.enqueue_similarity_backfill(source);
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
        self.show_status_progress(
            ProgressTaskKind::Analysis,
            "Finalizing similarity prep",
            0,
            true,
        );
        self.start_similarity_finalize(source_id, umap_version);
    }

    pub(super) fn handle_similarity_prep_result(
        &mut self,
        result: jobs::SimilarityPrepResult,
    ) {
        let state = self.runtime.similarity_prep.take();
        if state.as_ref().map(|s| &s.source_id) != Some(&result.source_id) {
            return;
        }
        if self.ui.progress.task == Some(ProgressTaskKind::Analysis) {
            self.clear_progress();
        }
        match result.result {
            Ok(outcome) => {
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
                    return;
                }
                if self.ui.progress.visible {
                    return;
                }
                self.show_status_progress(
                    ProgressTaskKind::Analysis,
                    "Preparing similarity search",
                    0,
                    false,
                );
            }
            SimilarityPrepStage::AwaitEmbeddings => {
                let progress = match analysis_jobs::current_progress() {
                    Ok(progress) => progress,
                    Err(_) => {
                        if !self.ui.progress.visible {
                            self.show_status_progress(
                                ProgressTaskKind::Analysis,
                                "Preparing similarity search",
                                0,
                                false,
                            );
                        }
                        return;
                    }
                };
                if progress.pending == 0 && progress.running == 0 {
                    self.handle_similarity_analysis_progress(&progress);
                    return;
                }
                if !self.ui.progress.visible
                    || self.ui.progress.task != Some(ProgressTaskKind::Analysis)
                {
                    self.show_status_progress(
                        ProgressTaskKind::Analysis,
                        "Preparing similarity search",
                        progress.total(),
                        true,
                    );
                }
                self.ui.progress.total = progress.total();
                self.ui.progress.completed = progress.completed();
                let jobs_completed = progress.completed();
                let jobs_total = progress.total();
                let samples_completed = progress.samples_completed();
                let samples_total = progress.samples_total;
                let mut detail = format!(
                    "Jobs {jobs_completed}/{jobs_total} • Samples {samples_completed}/{samples_total}"
                );
                if progress.failed > 0 {
                    detail.push_str(&format!(" • {} failed", progress.failed));
                }
                self.ui.progress.detail = Some(detail);
            }
            SimilarityPrepStage::Finalizing => {
                if !self.ui.progress.visible
                    || self.ui.progress.task != Some(ProgressTaskKind::Analysis)
                {
                    self.show_status_progress(
                        ProgressTaskKind::Analysis,
                        "Finalizing similarity prep",
                        0,
                        true,
                    );
                }
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
    crate::analysis::rebuild_ann_index(&conn)?;
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
