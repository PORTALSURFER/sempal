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
                            self.ui.progress.detail = detail;
                        }
                    }
                    ScanJobMessage::Finished(result) => {
                        self.runtime.jobs.clear_scan();
                        if self.ui.progress.task == Some(ProgressTaskKind::Scan) {
                            self.clear_progress();
                        }
                        if Some(&result.source_id)
                            != self.selection_state.ctx.selected_source.as_ref()
                        {
                            continue;
                        }
                        let label = match result.mode {
                            ScanMode::Quick => "Quick sync",
                            ScanMode::Hard => "Hard sync",
                        };
                        match result.result {
                            Ok(stats) => {
                                self.set_status(
                                    format!(
                                        "{label} complete: {} added, {} updated, {} missing",
                                        stats.added, stats.updated, stats.missing
                                    ),
                                    StatusTone::Info,
                                );
                                if let Some(source) = self.current_source() {
                                    let mut invalidator =
                                        source_cache_invalidator::SourceCacheInvalidator::new_from_state(
                                            &mut self.cache,
                                            &mut self.ui_cache,
                                            &mut self.library.missing,
                                        );
                                    invalidator.invalidate_wav_related(&source.id);
                                }
                                self.queue_wav_load();
                            }
                            Err(crate::sample_sources::scanner::ScanError::Canceled) => {
                                self.set_status(format!("{label} canceled"), StatusTone::Warning)
                            }
                            Err(err) => {
                                self.set_status(format!("{label} failed: {err}"), StatusTone::Error)
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
                JobMessage::UpdateChecked(message) => {
                    self.runtime.jobs.clear_update_check();
                    match message.result {
                        Ok(outcome) => self.apply_update_check_result(outcome),
                        Err(err) => self.apply_update_check_error(err),
                    }
                }
                JobMessage::GitHubIssueCreated(message) => {
                    self.runtime.jobs.clear_github_issue_create();
                    self.ui.feedback_issue.submitting = false;
                    match message.result {
                        Ok(issue) => {
                            self.ui.feedback_issue.open = false;
                            self.ui.feedback_issue.last_error = None;
                            self.ui.feedback_issue.draft.clear();
                            self.set_status(
                                format!("Created GitHub issue #{}", issue.number),
                                crate::egui_app::ui::style::StatusTone::Info,
                            );
                            if let Err(err) = open::that(&issue.html_url) {
                                self.set_status(
                                    format!("Issue created (could not open browser): {err}"),
                                    crate::egui_app::ui::style::StatusTone::Warning,
                                );
                            }
                        }
                        Err(err) => {
                            self.ui.feedback_issue.last_error = Some(err.to_string());
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
