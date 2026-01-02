use super::store::{DbSimilarityPrepStore, SimilarityPrepStore};
use super::DEFAULT_CLUSTER_MIN_SIZE;
use crate::analysis::hdbscan::{HdbscanConfig, HdbscanMethod};
use crate::egui_app::controller::{analysis_jobs, jobs, EguiController};
use crate::sample_sources::{SampleSource, SourceId};
use std::thread;

impl EguiController {
    pub(super) fn apply_similarity_prep_duration_cap(&mut self) {
        let max_duration = if self.similarity_prep_duration_cap_enabled() {
            self.settings.analysis.max_analysis_duration_seconds
        } else {
            0.0
        };
        self.runtime
            .analysis
            .set_max_analysis_duration_seconds(max_duration);
    }

    pub(super) fn restore_similarity_prep_duration_cap(&mut self) {
        self.runtime
            .analysis
            .set_max_analysis_duration_seconds(self.settings.analysis.max_analysis_duration_seconds);
    }

    pub(super) fn apply_similarity_prep_fast_mode(&mut self) {
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

    pub(super) fn restore_similarity_prep_fast_mode(&mut self) {
        self.runtime
            .analysis
            .set_analysis_sample_rate(crate::analysis::audio::ANALYSIS_SAMPLE_RATE);
        self.runtime.analysis.set_analysis_version_override(None);
    }

    pub(super) fn apply_similarity_prep_full_analysis(&mut self, force_full_analysis: bool) {
        if !force_full_analysis {
            return;
        }
        self.runtime.analysis.set_analysis_cache_enabled(false);
    }

    pub(super) fn restore_similarity_prep_full_analysis(&mut self) {
        self.runtime.analysis.set_analysis_cache_enabled(true);
    }

    pub(super) fn apply_similarity_prep_worker_boost(&mut self) {
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

    pub(super) fn restore_similarity_prep_worker_count(&mut self) {
        self.runtime.performance.idle_worker_override = None;
        self.runtime
            .analysis
            .set_worker_count(self.settings.analysis.analysis_worker_count);
    }

    pub(super) fn enqueue_similarity_backfill(
        &mut self,
        source: SampleSource,
        force_full_analysis: bool,
    ) {
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

    pub(super) fn start_similarity_finalize(&mut self, source_id: SourceId, umap_version: String) {
        let tx = self.runtime.jobs.message_sender();
        thread::spawn(move || {
            let started_at = std::time::Instant::now();
            let result = std::panic::catch_unwind(|| run_similarity_finalize(&source_id, &umap_version))
                .unwrap_or_else(|_| {
                    Err("Similarity finalize panicked".to_string())
                });
            tracing::info!(
                "Similarity finalize finished in {:.2?} (source_id={})",
                started_at.elapsed(),
                source_id.as_str()
            );
            let _ = tx.send(jobs::JobMessage::SimilarityPrepared(
                jobs::SimilarityPrepResult { source_id, result },
            ));
        });
    }

    pub(super) fn find_source_by_id(&self, source_id: &SourceId) -> Option<SampleSource> {
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
