use super::*;
use crate::egui_app::controller::library::collection_export;
use crate::egui_app::controller::library::collection_items_helpers::file_metadata;
use crate::sample_sources::is_supported_audio;
use std::path::{Path, PathBuf};

impl EguiController {
    /// Paste file paths from the system clipboard into the active source or collection.
    pub fn paste_files_from_clipboard(&mut self) -> bool {
        let input = match read_clipboard_audio_paths() {
            Ok(Some(input)) => input,
            Ok(None) => return false,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return true;
            }
        };
        if input.paths.is_empty() {
            self.set_status(
                "Clipboard has no supported audio files to paste",
                StatusTone::Warning,
            );
            return true;
        }
        let summary = match paste_clipboard_paths(self, input) {
            Ok(summary) => summary,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return true;
            }
        };
        report_paste_summary(self, summary);
        true
    }
}

struct PasteResult {
    added: usize,
    errors: Vec<String>,
}

struct ClipboardPasteInput {
    paths: Vec<PathBuf>,
    skipped: usize,
}

struct PasteSummary {
    added: usize,
    skipped: usize,
    errors: Vec<String>,
    target_label: &'static str,
}

fn read_clipboard_audio_paths() -> Result<Option<ClipboardPasteInput>, String> {
    let paths = crate::external_clipboard::read_file_paths()?;
    if paths.is_empty() {
        return Ok(None);
    }
    let mut supported = Vec::new();
    let mut skipped = 0usize;
    for path in paths {
        if !path.is_file() || !is_supported_audio(&path) {
            skipped += 1;
            continue;
        }
        supported.push(path);
    }
    Ok(Some(ClipboardPasteInput {
        paths: supported,
        skipped,
    }))
}

fn paste_clipboard_paths(
    controller: &mut EguiController,
    input: ClipboardPasteInput,
) -> Result<PasteSummary, String> {
    if let Some(collection_id) = controller.current_collection_id() {
        let clip_root = collection_clip_root(controller, &collection_id)?;
        let result =
            paste_files_into_collection(controller, &collection_id, &clip_root, &input.paths);
        return Ok(PasteSummary {
            added: result.added,
            skipped: input.skipped,
            errors: result.errors,
            target_label: "collection",
        });
    }
    let Some(source) = controller.current_source() else {
        return Err("Select a source first".into());
    };
    let result = paste_files_into_source(controller, &source, &input.paths);
    Ok(PasteSummary {
        added: result.added,
        skipped: input.skipped,
        errors: result.errors,
        target_label: "source",
    })
}

fn report_paste_summary(controller: &mut EguiController, summary: PasteSummary) {
    if summary.added == 0 && summary.errors.is_empty() {
        controller.set_status("No files pasted", StatusTone::Warning);
        return;
    }
    let tone = if summary.errors.is_empty() {
        StatusTone::Info
    } else {
        StatusTone::Warning
    };
    let mut message = format!(
        "Pasted {} file(s) into {}",
        summary.added, summary.target_label
    );
    if summary.skipped > 0 {
        message.push_str(&format!(" (skipped {})", summary.skipped));
    }
    if !summary.errors.is_empty() {
        message.push_str(&format!(" with {} error(s)", summary.errors.len()));
    }
    controller.set_status(message, tone);
    for err in summary.errors {
        eprintln!("Paste error: {err}");
    }
}

fn paste_files_into_source(
    controller: &mut EguiController,
    source: &SampleSource,
    paths: &[PathBuf],
) -> PasteResult {
    let mut errors = Vec::new();
    let mut added = 0usize;
    let mut last_relative = None;
    for path in paths {
        match paste_file_to_source(controller, source, path) {
            Ok(relative) => {
                added += 1;
                last_relative = Some(relative);
            }
            Err(err) => errors.push(err),
        }
    }
    if let Some(relative) = last_relative {
        controller
            .runtime
            .jobs
            .set_pending_select_path(Some(relative));
    }
    if added > 0 {
        controller.invalidate_wav_entries_for_source(source);
    }
    PasteResult { added, errors }
}

