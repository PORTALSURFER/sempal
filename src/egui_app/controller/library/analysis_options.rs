use super::*;

const MIN_MAX_ANALYSIS_DURATION_SECONDS: f32 = 1.0;
const MAX_MAX_ANALYSIS_DURATION_SECONDS: f32 = 60.0 * 60.0;
const MAX_ANALYSIS_WORKER_COUNT: u32 = 64;
const MIN_FAST_PREP_SAMPLE_RATE: u32 = 8_000;
const PANNS_BACKEND_ENV: &str = "SEMPAL_PANNS_BACKEND";
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

impl EguiController {
    pub(crate) fn sync_analysis_backend_from_env(&mut self) {
        if let Ok(value) = std::env::var(PANNS_BACKEND_ENV) {
            if let Some(parsed) =
                crate::sample_sources::config::PannsBackendChoice::from_env(&value)
            {
                self.settings.analysis.panns_backend = parsed;
            }
        }
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
        let mut backend = self.settings.analysis.panns_backend;
        if matches!(
            backend,
            crate::sample_sources::config::PannsBackendChoice::Cuda
        ) && !cfg!(feature = "panns-cuda")
        {
            backend = crate::sample_sources::config::PannsBackendChoice::Wgpu;
            self.set_status(
                "CUDA backend requested but not available in this build; using WGPU.".to_string(),
                StatusTone::Warning,
            );
        }
        if matches!(
            backend,
            crate::sample_sources::config::PannsBackendChoice::Cuda
                | crate::sample_sources::config::PannsBackendChoice::Cpu
        ) {
            set_env_var(PANNS_BACKEND_ENV, backend.as_env());
        } else {
            remove_env_var(PANNS_BACKEND_ENV);
        }

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

    pub fn max_analysis_duration_seconds(&self) -> f32 {
        self.settings.analysis.max_analysis_duration_seconds
    }

    pub fn similarity_prep_duration_cap_enabled(&self) -> bool {
        self.settings.analysis.limit_similarity_prep_duration
    }

    pub fn set_similarity_prep_duration_cap_enabled(&mut self, enabled: bool) {
        if self.settings.analysis.limit_similarity_prep_duration == enabled {
            return;
        }
        self.settings.analysis.limit_similarity_prep_duration = enabled;
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

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

    pub fn analysis_worker_count(&self) -> u32 {
        self.settings.analysis.analysis_worker_count
    }

    pub fn analysis_auto_worker_count(&self) -> u32 {
        crate::egui_app::controller::library::analysis_jobs::default_worker_count()
    }

    pub fn similarity_prep_fast_mode_enabled(&self) -> bool {
        self.settings.analysis.fast_similarity_prep
    }

    pub fn set_similarity_prep_fast_mode_enabled(&mut self, enabled: bool) {
        if self.settings.analysis.fast_similarity_prep == enabled {
            return;
        }
        self.settings.analysis.fast_similarity_prep = enabled;
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    pub fn similarity_prep_fast_sample_rate(&self) -> u32 {
        self.settings.analysis.fast_similarity_prep_sample_rate
    }

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

    pub fn set_analysis_worker_allowed_sources(&mut self, sources: Option<Vec<SourceId>>) {
        self.runtime.analysis.set_allowed_sources(sources);
    }

    pub fn set_analysis_worker_allowed_sources_to_selected(&mut self) {
        let sources = self.current_source().map(|source| vec![source.id]);
        self.set_analysis_worker_allowed_sources(sources);
    }

    pub fn panns_backend(&self) -> crate::sample_sources::config::PannsBackendChoice {
        self.settings.analysis.panns_backend
    }

    pub fn set_panns_backend(
        &mut self,
        backend: crate::sample_sources::config::PannsBackendChoice,
    ) {
        if self.settings.analysis.panns_backend == backend {
            return;
        }
        self.settings.analysis.panns_backend = backend;
        self.runtime.pending_backend_switch = Some(backend);
        let applied = self.apply_pending_backend_switch();
        if !applied {
            self.set_status(
                "Backend switch queued until analysis is idle".to_string(),
                StatusTone::Info,
            );
        }
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    pub fn wgpu_power_preference(&self) -> crate::sample_sources::config::WgpuPowerPreference {
        self.settings.analysis.wgpu_power_preference
    }

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

    pub fn wgpu_adapter_name(&self) -> Option<&str> {
        self.settings.analysis.wgpu_adapter_name.as_deref()
    }

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

    pub(crate) fn apply_pending_backend_switch(&mut self) -> bool {
        let Some(backend) = self.runtime.pending_backend_switch.take() else {
            return false;
        };
        if self.analysis_jobs_active() {
            self.runtime.pending_backend_switch = Some(backend);
            return false;
        }
        if !crate::analysis::embedding::try_reset_panns_model() {
            self.runtime.pending_backend_switch = Some(backend);
            return false;
        }
        self.apply_analysis_backend_env();
        self.runtime
            .analysis
            .restart(self.runtime.jobs.message_sender());
        true
    }

    fn analysis_jobs_active(&self) -> bool {
        self.ui
            .progress
            .analysis
            .as_ref()
            .is_some_and(|snapshot| snapshot.pending > 0 || snapshot.running > 0)
    }
}
