use super::*;
use std::path::{Path, PathBuf};

impl EguiController {
    /// Copy either the current waveform selection (as a new wav file) or the currently selected
    /// samples to the system clipboard as file drops.
    pub fn copy_selection_to_clipboard(&mut self) {
        let result = self.clipboard_paths_for_copy();
        match result {
            Ok(paths) if paths.is_empty() => {
                self.set_status("Select a sample to copy", StatusTone::Warning);
            }
            Ok(paths) => {
                if let Err(err) = crate::external_clipboard::copy_file_paths(&paths) {
                    self.set_status(err, StatusTone::Error);
                } else {
                    let label = clipboard_copy_label(&paths);
                    self.set_status(label, StatusTone::Info);
                }
            }
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }

    /// Copy the status log text to the system clipboard.
    pub fn copy_status_log_to_clipboard(&mut self) {
        let text = self.ui.status.log_text();
        if text.is_empty() {
            self.set_status("Status log is empty", StatusTone::Info);
            return;
        }
        match crate::external_clipboard::copy_text(&text) {
            Ok(()) => self.set_status("Copied status log to clipboard", StatusTone::Info),
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }

    fn clipboard_paths_for_copy(&mut self) -> Result<Vec<PathBuf>, String> {
        let waveform_copy = self.waveform_selection_clipboard_path()?;
        if let Some(path) = waveform_copy {
            return Ok(vec![path]);
        }
        self.selected_sample_paths()
    }

    fn waveform_selection_clipboard_path(&mut self) -> Result<Option<PathBuf>, String> {
        if self.ui.focus.context != crate::egui_app::state::FocusContext::Waveform {
            return Ok(None);
        }
        let Some(bounds) = self.selection_state.range.range() else {
            return Ok(None);
        };
        let (source_id, relative_path) = {
            let audio = self
                .sample_view
                .wav
                .loaded_audio
                .as_ref()
                .ok_or_else(|| "Load a sample before copying a selection".to_string())?;
            (audio.source_id.clone(), audio.relative_path.clone())
        };
        let clip_root = crate::app_dirs::app_root_dir()
            .map_err(|err| err.to_string())?
            .join("clipboard_clips");
        std::fs::create_dir_all(&clip_root)
            .map_err(|err| format!("Failed to create clipboard folder: {err}"))?;
        let name_hint = relative_path
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("selection.wav"));
        let entry = self.export_selection_clip_to_root(
            &source_id,
            &relative_path,
            bounds,
            None,
            &clip_root,
            &name_hint,
        )?;
        Ok(Some(clip_root.join(entry.relative_path)))
    }

    fn selected_sample_paths(&self) -> Result<Vec<PathBuf>, String> {
        if let Some(idx) = self.ui.collections.selected_sample {
            let sample = self
                .ui
                .collections
                .samples
                .get(idx)
                .ok_or_else(|| "Collection sample not found".to_string())?;
            let source = self
                .library
                .sources
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
        } else if let Some(selected) = self.sample_view.wav.selected_wav.as_ref() {
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

fn clipboard_copy_label(paths: &[PathBuf]) -> String {
    if paths.len() == 1 {
        format!("Copied {} to clipboard", display_path(&paths[0]))
    } else {
        format!("Copied {} files to clipboard", paths.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_dirs::ConfigBaseGuard;
    use crate::egui_app::controller::test_support::write_test_wav;
    use crate::egui_app::state::FocusContext;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn copy_shortcut_exports_waveform_selection_clip_for_clipboard_paths() {
        let temp = tempdir().unwrap();
        let _guard = ConfigBaseGuard::set(temp.path().to_path_buf());
        let source_root = temp.path().join("source");
        std::fs::create_dir_all(&source_root).unwrap();

        let renderer = crate::waveform::WaveformRenderer::new(12, 12);
        let mut controller = EguiController::new(renderer, None);
        let source = SampleSource::new(source_root.clone());
        controller.library.sources.push(source.clone());
        controller.selection_state.ctx.selected_source = Some(source.id.clone());

        let orig = source_root.join("clip.wav");
        write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
        controller
            .load_waveform_for_selection(&source, Path::new("clip.wav"))
            .unwrap();

        controller.ui.focus.context = FocusContext::Waveform;
        controller
            .selection_state
            .range
            .set_range(Some(SelectionRange::new(0.25, 0.75)));

        let paths = controller.clipboard_paths_for_copy().unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].is_file());
        assert!(
            paths[0]
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("clip_sel"))
        );
    }
}
