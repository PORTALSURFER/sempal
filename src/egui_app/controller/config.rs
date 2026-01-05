use super::interaction_options::{clamp_scroll_speed, clamp_zoom_factor};
use super::*;

impl EguiController {
    /// Load persisted configuration and populate initial UI state.
    pub fn load_configuration(&mut self) -> Result<(), crate::sample_sources::config::ConfigError> {
        let cfg = crate::sample_sources::config::load_or_default()?;
        if let Err(err) = crate::model_setup::sync_bundled_burnpack() {
            self.set_status(
                format!("Bundled model sync failed: {err}"),
                StatusTone::Warning,
            );
        }
        self.settings.feature_flags = cfg.core.feature_flags;
        self.settings.analysis = cfg.core.analysis;
        self.settings.analysis.max_analysis_duration_seconds =
            super::analysis_options::clamp_max_analysis_duration_seconds(
                self.settings.analysis.max_analysis_duration_seconds,
            );
        self.settings.updates = cfg.core.updates.clone();
        self.settings.hints = cfg.core.hints.clone();
        self.settings.app_data_dir = cfg.core.app_data_dir.clone();
        self.settings.trash_folder = cfg.core.trash_folder.clone();
        self.settings.collection_export_root = cfg.core.collection_export_root.clone();
        self.ui.collections.enabled = self.settings.feature_flags.collections_enabled;
        self.settings.audio_output = cfg.core.audio_output.clone();
        self.ui.audio.selected = self.settings.audio_output.clone();
        self.settings.audio_input = cfg.core.audio_input.clone();
        self.ui.audio.input_selected = self.settings.audio_input.clone();
        self.settings.controls = cfg.core.controls.clone();
        self.settings.controls.waveform_scroll_speed =
            clamp_scroll_speed(self.settings.controls.waveform_scroll_speed);
        self.settings.controls.wheel_zoom_factor =
            clamp_zoom_factor(self.settings.controls.wheel_zoom_factor);
        self.settings.controls.keyboard_zoom_factor =
            clamp_zoom_factor(self.settings.controls.keyboard_zoom_factor);
        self.settings.controls.anti_clip_fade_ms =
            super::interaction_options::clamp_anti_clip_fade_ms(
                self.settings.controls.anti_clip_fade_ms,
            );
        self.ui.controls = crate::egui_app::state::InteractionOptionsState {
            invert_waveform_scroll: self.settings.controls.invert_waveform_scroll,
            waveform_scroll_speed: self.settings.controls.waveform_scroll_speed,
            wheel_zoom_factor: self.settings.controls.wheel_zoom_factor,
            keyboard_zoom_factor: self.settings.controls.keyboard_zoom_factor,
            anti_clip_fade_enabled: self.settings.controls.anti_clip_fade_enabled,
            anti_clip_fade_ms: self.settings.controls.anti_clip_fade_ms,
            destructive_yolo_mode: self.settings.controls.destructive_yolo_mode,
            waveform_channel_view: self.settings.controls.waveform_channel_view,
            input_monitoring_enabled: self.settings.controls.input_monitoring_enabled,
        };
        self.ui.waveform.channel_view = self.settings.controls.waveform_channel_view;
        self.ui.waveform.bpm_snap_enabled = self.settings.controls.bpm_snap_enabled;
        self.ui.waveform.bpm_value = normalize_bpm_value(self.settings.controls.bpm_value);
        self.ui.waveform.transient_markers_enabled =
            self.settings.controls.transient_markers_enabled;
        self.ui.waveform.transient_snap_enabled = self.settings.controls.transient_snap_enabled;
        self.ui.waveform.normalized_audition_enabled =
            self.settings.controls.normalized_audition_enabled;
        if let Some(value) = self.ui.waveform.bpm_value {
            let rounded = value.round();
            if (value - rounded).abs() < 0.01 {
                self.ui.waveform.bpm_input = format!("{rounded:.0}");
            } else {
                self.ui.waveform.bpm_input = format!("{value:.2}");
            }
        } else {
            self.ui.waveform.bpm_input.clear();
        }
        self.refresh_audio_options(true);
        self.refresh_audio_input_options(true);
        self.apply_volume(cfg.core.volume);
        self.ui.trash_folder = cfg.core.trash_folder.clone();
        self.ui.collection_export_root = cfg.core.collection_export_root.clone();
        self.ui.update.last_seen_nightly_published_at =
            cfg.core.updates.last_seen_nightly_published_at.clone();
        self.ui.hints.show_on_startup = self.settings.hints.show_on_startup;
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
        let mut purge_failures = Vec::new();
        for source in &self.library.sources {
            if let Ok(mut conn) = super::analysis_jobs::open_source_db(&source.root) {
                if let Err(err) = super::analysis_jobs::purge_orphaned_samples(&mut conn) {
                    purge_failures.push((source.root.display().to_string(), err));
                }
            }
        }
        for (root, err) in purge_failures {
            self.set_status(
                format!("Failed to purge orphaned sample data for {root}: {err}"),
                StatusTone::Warning,
            );
        }
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
        self.sync_collection_exports_on_startup();
        self.selection_state.ctx.selected_source = cfg
            .core
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
        self.maybe_open_hint_of_day();
        self.runtime.analysis.set_max_analysis_duration_seconds(
            self.settings.analysis.max_analysis_duration_seconds,
        );
        self.runtime
            .analysis
            .set_worker_count(self.settings.analysis.analysis_worker_count);
        self.sync_analysis_backend_from_env();
        self.apply_analysis_backend_env();
        self.runtime
            .analysis
            .start(self.runtime.jobs.message_sender());
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
            core: crate::sample_sources::config::AppSettingsCore {
                feature_flags: self.settings.feature_flags.clone(),
                analysis: self.settings.analysis.clone(),
                updates: self.settings.updates.clone(),
                hints: self.settings.hints.clone(),
                app_data_dir: self.settings.app_data_dir.clone(),
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
                audio_input: self.settings.audio_input.clone(),
                volume: self.ui.volume,
                controls: self.settings.controls.clone(),
            },
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

fn normalize_bpm_value(value: f32) -> Option<f32> {
    if value.is_finite() && value > 0.0 {
        Some(value)
    } else {
        None
    }
}
