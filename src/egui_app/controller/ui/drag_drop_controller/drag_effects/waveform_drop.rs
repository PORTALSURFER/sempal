use super::super::{DragDropController, file_metadata};
use crate::egui_app::ui::style::StatusTone;
use crate::sample_sources::SourceId;
use std::fs;
use std::path::PathBuf;
use tracing::info;

impl DragDropController<'_> {
    pub(in crate::egui_app::controller::drag_drop_controller) fn handle_waveform_sample_drop_to_browser(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
    ) {
        info!(
            "handle_waveform_sample_drop_to_browser source={} path={}",
            source_id,
            relative_path.display()
        );
        self.set_status(
            format!(
                "Waveform drop to browser handled for {}",
                relative_path.display()
            ),
            StatusTone::Info,
        );
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|s| s.id == source_id)
            .cloned()
        else {
            self.set_status("Source not available", StatusTone::Error);
            return;
        };
        let absolute = source.root.join(&relative_path);
        if !absolute.exists() {
            self.set_status(
                format!("Source file missing: {}", relative_path.display()),
                StatusTone::Error,
            );
            return;
        }
        let parent = relative_path.parent().map(|parent| parent.to_path_buf());
        let stem = relative_path
            .file_stem()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "sample".to_string());
        let extension = relative_path
            .extension()
            .map(|ext| ext.to_string_lossy().to_string());
        let mut copy_relative = None;
        let mut copy_absolute = None;
        for count in 1.. {
            let suffix = format!("{stem}_copy{count:03}");
            let file_name = if let Some(ext) = &extension {
                format!("{suffix}.{ext}")
            } else {
                suffix
            };
            let candidate = if let Some(parent) = parent.as_ref() {
                if parent.as_os_str().is_empty() {
                    PathBuf::from(&file_name)
                } else {
                    parent.join(&file_name)
                }
            } else {
                PathBuf::from(&file_name)
            };
            let candidate_abs = source.root.join(&candidate);
            if !candidate_abs.exists() {
                copy_relative = Some(candidate);
                copy_absolute = Some(candidate_abs);
                break;
            }
        }
        let copy_relative = match copy_relative {
            Some(path) => path,
            None => {
                self.set_status("Unable to find a unique copy name", StatusTone::Error);
                return;
            }
        };
        let copy_absolute = copy_absolute.unwrap();
        if let Some(parent) = copy_relative.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(err) = fs::create_dir_all(source.root.join(parent)) {
                    self.set_status(
                        format!("Failed to create folder for copy: {err}"),
                        StatusTone::Error,
                    );
                    return;
                }
            }
        }
        if let Err(err) = fs::copy(&absolute, &copy_absolute) {
            self.set_status(format!("Failed to copy sample: {err}"), StatusTone::Error);
            return;
        }
        let (file_size, modified_ns) = match file_metadata(&copy_absolute) {
            Ok(meta) => meta,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        let db = match self.database_for(&source) {
            Ok(db) => db,
            Err(err) => {
                self.set_status(
                    format!("Failed to open source DB: {err}"),
                    StatusTone::Error,
                );
                return;
            }
        };
        if let Err(err) = db.upsert_file(&copy_relative, file_size, modified_ns) {
            self.set_status(format!("Failed to register copy: {err}"), StatusTone::Error);
            return;
        }
        self.enqueue_similarity_for_new_sample(&source, &copy_relative, file_size, modified_ns);
        self.runtime
            .jobs
            .set_pending_select_path(Some(copy_relative.clone()));
        self.invalidate_wav_entries_for_source(&source);
        self.set_status(
            format!("Copied sample to {}", copy_relative.display()),
            StatusTone::Info,
        );
    }
}
