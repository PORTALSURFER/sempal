use super::progress;
use super::*;
use crate::egui_app::state::ProgressTaskKind;

pub(super) fn handle_scan_progress(
    controller: &mut EguiController,
    completed: usize,
    detail: Option<String>,
) {
    let detail = match detail {
        Some(detail) if !detail.is_empty() => {
            format!("Scanned {completed} file(s)\n{detail}")
        }
        _ => format!("Scanned {completed} file(s)"),
    };
    progress::update_progress_detail(controller, ProgressTaskKind::Scan, completed, Some(detail));
}

pub(super) fn handle_scan_finished(controller: &mut EguiController, result: ScanResult) {
    controller.runtime.jobs.clear_scan();
    if controller.ui.progress.task == Some(ProgressTaskKind::Scan) {
        controller.clear_progress();
    }
    let is_selected_source =
        Some(&result.source_id) == controller.selection_state.ctx.selected_source.as_ref();
    let label = match result.mode {
        ScanMode::Quick => "Quick sync",
        ScanMode::Hard => "Hard sync",
    };
    match result.result {
        Ok(stats) => {
            let changed_samples = stats.changed_samples.clone();
            let scan_changed = !changed_samples.is_empty();
            let similarity_prep_active = controller
                .runtime
                .similarity_prep
                .as_ref()
                .is_some_and(|state| state.source_id == result.source_id);
            if is_selected_source {
                controller.set_status(
                    format!(
                        "{label} complete: {} added, {} updated, {} missing",
                        stats.added, stats.updated, stats.missing
                    ),
                    StatusTone::Info,
                );
            }

            {
                let mut invalidator =
                    source_cache_invalidator::SourceCacheInvalidator::new_from_state(
                        &mut controller.cache,
                        &mut controller.ui_cache,
                        &mut controller.library.missing,
                    );
                invalidator.invalidate_wav_related(&result.source_id);
            }

            if is_selected_source {
                controller.queue_wav_load();
            }

            let source_for_jobs = controller
                .library
                .sources
                .iter()
                .find(|source| source.id == result.source_id)
                .cloned();

            if scan_changed {
                if let Some(source) = source_for_jobs.clone() {
                    let tx = controller.runtime.jobs.message_sender();
                    let changed_samples = changed_samples.clone();
                    std::thread::spawn(move || {
                        let result = super::analysis_jobs::enqueue_jobs_for_source(
                            &source,
                            &changed_samples,
                        );
                        match result {
                            Ok((inserted, progress)) => {
                                let _ = tx.send(JobMessage::Analysis(
                                    super::AnalysisJobMessage::EnqueueFinished {
                                        inserted,
                                        progress,
                                    },
                                ));
                            }
                            Err(err) => {
                                let _ = tx.send(JobMessage::Analysis(
                                    super::AnalysisJobMessage::EnqueueFailed(err),
                                ));
                            }
                        }
                    });
                }
            } else if let Some(source) = source_for_jobs {
                if similarity_prep_active {
                    controller.handle_similarity_scan_finished(&result.source_id, false);
                    return;
                }
                let tx = controller.runtime.jobs.message_sender();
                std::thread::spawn(move || {
                    let result = super::analysis_jobs::enqueue_jobs_for_source_backfill(&source);
                    match result {
                        Ok((inserted, progress)) => {
                            let _ = tx.send(JobMessage::Analysis(
                                super::AnalysisJobMessage::EnqueueFinished { inserted, progress },
                            ));
                        }
                        Err(err) => {
                            let _ = tx.send(JobMessage::Analysis(
                                super::AnalysisJobMessage::EnqueueFailed(err),
                            ));
                        }
                    }
                    let embed_result =
                        super::analysis_jobs::enqueue_jobs_for_embedding_backfill(&source);
                    match embed_result {
                        Ok((inserted, progress)) => {
                            if inserted > 0 {
                                let _ = tx.send(JobMessage::Analysis(
                                    super::AnalysisJobMessage::EmbeddingBackfillEnqueueFinished {
                                        inserted,
                                        progress,
                                    },
                                ));
                            }
                        }
                        Err(err) => {
                            let _ = tx.send(JobMessage::Analysis(
                                super::AnalysisJobMessage::EmbeddingBackfillEnqueueFailed(err),
                            ));
                        }
                    }
                });
            }
            controller.handle_similarity_scan_finished(&result.source_id, scan_changed);
        }
        Err(crate::sample_sources::scanner::ScanError::Canceled) => {
            if is_selected_source {
                controller.set_status(format!("{label} canceled"), StatusTone::Warning);
            }
            controller.cancel_similarity_prep(&result.source_id);
        }
        Err(err) => {
            if is_selected_source {
                controller.set_status(format!("{label} failed: {err}"), StatusTone::Error);
            }
            controller.cancel_similarity_prep(&result.source_id);
        }
    }
}
