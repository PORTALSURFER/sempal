use super::*;

const MIN_MAX_ANALYSIS_DURATION_SECONDS: f32 = 1.0;
const MAX_MAX_ANALYSIS_DURATION_SECONDS: f32 = 60.0 * 60.0;
const MIN_LONG_SAMPLE_THRESHOLD_SECONDS: f32 = 1.0;
const MAX_LONG_SAMPLE_THRESHOLD_SECONDS: f32 = 60.0 * 60.0;
const MAX_ANALYSIS_WORKER_COUNT: u32 = 64;
const MIN_FAST_PREP_SAMPLE_RATE: u32 = 8_000;
const WGPU_POWER_ENV: &str = "WGPU_POWER_PREFERENCE";
const WGPU_ADAPTER_ENV: &str = "WGPU_ADAPTER_NAME";

fn set_env_var(key: &str, value: &str) {
    // Safety: env var mutation is process-global; we keep it scoped to runtime config changes.
    unsafe {
        std::env::set_var(key, value);
    }
}

fn remove_env_var(key: &str) {
    // Safety: env var mutation is process-global; we keep it scoped to runtime config changes.
    unsafe {
        std::env::remove_var(key);
    }
}

pub(crate) fn clamp_max_analysis_duration_seconds(seconds: f32) -> f32 {
    seconds.clamp(
        MIN_MAX_ANALYSIS_DURATION_SECONDS,
        MAX_MAX_ANALYSIS_DURATION_SECONDS,
    )
}

pub(crate) fn clamp_long_sample_threshold_seconds(seconds: f32) -> f32 {
    seconds.clamp(
        MIN_LONG_SAMPLE_THRESHOLD_SECONDS,
        MAX_LONG_SAMPLE_THRESHOLD_SECONDS,
    )
}

