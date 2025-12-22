use super::*;
use super::analysis_jobs;
use super::jobs;
use crate::analysis::hdbscan::{HdbscanConfig, HdbscanMethod};
use crate::egui_app::state::ProgressTaskKind;
use std::thread;

const DEFAULT_CLUSTER_MIN_SIZE: usize = 10;

impl EguiController {
    pub fn prepare_similarity_for_selected_source(&mut self) {
        if self.runtime.similarity_prep.is_some() {
            self.set_status("Similarity prep already running", StatusTone::Info);
            return;
        }
        if self.runtime.jobs.scan_in_progress() {
            self.set_status("Scan already in progress", StatusTone::Info);
            return;
        }
        if self.runtime.jobs.umap_build_in_progress() {
            self.set_status("UMAP build already in progress", StatusTone::Info);
            return;
        }
        let Some(source) = self.current_source() else {
            self.set_status("Select a source first", StatusTone::Warning);
            return;
        };
        self.runtime.similarity_prep = Some(SimilarityPrepState {
            source_id: source.id.clone(),
            stage: SimilarityPrepStage::AwaitScan,
            umap_version: self.ui.map.umap_version.clone(),
        });
        self.set_status(
            format!("Preparing similarity search for {}", source.root.display()),
            StatusTone::Busy,
        );
        self.request_hard_sync();
    }

    pub(super) fn handle_similarity_scan_finished(&mut self, source_id: &SourceId) {
        if !matches_similarity_stage(&self.runtime.similarity_prep, source_id, SimilarityPrepStage::AwaitScan) {
            return;
        }
        if let Some(source) = self.find_source_by_id(source_id) {
            self.runtime.similarity_prep.as_mut().expect("checked").stage =
                SimilarityPrepStage::AwaitEmbeddings;
            self.enqueue_embedding_backfill(source);
        }
    }

    pub(super) fn handle_similarity_analysis_progress(
        &mut self,
        progress: &analysis_jobs::types::AnalysisProgress,
    ) {
        if progress.pending > 0 || progress.running > 0 {
            return;
        }
        let Some(state) = self.runtime.similarity_prep.as_mut() else {
            return;
        };
        if state.stage != SimilarityPrepStage::AwaitEmbeddings {
            return;
        }
        state.stage = SimilarityPrepStage::Finalizing;
        self.set_status("Finalizing similarity prep...", StatusTone::Busy);
        self.show_status_progress(ProgressTaskKind::Analysis, "Finalizing similarity prep", 0, true);
        let source_id = state.source_id.clone();
        let umap_version = state.umap_version.clone();
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
                self.set_status(
                    format!(
                        "Similarity ready: {} clusters (noise {:.1}%)",
                        outcome.cluster_stats.cluster_count,
                        outcome.cluster_stats.noise_ratio * 100.0
                    ),
                    StatusTone::Info,
                );
            }
            Err(err) => {
                self.set_status(format!("Similarity prep failed: {err}"), StatusTone::Error);
            }
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
    fn enqueue_embedding_backfill(&mut self, source: SampleSource) {
        let tx = self.runtime.jobs.message_sender();
        thread::spawn(move || {
            let result = analysis_jobs::enqueue_jobs_for_embedding_backfill(&source);
            match result {
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
            let result = run_similarity_finalize(&umap_version);
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

fn run_similarity_finalize(umap_version: &str) -> Result<jobs::SimilarityPrepOutcome, String> {
    let mut conn = open_library_db_for_similarity()?;
    crate::analysis::rebuild_ann_index(&conn)?;
    crate::analysis::umap::build_umap_layout(
        &mut conn,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
        umap_version,
        0,
        0.95,
    )?;
    let cluster_stats = crate::analysis::hdbscan::build_hdbscan_clusters(
        &mut conn,
        crate::analysis::embedding::EMBEDDING_MODEL_ID,
        HdbscanMethod::Umap,
        Some(umap_version),
        HdbscanConfig {
            min_cluster_size: DEFAULT_CLUSTER_MIN_SIZE,
            min_samples: None,
            allow_single_cluster: false,
        },
    )?;
    Ok(jobs::SimilarityPrepOutcome {
        cluster_stats,
        umap_version: umap_version.to_string(),
    })
}

fn open_library_db_for_similarity() -> Result<rusqlite::Connection, String> {
    let root = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    let path = root.join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
    rusqlite::Connection::open(path).map_err(|err| format!("Open library DB failed: {err}"))
}
