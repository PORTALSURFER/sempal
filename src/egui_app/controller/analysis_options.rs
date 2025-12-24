use super::*;

const MIN_MAX_ANALYSIS_DURATION_SECONDS: f32 = 1.0;
const MAX_MAX_ANALYSIS_DURATION_SECONDS: f32 = 60.0 * 60.0;
const MAX_ANALYSIS_WORKER_COUNT: u32 = 64;

pub(super) fn clamp_max_analysis_duration_seconds(seconds: f32) -> f32 {
    seconds.clamp(
        MIN_MAX_ANALYSIS_DURATION_SECONDS,
        MAX_MAX_ANALYSIS_DURATION_SECONDS,
    )
}

impl EguiController {
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
}
