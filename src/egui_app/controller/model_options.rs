use super::*;

impl EguiController {
    pub fn unknown_confidence_threshold(&self) -> f32 {
        self.settings.model.unknown_confidence_threshold
    }

    pub fn set_unknown_confidence_threshold(&mut self, value: f32) {
        let clamped = value.clamp(0.0, 1.0);
        if (self.settings.model.unknown_confidence_threshold - clamped).abs() < f32::EPSILON {
            return;
        }
        self.settings.model.unknown_confidence_threshold = clamped;
        self.runtime
            .analysis
            .set_unknown_confidence_threshold(clamped);
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    pub fn retrain_min_confidence(&self) -> f32 {
        self.settings.training.retrain_min_confidence
    }

    pub fn set_retrain_min_confidence(&mut self, value: f32) {
        let clamped = value.clamp(0.0, 1.0);
        if (self.settings.training.retrain_min_confidence - clamped).abs() < f32::EPSILON {
            return;
        }
        self.settings.training.retrain_min_confidence = clamped;
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    pub fn retrain_pack_depth(&self) -> usize {
        self.settings.training.retrain_pack_depth
    }

    pub fn set_retrain_pack_depth(&mut self, value: usize) {
        let clamped = value.clamp(1, 8);
        if self.settings.training.retrain_pack_depth == clamped {
            return;
        }
        self.settings.training.retrain_pack_depth = clamped;
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }
}
