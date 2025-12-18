use super::interaction_options::{clamp_scroll_speed, clamp_zoom_factor};
use super::*;

impl EguiController {
    /// Load persisted configuration and populate initial UI state.
    pub fn load_configuration(&mut self) -> Result<(), crate::sample_sources::config::ConfigError> {
        let cfg = crate::sample_sources::config::load_or_default()?;
        self.settings.feature_flags = cfg.feature_flags;
        self.settings.model = cfg.model;
        self.settings.analysis = cfg.analysis;
        self.settings.analysis.max_analysis_duration_seconds =
            super::analysis_options::clamp_max_analysis_duration_seconds(
                self.settings.analysis.max_analysis_duration_seconds,
            );
        self.settings.updates = cfg.updates.clone();
        self.settings.trash_folder = cfg.trash_folder.clone();
        self.settings.collection_export_root = cfg.collection_export_root.clone();
        self.ui.collections.enabled = self.settings.feature_flags.collections_enabled;
        self.settings.audio_output = cfg.audio_output.clone();
        self.ui.audio.selected = self.settings.audio_output.clone();
        self.settings.controls = cfg.controls.clone();
        self.settings.controls.waveform_scroll_speed =
            clamp_scroll_speed(self.settings.controls.waveform_scroll_speed);
        self.settings.controls.wheel_zoom_factor =
            clamp_zoom_factor(self.settings.controls.wheel_zoom_factor);
        self.settings.controls.keyboard_zoom_factor =
            clamp_zoom_factor(self.settings.controls.keyboard_zoom_factor);
        self.ui.controls = crate::egui_app::state::InteractionOptionsState {
            invert_waveform_scroll: self.settings.controls.invert_waveform_scroll,
            waveform_scroll_speed: self.settings.controls.waveform_scroll_speed,
            wheel_zoom_factor: self.settings.controls.wheel_zoom_factor,
            keyboard_zoom_factor: self.settings.controls.keyboard_zoom_factor,
            destructive_yolo_mode: self.settings.controls.destructive_yolo_mode,
            waveform_channel_view: self.settings.controls.waveform_channel_view,
        };
        self.ui.waveform.channel_view = self.settings.controls.waveform_channel_view;
        self.refresh_audio_options();
        self.apply_volume(cfg.volume);
        self.ui.trash_folder = cfg.trash_folder.clone();
        self.ui.collection_export_root = cfg.collection_export_root.clone();
        self.ui.update.last_seen_nightly_published_at =
            cfg.updates.last_seen_nightly_published_at.clone();
        self.library.sources = cfg.sources.clone();
        self.rebuild_missing_sources();
        if !self.library.missing.sources.is_empty() {
            let count = self.library.missing.sources.len();
            let suffix = if count == 1 { "" } else { "s" };
            self.set_status(
                format!("{count} source{suffix} unavailable"),
                StatusTone::Warning,
            );
        }
        self.library.collections = cfg.collections;
        // Backfill clip roots for legacy collection-owned clips that were not persisted.
        for collection in self.library.collections.iter_mut() {
            let expected_source_prefix = format!("collection-{}", collection.id.as_str());
            let resolved_root =
                crate::egui_app::controller::collection_export::resolved_export_dir(
                    collection,
                    self.settings.collection_export_root.as_deref(),
                )
                .or_else(|| {
                    crate::app_dirs::app_root_dir()
                        .ok()
                        .map(|root| root.join("collection_clips").join(collection.id.as_str()))
                });
            if let Some(root) = resolved_root {
                for member in collection.members.iter_mut() {
                    if member.clip_root.is_none()
                        && member.source_id.as_str() == expected_source_prefix
                    {
                        member.clip_root = Some(root.clone());
                    }
                }
            }
        }
        self.selection_state.ctx.selected_source = cfg
            .last_selected_source
            .filter(|id| self.library.sources.iter().any(|s| &s.id == id));
        self.selection_state.ctx.last_selected_browsable_source =
            self.selection_state.ctx.selected_source.clone();
        self.ensure_collection_selection();
        self.refresh_sources_ui();
        self.refresh_collections_ui();
        if self.selection_state.ctx.selected_source.is_some() {
            let _ = self.refresh_wavs();
        }
        self.maybe_check_for_updates_on_startup();
        self.runtime
            .analysis
            .set_unknown_confidence_threshold(self.settings.model.unknown_confidence_threshold);
        self.runtime
            .analysis
            .set_max_analysis_duration_seconds(self.settings.analysis.max_analysis_duration_seconds);
        self.runtime
            .analysis
            .start(self.runtime.jobs.message_sender());
        {
            let tx = self.runtime.jobs.message_sender();
            std::thread::spawn(move || {
                let result = super::analysis_jobs::enqueue_inference_jobs_for_all_sources();
                match result {
                    Ok((inserted, progress)) => {
                        if inserted > 0 {
                            let _ = tx.send(super::jobs::JobMessage::Analysis(
                                super::analysis_jobs::AnalysisJobMessage::EnqueueFinished {
                                    inserted,
                                    progress,
                                },
                            ));
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(super::jobs::JobMessage::Analysis(
                            super::analysis_jobs::AnalysisJobMessage::EnqueueFailed(err),
                        ));
                    }
                }
            });
        }
        Ok(())
    }

    pub(super) fn persist_config(&mut self, error_prefix: &str) -> Result<(), String> {
        self.save_full_config()
            .map_err(|err| format!("{error_prefix}: {err}"))
    }

    pub(super) fn save_full_config(
        &self,
    ) -> Result<(), crate::sample_sources::config::ConfigError> {
        crate::sample_sources::config::save(&crate::sample_sources::config::AppConfig {
            sources: self.library.sources.clone(),
            collections: self.library.collections.clone(),
            feature_flags: self.settings.feature_flags.clone(),
            model: self.settings.model.clone(),
            analysis: self.settings.analysis.clone(),
            updates: self.settings.updates.clone(),
            trash_folder: self.settings.trash_folder.clone(),
            collection_export_root: self.settings.collection_export_root.clone(),
            last_selected_source: self
                .selection_state
                .ctx
                .selected_source
                .clone()
                .filter(|id| self.library.sources.iter().any(|s| &s.id == id))
                .or_else(|| {
                    self.selection_state
                        .ctx
                        .last_selected_browsable_source
                        .clone()
                }),
            audio_output: self.settings.audio_output.clone(),
            volume: self.ui.volume,
            controls: self.settings.controls.clone(),
        })
    }

    /// Open the `.sempal` config directory in the OS file explorer.
    pub fn open_config_folder(&mut self) {
        match crate::app_dirs::app_root_dir() {
            Ok(path) => {
                if let Err(err) = open::that(&path) {
                    self.set_status(
                        format!("Could not open config folder {}: {err}", path.display()),
                        StatusTone::Error,
                    );
                }
            }
            Err(err) => {
                self.set_status(
                    format!("Could not resolve config folder: {err}"),
                    StatusTone::Error,
                );
            }
        }
    }
}
