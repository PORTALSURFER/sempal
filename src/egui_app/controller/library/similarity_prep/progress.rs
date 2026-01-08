use super::store::{DbSimilarityPrepStore, SimilarityPrepStore};
use crate::egui_app::controller::{EguiController, SimilarityPrepStage};
use crate::egui_app::state::AnalysisProgressSnapshot;
use tracing::info;

impl EguiController {
    pub(super) fn refresh_similarity_prep_progress(&mut self) {
        let Some(state) = self.runtime.similarity_prep.as_ref() else {
            return;
        };
        info!(
            "Similarity prep progress refresh (stage={:?}, source_id={})",
            state.stage,
            state.source_id.as_str()
        );
        let store = DbSimilarityPrepStore;
        match state.stage {
            SimilarityPrepStage::AwaitScan => {
                if self.runtime.jobs.scan_in_progress() {
                    info!(
                        "Similarity prep waiting for scan (source_id={})",
                        state.source_id.as_str()
                    );
                    self.ensure_similarity_prep_progress(0, false);
                    self.set_similarity_scan_detail();
                    return;
                }
                info!(
                    "Similarity prep requesting hard sync (source_id={})",
                    state.source_id.as_str()
                );
                self.request_hard_sync();
                return;
            }
            SimilarityPrepStage::AwaitEmbeddings => {
                let Some(source) = self.find_source_by_id(&state.source_id) else {
                    info!(
                        "Similarity prep missing source (source_id={})",
                        state.source_id.as_str()
                    );
                    return;
                };
                let progress = match store.current_analysis_progress(&source) {
                    Ok(progress) => progress,
                    Err(_) => {
                        info!(
                            "Similarity prep analysis progress unavailable (source_id={})",
                            source.id.as_str()
                        );
                        if !self.ui.progress.visible {
                            self.show_similarity_prep_progress(0, false);
                            self.set_similarity_analysis_detail();
                        }
                        return;
                    }
                };
                info!(
                    "Similarity prep analysis progress (pending={}, running={}, failed={}, completed={}, total={}, source_id={})",
                    progress.pending,
                    progress.running,
                    progress.failed,
                    progress.completed(),
                    progress.total(),
                    source.id.as_str()
                );
                if progress.pending == 0 && progress.running == 0 {
                    let embed_progress = store
                        .current_embedding_backfill_progress(&source)
                        .unwrap_or_default();
                    info!(
                        "Similarity prep embedding progress (pending={}, running={}, failed={}, completed={}, total={}, source_id={})",
                        embed_progress.pending,
                        embed_progress.running,
                        embed_progress.failed,
                        embed_progress.completed(),
                        embed_progress.total(),
                        source.id.as_str()
                    );
                    if embed_progress.pending > 0 || embed_progress.running > 0 {
                        self.ensure_similarity_prep_progress(embed_progress.total(), true);
                        self.set_similarity_embedding_detail();
                        self.ui
                            .progress
                            .set_counts(embed_progress.total(), embed_progress.completed());
                        let jobs_completed = embed_progress.completed();
                        let jobs_total = embed_progress.total();
                        let mut detail =
                            format!("Embedding backfill… Jobs {jobs_completed}/{jobs_total}");
                        if embed_progress.failed > 0 {
                            detail.push_str(&format!(" • {} failed", embed_progress.failed));
                        }
                        self.ui.progress.set_detail(Some(detail));
                        self.ui.progress.set_analysis_snapshot(None);
                        return;
                    }
                    if !store.source_has_embeddings(&source) {
                        info!(
                            "Similarity prep enqueueing embedding backfill (source_id={})",
                            source.id.as_str()
                        );
                        self.ensure_similarity_prep_progress(0, true);
                        self.set_similarity_embedding_detail();
                        self.enqueue_similarity_backfill(source, false);
                        return;
                    }
                    self.handle_similarity_analysis_progress(&progress);
                    return;
                }
                self.ensure_similarity_prep_progress(progress.total(), true);
                self.ui
                    .progress
                    .set_counts(progress.total(), progress.completed());
                let jobs_completed = progress.completed();
                let jobs_total = progress.total();
                let samples_completed = progress.samples_completed();
                let samples_total = progress.samples_total;
                let mut detail = format!(
                    "Analyzing audio features… Jobs {jobs_completed}/{jobs_total} • Samples {samples_completed}/{samples_total}"
                );
                if progress.running == 0 && progress.pending > 0 {
                    detail.push_str(" • Waiting for workers");
                }
                if progress.failed > 0 {
                    detail.push_str(&format!(" • {} failed", progress.failed));
                }
                self.ui.progress.set_detail(Some(detail));
                self.ui
                    .progress
                    .set_analysis_snapshot(Some(AnalysisProgressSnapshot {
                        pending: progress.pending,
                        running: progress.running,
                        failed: progress.failed,
                        samples_completed,
                        samples_total,
                        running_jobs: Vec::new(),
                        stale_after_secs: Some(
                            crate::egui_app::controller::analysis_jobs::stale_running_job_seconds(),
                        ),
                    }));
            }
            SimilarityPrepStage::Finalizing => {
                info!(
                    "Similarity prep finalizing (source_id={})",
                    state.source_id.as_str()
                );
                self.ensure_similarity_finalize_progress();
                self.set_similarity_finalize_detail();
            }
        }
    }
}
