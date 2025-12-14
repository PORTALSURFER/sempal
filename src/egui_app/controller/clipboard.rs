use super::*;
use std::path::{Path, PathBuf};

impl EguiController {
    /// Copy the currently selected samples to the system clipboard as file drops.
    pub fn copy_selection_to_clipboard(&mut self) {
        let result = self.selected_sample_paths();
        match result {
            Ok(paths) if paths.is_empty() => {
                self.set_status("Select a sample to copy", StatusTone::Warning);
            }
            Ok(paths) => {
                if let Err(err) = crate::external_clipboard::copy_file_paths(&paths) {
                    self.set_status(err, StatusTone::Error);
                } else {
                    let label = if paths.len() == 1 {
                        format!("Copied {} to clipboard", display_path(&paths[0]))
                    } else {
                        format!("Copied {} files to clipboard", paths.len())
                    };
                    self.set_status(label, StatusTone::Info);
                }
            }
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }

    fn selected_sample_paths(&self) -> Result<Vec<PathBuf>, String> {
        if let Some(idx) = self.ui.collections.selected_sample {
            let sample = self
                .ui
                .collections
                .samples
                .get(idx)
                .ok_or_else(|| "Collection sample not found".to_string())?;
            let source = self.library.sources
                .iter()
                .find(|s| s.id == sample.source_id)
                .ok_or_else(|| "Source not available for this sample".to_string())?;
            let path = source.root.join(&sample.path);
            return Ok(vec![path]);
        }

        let Some(source) = self.current_source() else {
            return Err("Select a source first".into());
        };
        let mut paths: Vec<PathBuf> = if !self.ui.browser.selected_paths.is_empty() {
            self.ui
                .browser
                .selected_paths
                .iter()
                .map(|p| source.root.join(p))
                .collect()
        } else if let Some(selected) = self.wav_selection.selected_wav.as_ref() {
            vec![source.root.join(selected)]
        } else {
            Vec::new()
        };
        paths.retain(|p| p.exists());
        Ok(paths)
    }
}

fn display_path(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.display().to_string())
}
