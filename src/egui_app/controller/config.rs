use super::interaction_options::{clamp_scroll_speed, clamp_zoom_factor};
use super::*;

impl EguiController {
    /// Load persisted configuration and populate initial UI state.
    pub fn load_configuration(&mut self) -> Result<(), crate::sample_sources::config::ConfigError> {
        let cfg = crate::sample_sources::config::load_or_default()?;
        self.feature_flags = cfg.feature_flags;
        self.trash_folder = cfg.trash_folder.clone();
        self.collection_export_root = cfg.collection_export_root.clone();
        self.ui.collections.enabled = self.feature_flags.collections_enabled;
        self.audio_output = cfg.audio_output.clone();
        self.ui.audio.selected = self.audio_output.clone();
        self.controls = cfg.controls.clone();
        self.controls.waveform_scroll_speed =
            clamp_scroll_speed(self.controls.waveform_scroll_speed);
        self.controls.wheel_zoom_factor = clamp_zoom_factor(self.controls.wheel_zoom_factor);
        self.controls.keyboard_zoom_factor = clamp_zoom_factor(self.controls.keyboard_zoom_factor);
        self.ui.controls = crate::egui_app::state::InteractionOptionsState {
            invert_waveform_scroll: self.controls.invert_waveform_scroll,
            waveform_scroll_speed: self.controls.waveform_scroll_speed,
            wheel_zoom_factor: self.controls.wheel_zoom_factor,
            keyboard_zoom_factor: self.controls.keyboard_zoom_factor,
            destructive_yolo_mode: self.controls.destructive_yolo_mode,
            waveform_channel_view: self.controls.waveform_channel_view,
        };
        self.ui.waveform.channel_view = self.controls.waveform_channel_view;
        self.refresh_audio_options();
        self.apply_volume(cfg.volume);
        self.ui.trash_folder = cfg.trash_folder.clone();
        self.ui.collection_export_root = cfg.collection_export_root.clone();
        self.sources = cfg.sources.clone();
        self.rebuild_missing_sources();
        if !self.missing_sources.is_empty() {
            let count = self.missing_sources.len();
            let suffix = if count == 1 { "" } else { "s" };
            self.set_status(
                format!("{count} source{suffix} unavailable"),
                StatusTone::Warning,
            );
        }
        self.collections = cfg.collections;
        self.selected_source = cfg
            .last_selected_source
            .filter(|id| self.sources.iter().any(|s| &s.id == id));
        self.ensure_collection_selection();
        self.refresh_sources_ui();
        self.refresh_collections_ui();
        if self.selected_source.is_some() {
            let _ = self.refresh_wavs();
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
            sources: self.sources.clone(),
            collections: self.collections.clone(),
            feature_flags: self.feature_flags.clone(),
            trash_folder: self.trash_folder.clone(),
            collection_export_root: self.collection_export_root.clone(),
            last_selected_source: self.selected_source.clone(),
            audio_output: self.audio_output.clone(),
            volume: self.ui.volume,
            controls: self.controls.clone(),
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
