use super::jobs::JobMessage;
use super::trash_move::TrashMoveMessage;
use super::*;
use crate::egui_app::state::ProgressTaskKind;
use std::sync::atomic::Ordering;

impl EguiController {
    pub(in crate::egui_app::controller) fn poll_background_jobs(&mut self) {
        if self.ui.progress.cancel_requested {
            match self.ui.progress.task {
                Some(ProgressTaskKind::TrashMove) => {
                    if let Some(cancel) = self.runtime.jobs.trash_move_cancel().as_ref() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
                Some(ProgressTaskKind::Scan) => {
                    if let Some(cancel) = self.runtime.jobs.scan_cancel().as_ref() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
                Some(ProgressTaskKind::Analysis) => {
                    self.runtime.analysis.cancel();
                    self.clear_progress();
                }
                _ => {}
            }
        }

        loop {
            let message = match self.runtime.jobs.try_recv_message() {
                Ok(message) => message,
                Err(
                    std::sync::mpsc::TryRecvError::Empty
                    | std::sync::mpsc::TryRecvError::Disconnected,
                ) => {
                    break;
                }
            };

            match message {
                JobMessage::WavLoaded(message) => {
                    if Some(&message.source_id) != self.selection_state.ctx.selected_source.as_ref()
                    {
                        continue;
                    }
                    match message.result {
                        Ok(entries) => {
                            self.cache
                                .wav
                                .entries
                                .insert(message.source_id.clone(), entries.clone());
                            self.rebuild_wav_cache_lookup(&message.source_id);
                            self.apply_wav_entries(
                                entries,
                                false,
                                Some(message.source_id.clone()),
                                Some(message.elapsed),
                            );
                        }
                        Err(err) => self.handle_wav_load_error(&message.source_id, err),
                    }
                    self.runtime.jobs.clear_wav_load_pending();
                    if self.ui.progress.task == Some(ProgressTaskKind::WavLoad) {
                        self.clear_progress();
                    }
                }
                JobMessage::AudioLoaded(message) => {
                    let Some(pending) = self.runtime.jobs.pending_audio() else {
                        continue;
                    };
                    if message.request_id != pending.request_id
                        || message.source_id != pending.source_id
                        || message.relative_path != pending.relative_path
                    {
                        continue;
                    }
                    self.runtime.jobs.set_pending_audio(None);
                    self.ui.waveform.loading = None;
                    match message.result {
                        Ok(outcome) => self.handle_audio_loaded(pending, outcome),
                        Err(err) => self.handle_audio_load_error(pending, err),
                    }
                }
                JobMessage::Scan(message) => match message {
                    ScanJobMessage::Progress { completed, detail } => {
                        if self.ui.progress.task == Some(ProgressTaskKind::Scan) {
                            self.ui.progress.completed = completed;
                            self.ui.progress.detail = Some(match detail {
                                Some(detail) if !detail.is_empty() => {
                                    format!("Scanned {completed} file(s)\n{detail}")
                                }
                                _ => format!("Scanned {completed} file(s)"),
                            });
                        }
                    }
                    ScanJobMessage::Finished(result) => {
                        self.runtime.jobs.clear_scan();
                        if self.ui.progress.task == Some(ProgressTaskKind::Scan) {
                            self.clear_progress();
                        }
                        let is_selected_source = Some(&result.source_id)
                            == self.selection_state.ctx.selected_source.as_ref();
                        let label = match result.mode {
                            ScanMode::Quick => "Quick sync",
                            ScanMode::Hard => "Hard sync",
                        };
                        match result.result {
                            Ok(stats) => {
                                let changed_samples = stats.changed_samples.clone();
                                if is_selected_source {
                                    self.set_status(
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
                                            &mut self.cache,
                                            &mut self.ui_cache,
                                            &mut self.library.missing,
                                        );
                                    invalidator.invalidate_wav_related(&result.source_id);
                                }

                                if is_selected_source {
                                    self.queue_wav_load();
                                }

                                let source_for_jobs = self
                                    .library
                                    .sources
                                    .iter()
                                    .find(|source| source.id == result.source_id)
                                    .cloned();

                                if !changed_samples.is_empty() {
                                    let tx = self.runtime.jobs.message_sender();
                                    let source_id = result.source_id.clone();
                                    std::thread::spawn(move || {
                                        let result = super::analysis_jobs::enqueue_jobs_for_source(
                                            &source_id,
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
                                } else if let Some(source) = source_for_jobs {
                                    let tx = self.runtime.jobs.message_sender();
                                    std::thread::spawn(move || {
                                        let result =
                                            super::analysis_jobs::enqueue_jobs_for_source_backfill(
                                                &source,
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
                                        let embed_result =
                                            super::analysis_jobs::enqueue_jobs_for_embedding_backfill(
                                                &source,
                                            );
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
                                self.handle_similarity_scan_finished(&result.source_id);
                            }
                            Err(crate::sample_sources::scanner::ScanError::Canceled) => {
                                if is_selected_source {
                                    self.set_status(
                                        format!("{label} canceled"),
                                        StatusTone::Warning,
                                    );
                                }
                            }
                            Err(err) => {
                                if is_selected_source {
                                    self.set_status(
                                        format!("{label} failed: {err}"),
                                        StatusTone::Error,
                                    );
                                }
                            }
                        }
                    }
                },
                JobMessage::TrashMove(message) => match message {
                    TrashMoveMessage::SetTotal(total) => {
                        self.ui.progress.total = total;
                    }
                    TrashMoveMessage::Progress { completed, detail } => {
                        self.ui.progress.completed = completed;
                        self.ui.progress.detail = detail;
                    }
                    TrashMoveMessage::Finished(result) => {
                        self.runtime.jobs.clear_trash_move();
                        self.apply_trash_move_finished(result);
                    }
                },
                JobMessage::Analysis(message) => match message {
                    super::AnalysisJobMessage::Progress(progress) => {
                        self.handle_similarity_analysis_progress(&progress);
                        if progress.total() == 0 {
                            if self.ui.progress.task == Some(ProgressTaskKind::Analysis) {
                                self.clear_progress();
                            }
                            continue;
                        }
                        if progress.pending == 0 && progress.running == 0 {
                            if let Some(source_id) =
                                self.selection_state.ctx.selected_source.clone()
                                && let Ok(failures) =
                                    super::analysis_jobs::failed_samples_for_source(&source_id)
                            {
                                self.ui_cache
                                    .browser
                                    .analysis_failures
                                    .insert(source_id, failures);
                            }
                            if let Some(source_id) =
                                self.selection_state.ctx.selected_source.clone()
                            {
                                self.ui_cache.browser.features.remove(&source_id);
                            }
                            if self.ui.progress.task == Some(ProgressTaskKind::Analysis) {
                                self.clear_progress();
                            }
                            continue;
                        }
                        if self.ui.progress.task.is_none()
                            || self.ui.progress.task == Some(ProgressTaskKind::Analysis)
                        {
                            if !self.ui.progress.visible
                                || self.ui.progress.task != Some(ProgressTaskKind::Analysis)
                            {
                                self.show_status_progress(
                                    ProgressTaskKind::Analysis,
                                    "Analyzing samples",
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
                    }
                    super::AnalysisJobMessage::EnqueueFinished { inserted, progress } => {
                        self.runtime.analysis.resume();
                        if inserted > 0 {
                            self.set_status(
                                format!("Queued {inserted} analysis jobs"),
                                StatusTone::Info,
                            );
                        }
                        if let Some(source_id) = self.selection_state.ctx.selected_source.clone() {
                            self.ui_cache.browser.features.remove(&source_id);
                        }
                        let _ = self
                            .runtime
                            .jobs
                            .message_sender()
                            .send(JobMessage::Analysis(super::AnalysisJobMessage::Progress(
                                progress,
                            )));
                    }
                    super::AnalysisJobMessage::EnqueueFailed(err) => {
                        self.set_status(
                            format!("Analysis enqueue failed: {err}"),
                            StatusTone::Error,
                        );
                    }
                    super::AnalysisJobMessage::EmbeddingBackfillEnqueueFinished {
                        inserted,
                        progress,
                    } => {
                        self.runtime.analysis.resume();
                        if inserted > 0 {
                            self.set_status(
                                format!("Queued {inserted} embedding backfill jobs"),
                                StatusTone::Info,
                            );
                        }
                        let _ = self
                            .runtime
                            .jobs
                            .message_sender()
                            .send(JobMessage::Analysis(super::AnalysisJobMessage::Progress(
                                progress,
                            )));
                    }
                    super::AnalysisJobMessage::EmbeddingBackfillEnqueueFailed(err) => {
                        self.set_status(
                            format!("Embedding backfill enqueue failed: {err}"),
                            StatusTone::Error,
                        );
                    }
                },
                JobMessage::UmapBuilt(message) => {
                    self.runtime.jobs.clear_umap_build();
                    match message.result {
                        Ok(()) => {
                            self.ui.map.bounds = None;
                            self.ui.map.last_query = None;
                            self.set_status(
                                format!("t-SNE layout {} built", message.umap_version),
                                StatusTone::Info,
                            );
                        }
                        Err(err) => {
                            self.set_status(
                                format!("t-SNE build failed: {err}"),
                                StatusTone::Error,
                            );
                        }
                    }
                }
                JobMessage::SimilarityPrepared(message) => {
                    self.handle_similarity_prep_result(message);
                }
                JobMessage::UpdateChecked(message) => {
                    self.runtime.jobs.clear_update_check();
                    match message.result {
                        Ok(outcome) => self.apply_update_check_result(outcome),
                        Err(err) => self.apply_update_check_error(err),
                    }
                }
                JobMessage::IssueGatewayCreated(message) => {
                    self.runtime.jobs.clear_issue_gateway_create();
                    self.ui.feedback_issue.submitting = false;
                    match message.result {
                        Ok(outcome) => {
                            if outcome.ok {
                                self.ui.feedback_issue.last_error = None;
                                self.ui.feedback_issue.last_success_url =
                                    Some(outcome.issue_url.clone());
                                self.set_status(
                                    format!("Created GitHub issue #{}", outcome.number),
                                    crate::egui_app::ui::style::StatusTone::Info,
                                );
                            } else {
                                self.ui.feedback_issue.last_error =
                                    Some("Issue creation failed.".to_string());
                                self.set_status(
                                    "Failed to create issue".to_string(),
                                    crate::egui_app::ui::style::StatusTone::Error,
                                );
                            }
                        }
                        Err(err) => {
                            if matches!(
                                err,
                                crate::issue_gateway::api::CreateIssueError::Unauthorized
                            ) {
                                if let Ok(store) = crate::issue_gateway::IssueTokenStore::new() {
                                    let _ = store.delete();
                                }
                                self.ui.feedback_issue.token_modal_open = true;
                                self.ui.feedback_issue.focus_token_requested = true;
                                self.ui.feedback_issue.last_error = Some(
                                    "GitHub connection expired. Reconnect and paste a new token."
                                        .to_string(),
                                );
                            } else {
                                self.ui.feedback_issue.last_error = Some(err.to_string());
                            }
                            self.set_status(
                                format!("Failed to create issue: {err}"),
                                crate::egui_app::ui::style::StatusTone::Error,
                            );
                        }
                    }
                }
            }
        }
    }
}
