use radiant::prelude as ui;

use crate::native_app::app::{ClipboardHandoffTarget, GuiMessage, NativeAppState};
use crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage;

impl NativeAppState {
    pub(super) fn apply_browser_dispatch(
        &mut self,
        message: GuiMessage,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        match message {
            GuiMessage::BrowserScrollAccepted(surface) => {
                let _ = surface;
                self.ui.chrome.overflow_fades.arm();
                context.request_paint_only();
            }
            GuiMessage::ResizeFolder(message) => self.resize_folder_browser(message),
            GuiMessage::AddSourceDialogFinished(result) => {
                self.finish_add_source_dialog(result, context);
            }
            GuiMessage::FolderBrowser(message) => {
                if matches!(message, FolderBrowserMessage::SetCurationScope(_, _)) {
                    self.ui.chrome.curation_filter_dropdown_open = false;
                }
                if matches!(message, FolderBrowserMessage::SetHarvestFilter(_, _)) {
                    self.ui.chrome.harvest_filter_dropdown_open = false;
                }
                self.apply_folder_browser_message(message, context);
            }
            GuiMessage::SetSimilarityAspectWeightingEnabled(enabled) => {
                self.set_similarity_aspect_weighting_enabled(enabled);
            }
            GuiMessage::SetSimilarityAspectEnabled { aspect, enabled } => {
                self.set_similarity_aspect_enabled(aspect, enabled);
            }
            GuiMessage::SetSimilarityAspectWeight { aspect, weight } => {
                self.set_similarity_aspect_weight(aspect, weight);
            }
            GuiMessage::SimilaritySettingsPersisted(result) => {
                self.finish_similarity_settings_persist(result);
            }
            GuiMessage::StarmapLayoutLoaded(result) => {
                self.finish_starmap_layout_load(result);
            }
            GuiMessage::SimilarityScoresResolved(result) => {
                self.finish_similarity_scores(result, context);
            }
            GuiMessage::SimilarityReadinessAdvanced {
                source_id,
                lifecycle_generation,
            } => {
                if self.background.source_lifecycle_generations.get(&source_id)
                    != Some(&lifecycle_generation)
                {
                    return;
                }
                self.finish_similarity_readiness_advanced(source_id, context);
            }
            GuiMessage::SourceProcessingHealth(health) => {
                if !self.library.folder_browser.source_exists(&health.source_id)
                    || self
                        .background
                        .source_lifecycle_generations
                        .get(&health.source_id)
                        != Some(&health.lifecycle_generation)
                {
                    return;
                }
                self.background
                    .source_processing_health
                    .insert(health.source_id.clone(), health);
                context.repaint(ui::RepaintScope::Projection);
            }
            GuiMessage::SourceProcessingProgress(progress) => {
                if matches!(
                    progress.presentation,
                    crate::native_app::source_processing::SourceProcessingPresentation::RoutineMaintenance
                ) {
                    return;
                }
                let source_is_current = if progress.source_id.is_empty() {
                    !progress.active
                        || !self
                            .library
                            .folder_browser
                            .configured_sample_sources()
                            .is_empty()
                } else {
                    self.library
                        .folder_browser
                        .source_exists(&progress.source_id)
                        && self
                            .background
                            .source_lifecycle_generations
                            .get(&progress.source_id)
                            == Some(&progress.lifecycle_generation)
                };
                if !source_is_current {
                    return;
                }
                let worker_progress_visible_before = self.worker_progress_indicator_visible();
                if !progress.active {
                    self.background.source_processing_progress = None;
                    self.ui.chrome.job_details_open = false;
                } else {
                    self.background.source_processing_progress = Some(progress);
                }
                context.repaint(if self.starmap_retained_scene_active() {
                    if worker_progress_visible_before != self.worker_progress_indicator_visible() {
                        ui::RepaintScope::Surface
                    } else {
                        ui::RepaintScope::PaintOnly
                    }
                } else {
                    ui::RepaintScope::Projection
                });
            }
            GuiMessage::FolderScanProgress(progress) => {
                self.apply_folder_scan_progress(progress);
            }
            GuiMessage::FolderScanDiscoveryBatch(batch) => {
                self.apply_folder_scan_discovery_batch(batch);
            }
            GuiMessage::FolderScanFinished(result) => self.finish_folder_scan(result, context),
            GuiMessage::FolderScanMaintenanceFinished(result) => {
                self.finish_folder_scan_maintenance(result, context)
            }
            GuiMessage::FolderTreeRefreshFinished(completion) => {
                self.finish_folder_tree_refresh(completion, context);
            }
            GuiMessage::SelectedFolderVerifyFinished(ticket) => {
                self.finish_folder_verify(ticket, context)
            }
            GuiMessage::SourceFilesystemChanged {
                source_id,
                paths,
                overflowed,
                source_root_available,
                journal_checkpoint_event_id,
                watcher_continuity_proof,
            } => {
                self.refresh_source_after_filesystem_change(
                    source_id,
                    paths,
                    overflowed,
                    source_root_available,
                    journal_checkpoint_event_id,
                    watcher_continuity_proof,
                    context,
                );
            }
            GuiMessage::SourceWatcherReady {
                deferred_audit_sources,
            } => {
                self.background
                    .source_processing
                    .request_lifecycle_audit_probe(
                    crate::native_app::source_processing::SourceAuditLifecycleCause::WatcherReady,
                    &deferred_audit_sources,
                );
            }
            GuiMessage::SourceWatcherJournalGap { source_id, reason } => {
                self.background
                    .source_processing
                    .request_source_manifest_audit(&source_id, reason);
            }
            GuiMessage::SourceWatcherCheckpointReady(request) => {
                if self
                    .library
                    .folder_browser
                    .source_exists(&request.source_id)
                    && self
                        .background
                        .source_lifecycle_generations
                        .get(&request.source_id)
                        == Some(&request.lifecycle_generation)
                {
                    self.background
                        .source_processing
                        .budget_handle()
                        .submit_watcher_checkpoint(request);
                }
            }
            GuiMessage::SourceFilesystemSyncFinished(result) => {
                self.finish_source_filesystem_sync(result, context);
            }
            GuiMessage::SourceManifestAuditCommitted {
                source_id,
                lifecycle_generation,
                committed_delta,
                complete,
            } => {
                self.finish_source_manifest_audit(
                    source_id,
                    lifecycle_generation,
                    committed_delta,
                    complete,
                    context,
                );
            }
            GuiMessage::SourceManifestAuditFinished {
                source_id,
                lifecycle_generation,
                source_revision,
                complete,
            } => {
                let source_is_current = self.library.folder_browser.source_exists(&source_id)
                    && self.background.source_lifecycle_generations.get(&source_id)
                        == Some(&lifecycle_generation);
                if source_is_current {
                    if let Some(watcher) = self.library.source_watcher.as_ref() {
                        watcher.finish_journal_barrier_audit(
                            source_id,
                            lifecycle_generation,
                            source_revision,
                            complete,
                        );
                    }
                }
            }
            GuiMessage::NormalizationProgress(progress) => {
                self.apply_normalization_progress(progress);
            }
            GuiMessage::NormalizationFinished(result) => self.finish_normalization(result, context),
            GuiMessage::SelectSampleWithModifiers { path, modifiers } => {
                self.ui.browser_interaction.clipboard_handoff_target =
                    ClipboardHandoffTarget::BrowserFiles;
                self.ui.browser_interaction.context_menu = None;
                self.select_sample_with_modifiers(path, modifiers, context);
            }
            GuiMessage::OpenSampleContextMenu { path, position } => {
                self.ui.browser_interaction.clipboard_handoff_target =
                    ClipboardHandoffTarget::BrowserFiles;
                self.open_sample_context_menu(path, position);
            }
            GuiMessage::DragSampleFile { path, drag } => {
                self.ui.browser_interaction.clipboard_handoff_target =
                    ClipboardHandoffTarget::BrowserFiles;
                self.ui.browser_interaction.context_menu = None;
                self.drag_sample_file(path, drag, context);
            }
            GuiMessage::ExternalDragCompleted(result) => {
                self.external_drag_completed(result, context)
            }
            _ => unreachable!("browser dispatcher received a non-browser message"),
        }
    }
}
