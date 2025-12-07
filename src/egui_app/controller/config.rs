use super::*;

impl EguiController {
    /// Load persisted configuration and populate initial UI state.
    pub fn load_configuration(&mut self) -> Result<(), crate::sample_sources::config::ConfigError> {
        let cfg = crate::sample_sources::config::load_or_default()?;
        self.feature_flags = cfg.feature_flags;
        self.ui.collections.enabled = self.feature_flags.collections_enabled;
        self.apply_volume(cfg.volume);
        let mut sources = cfg.sources.clone();
        let original_count = sources.len();
        sources.retain(|s| s.root.is_dir());
        if sources.len() != original_count {
            self.set_status("Removed missing sources from config", StatusTone::Warning);
        }
        self.sources = sources;
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
            last_selected_source: self.selected_source.clone(),
            volume: self.ui.volume,
        })
    }
}
