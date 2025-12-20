use super::*;
use crate::sample_sources::config::normalize_path;
use rfd::FileDialog;

impl EguiController {
    pub fn pick_training_dataset_root(&mut self) {
        let Some(path) = FileDialog::new().pick_folder() else {
            return;
        };
        let normalized = normalize_path(path.as_path());
        self.set_training_dataset_root(Some(normalized.clone()));
        self.set_status(
            format!("Training dataset set to {}", normalized.display()),
            StatusTone::Info,
        );
    }

    pub fn clear_training_dataset_root(&mut self) {
        self.set_training_dataset_root(None);
        self.set_status("Training dataset cleared", StatusTone::Info);
    }

    pub fn open_training_dataset_root(&mut self) {
        let Some(path) = self.training_dataset_root() else {
            self.set_status("No training dataset folder set", StatusTone::Warning);
            return;
        };
        if let Err(err) = open::that(&path) {
            self.set_status(
                format!("Could not open training dataset folder {}: {err}", path.display()),
                StatusTone::Error,
            );
        }
    }
}
