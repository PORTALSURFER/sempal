use super::*;

impl EguiController {
    /// Load persisted configuration and populate initial UI state.
    pub fn load_configuration(&mut self) -> Result<(), crate::sample_sources::config::ConfigError> {
        let cfg = crate::sample_sources::config::load_or_default()?;
        self.feature_flags = cfg.feature_flags;
        self.trash_folder = cfg.trash_folder.clone();
        self.ui.collections.enabled = self.feature_flags.collections_enabled;
        self.apply_volume(cfg.volume);
        self.ui.trash_folder = cfg.trash_folder.clone();
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
            last_selected_source: self.selected_source.clone(),
            volume: self.ui.volume,
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