impl EguiController {
    pub(crate) fn sync_analysis_backend_from_env(&mut self) {
        if let Ok(value) = std::env::var(WGPU_POWER_ENV) {
            if let Some(parsed) =
                crate::sample_sources::config::WgpuPowerPreference::from_env(&value)
            {
                self.settings.analysis.wgpu_power_preference = parsed;
            }
        }
        if let Ok(value) = std::env::var(WGPU_ADAPTER_ENV) {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                self.settings.analysis.wgpu_adapter_name = None;
            } else {
                self.settings.analysis.wgpu_adapter_name = Some(trimmed.to_string());
            }
        }
    }

    pub(crate) fn apply_analysis_backend_env(&mut self) {
        match self.settings.analysis.wgpu_power_preference.as_env() {
            Some(value) => set_env_var(WGPU_POWER_ENV, value),
            None => remove_env_var(WGPU_POWER_ENV),
        }

        match self.settings.analysis.wgpu_adapter_name.as_ref() {
            Some(value) if !value.trim().is_empty() => {
                set_env_var(WGPU_ADAPTER_ENV, value);
            }
            _ => remove_env_var(WGPU_ADAPTER_ENV),
        }
    }

    /// Return the maximum analysis duration in seconds.
    pub fn max_analysis_duration_seconds(&self) -> f32 {
        self.settings.analysis.max_analysis_duration_seconds
    }

    /// Return whether similarity-prep duration capping is enabled.
    pub fn similarity_prep_duration_cap_enabled(&self) -> bool {
        self.settings.analysis.limit_similarity_prep_duration
    }

    /// Return the threshold for marking long samples in the browser.
    pub fn long_sample_threshold_seconds(&self) -> f32 {
        self.settings.analysis.long_sample_threshold_seconds
    }

    /// Enable or disable similarity-prep duration capping.
    pub fn set_similarity_prep_duration_cap_enabled(&mut self, enabled: bool) {
        if self.settings.analysis.limit_similarity_prep_duration == enabled {
            return;
        }
        self.settings.analysis.limit_similarity_prep_duration = enabled;
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    /// Set the maximum analysis duration in seconds.
    pub fn set_max_analysis_duration_seconds(&mut self, seconds: f32) {
        let clamped = clamp_max_analysis_duration_seconds(seconds);
        if (self.settings.analysis.max_analysis_duration_seconds - clamped).abs() < f32::EPSILON {
            return;
        }
        self.settings.analysis.max_analysis_duration_seconds = clamped;
        self.runtime
            .analysis
            .set_max_analysis_duration_seconds(clamped);
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    /// Set the threshold for marking long samples in the browser.
    pub fn set_long_sample_threshold_seconds(&mut self, seconds: f32) {
        let clamped = clamp_long_sample_threshold_seconds(seconds);
        if (self.settings.analysis.long_sample_threshold_seconds - clamped).abs() < f32::EPSILON {
            return;
        }
        self.settings.analysis.long_sample_threshold_seconds = clamped;
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    /// Return the configured analysis worker count.
    pub fn analysis_worker_count(&self) -> u32 {
        self.settings.analysis.analysis_worker_count
    }

    /// Return the auto-selected analysis worker count for this host.
    pub fn analysis_auto_worker_count(&self) -> u32 {
        crate::egui_app::controller::library::analysis_jobs::default_worker_count()
    }

    /// Return whether fast similarity-prep mode is enabled.
    pub fn similarity_prep_fast_mode_enabled(&self) -> bool {
        self.settings.analysis.fast_similarity_prep
    }

    /// Enable or disable fast similarity-prep mode.
    pub fn set_similarity_prep_fast_mode_enabled(&mut self, enabled: bool) {
        if self.settings.analysis.fast_similarity_prep == enabled {
            return;
        }
        self.settings.analysis.fast_similarity_prep = enabled;
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    /// Return the sample rate used for fast similarity prep.
    pub fn similarity_prep_fast_sample_rate(&self) -> u32 {
        self.settings.analysis.fast_similarity_prep_sample_rate
    }

    /// Set the sample rate used for fast similarity prep.
    pub fn set_similarity_prep_fast_sample_rate(&mut self, value: u32) {
        let max_rate = crate::analysis::audio::ANALYSIS_SAMPLE_RATE;
        let clamped = value.clamp(MIN_FAST_PREP_SAMPLE_RATE, max_rate);
        if self.settings.analysis.fast_similarity_prep_sample_rate == clamped {
            return;
        }
        self.settings.analysis.fast_similarity_prep_sample_rate = clamped;
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    /// Set a fixed analysis worker count.
    pub fn set_analysis_worker_count(&mut self, value: u32) {
        let clamped = value.min(MAX_ANALYSIS_WORKER_COUNT);
        if self.settings.analysis.analysis_worker_count == clamped {
            return;
        }
        self.settings.analysis.analysis_worker_count = clamped;
        self.runtime.analysis.set_worker_count(clamped);
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    /// Restrict analysis workers to the provided source IDs.
    pub fn set_analysis_worker_allowed_sources(&mut self, sources: Option<Vec<SourceId>>) {
        self.runtime.analysis.set_allowed_sources(sources);
    }

    /// Restrict analysis workers to the currently selected source.
    pub fn set_analysis_worker_allowed_sources_to_selected(&mut self) {
        let sources = self.current_source().map(|source| vec![source.id]);
        self.set_analysis_worker_allowed_sources(sources);
    }


    /// Return the configured WGPU power preference.
    pub fn wgpu_power_preference(&self) -> crate::sample_sources::config::WgpuPowerPreference {
        self.settings.analysis.wgpu_power_preference
    }

    /// Update the WGPU power preference and persist it.
    pub fn set_wgpu_power_preference(
        &mut self,
        preference: crate::sample_sources::config::WgpuPowerPreference,
    ) {
        if self.settings.analysis.wgpu_power_preference == preference {
            return;
        }
        self.settings.analysis.wgpu_power_preference = preference;
        self.apply_analysis_backend_env();
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    /// Return the configured WGPU adapter name, if any.
    pub fn wgpu_adapter_name(&self) -> Option<&str> {
        self.settings.analysis.wgpu_adapter_name.as_deref()
    }

    /// Update the WGPU adapter name and persist it.
    pub fn set_wgpu_adapter_name(&mut self, name: String) {
        let trimmed = name.trim();
        let next = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        if self.settings.analysis.wgpu_adapter_name == next {
            return;
        }
        self.settings.analysis.wgpu_adapter_name = next;
        self.apply_analysis_backend_env();
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

}
