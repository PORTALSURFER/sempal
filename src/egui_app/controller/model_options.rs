use super::*;

impl EguiController {
    pub fn unknown_confidence_threshold(&self) -> f32 {
        self.settings.model.unknown_confidence_threshold
    }

    pub fn classifier_model_id(&self) -> Option<String> {
        let value = self.settings.model.classifier_model_id.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    }

    pub fn use_user_overrides_in_browser(&self) -> bool {
        self.settings.model.use_user_overrides
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
        self.ui_cache.browser.predictions.clear();
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    pub fn set_use_user_overrides_in_browser(&mut self, value: bool) {
        if self.settings.model.use_user_overrides == value {
            return;
        }
        self.settings.model.use_user_overrides = value;
        self.ui_cache.browser.predictions.clear();
        self.queue_prediction_load_for_selection();
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

    pub fn retrain_use_user_labels(&self) -> bool {
        self.settings.training.use_user_labels
    }

    pub fn set_retrain_use_user_labels(&mut self, value: bool) {
        if self.settings.training.use_user_labels == value {
            return;
        }
        self.settings.training.use_user_labels = value;
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    pub fn training_dataset_root(&self) -> Option<PathBuf> {
        self.settings.training.training_dataset_root.clone()
    }

    pub fn set_training_dataset_root(&mut self, root: Option<PathBuf>) {
        if self.settings.training.training_dataset_root == root {
            return;
        }
        self.settings.training.training_dataset_root = root;
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }

    pub fn training_model_kind(&self) -> crate::sample_sources::config::TrainingModelKind {
        self.settings.training.model_kind.clone()
    }

    pub fn set_training_model_kind(
        &mut self,
        value: crate::sample_sources::config::TrainingModelKind,
    ) {
        if self.settings.training.model_kind == value {
            return;
        }
        self.settings.training.model_kind = value;
        if let Err(err) = self.persist_config("Failed to save options") {
            self.set_status(err, StatusTone::Warning);
        }
    }
}
