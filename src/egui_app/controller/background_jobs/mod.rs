mod analysis;
mod progress;
mod scan;
mod similarity;
mod updates;

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
                            self.apply_wav_entries(
                                entries,
                                message.total,
                                self.wav_entries.page_size,
                                message.page_index,
                                false,
                                Some(message.source_id.clone()),
                                Some(message.elapsed),
                            );
                            self.cache.wav.insert_page(
                                message.source_id.clone(),
                                message.total,
                                self.wav_entries.page_size,
                                message.page_index,
                                self.wav_entries
                                    .pages
                                    .get(&message.page_index)
                                    .cloned()
                                    .unwrap_or_default(),
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
                        scan::handle_scan_progress(self, completed, detail);
                    }
                    ScanJobMessage::Finished(result) => {
                        scan::handle_scan_finished(self, result);
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
                JobMessage::Analysis(message) => {
                    analysis::handle_analysis_message(self, message);
                }
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
                JobMessage::UmapClustersBuilt(message) => {
                    self.runtime.jobs.clear_umap_cluster_build();
                    match message.result {
                        Ok(stats) => {
                            self.ui.map.last_query = None;
                            self.ui.map.cached_cluster_centroids_key = None;
                            self.ui.map.cached_cluster_centroids = None;
                            self.ui.map.auto_cluster_build_requested_key = None;
                            let scope = message
                                .source_id
                                .as_ref()
                                .map(|id| id.as_str())
                                .unwrap_or("all sources");
                            self.set_status(
                                format!(
                                    "Clusters built for {scope} ({} clusters, {:.1}% noise)",
                                    stats.cluster_count,
                                    stats.noise_ratio * 100.0
                                ),
                                StatusTone::Info,
                            );
                        }
                        Err(err) => {
                            self.set_status(
                                format!("Cluster build failed: {err}"),
                                StatusTone::Error,
                            );
                        }
                    }
                }
                JobMessage::SimilarityPrepared(message) => {
                    similarity::handle_similarity_prepared(self, message);
                }
                JobMessage::UpdateChecked(message) => {
                    updates::handle_update_checked(self, message);
                }
                JobMessage::IssueGatewayCreated(message) => {
                    updates::handle_issue_gateway_created(self, message);
                }
                JobMessage::IssueGatewayAuthed(message) => {
                    updates::handle_issue_gateway_authed(self, message);
                }
            }
        }
    }
}