fn paste_file_to_source(
    controller: &mut EguiController,
    source: &SampleSource,
    path: &Path,
) -> Result<PathBuf, String> {
    if !source.root.is_dir() {
        return Err("Source folder is not available".into());
    }
    let relative = unique_destination_name(&source.root, path)?;
    let absolute = source.root.join(&relative);
    std::fs::copy(path, &absolute)
        .map_err(|err| format!("Failed to paste {}: {err}", path.display()))?;
    let result = (|| -> Result<(), String> {
        let (file_size, modified_ns) = file_metadata(&absolute)?;
        let db = controller
            .database_for(source)
            .map_err(|err| format!("Failed to open source DB: {err}"))?;
        db.upsert_file(&relative, file_size, modified_ns)
            .map_err(|err| format!("Failed to register pasted file: {err}"))?;
        controller.enqueue_similarity_for_new_sample(source, &relative, file_size, modified_ns);
        Ok(())
    })();
    match result {
        Ok(()) => Ok(relative),
        Err(err) => {
            if let Err(remove_err) = std::fs::remove_file(&absolute) {
                Err(format!(
                    "{err}; failed to remove pasted file {}: {remove_err}",
                    absolute.display()
                ))
            } else {
                Err(err)
            }
        }
    }
}

fn paste_files_into_collection(
    controller: &mut EguiController,
    collection_id: &CollectionId,
    clip_root: &Path,
    paths: &[PathBuf],
) -> PasteResult {
    let mut errors = Vec::new();
    let mut added = 0usize;
    for path in paths {
        match paste_file_to_collection(controller, collection_id, clip_root, path) {
            Ok(_) => {
                added += 1;
            }
            Err(err) => errors.push(err),
        }
    }
    PasteResult { added, errors }
}

fn paste_file_to_collection(
    controller: &mut EguiController,
    collection_id: &CollectionId,
    clip_root: &Path,
    path: &Path,
) -> Result<PathBuf, String> {
    if !clip_root.is_dir() {
        return Err("Collection export folder is not available".into());
    }
    let relative = unique_destination_name(clip_root, path)?;
    let absolute = clip_root.join(&relative);
    std::fs::copy(path, &absolute)
        .map_err(|err| format!("Failed to paste {}: {err}", path.display()))?;
    controller.add_clip_to_collection(collection_id, clip_root.to_path_buf(), relative.clone())?;
    Ok(relative)
}

fn unique_destination_name(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| "Clipboard file has no name".to_string())?;
    let candidate = PathBuf::from(file_name);
    if !root.join(&candidate).exists() {
        return Ok(candidate);
    }
    let stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "sample".to_string());
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_string());
    for index in 1..=999 {
        let suffix = format!("{stem}_copy{index:03}");
        let file_name = if let Some(ext) = &extension {
            format!("{suffix}.{ext}")
        } else {
            suffix
        };
        let candidate = PathBuf::from(file_name);
        if !root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err("Unable to find a unique paste name".into())
}

fn collection_clip_root(
    controller: &mut EguiController,
    collection_id: &CollectionId,
) -> Result<PathBuf, String> {
    let Some(collection) = controller
        .library
        .collections
        .iter()
        .find(|c| &c.id == collection_id)
    else {
        return Err("Collection not found".into());
    };
    let preferred = collection_export::resolved_export_dir(
        collection,
        controller.settings.collection_export_root.as_deref(),
    );
    if let Some(path) = preferred {
        if path.exists() && !path.is_dir() {
            return Err(format!(
                "Collection export path is not a directory: {}",
                path.display()
            ));
        }
        std::fs::create_dir_all(&path).map_err(|err| {
            format!(
                "Failed to create collection export path {}: {err}",
                path.display()
            )
        })?;
        return Ok(path);
    }
    let fallback = crate::app_dirs::app_root_dir()
        .map_err(|err| err.to_string())?
        .join("collection_clips")
        .join(collection_id.as_str());
    std::fs::create_dir_all(&fallback)
        .map_err(|err| format!("Failed to create collection clip folder: {err}"))?;
    Ok(fallback)
}
