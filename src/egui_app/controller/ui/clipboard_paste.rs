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

    /// Import external audio files into the active source folder.
    pub(crate) fn import_external_files_to_source_folder(
        &mut self,
        target_folder: PathBuf,
        paths: Vec<PathBuf>,
    ) {
        if paths.is_empty() {
            return;
        }
        let Some(source) = self.current_source() else {
            self.set_status("Select a source first", StatusTone::Info);
            return;
        };
        if let Err(err) = validate_relative_folder_path(&target_folder) {
            self.set_status(err, StatusTone::Error);
            return;
        }
        let (supported, skipped) = filter_supported_audio_paths(paths);
        if supported.is_empty() {
            self.set_status(
                "No supported audio files to import",
                StatusTone::Warning,
            );
            return;
        }
        let result = import_files_into_source_folder(self, &source, &target_folder, &supported);
        let target_label = if target_folder.as_os_str().is_empty() {
            "source root".to_string()
        } else {
            format!("folder {}", target_folder.display())
        };
        report_import_summary(
            self,
            ImportSummary {
                added: result.added,
                skipped,
                errors: result.errors,
                target_label,
            },
        );
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

struct ImportResult {
    added: usize,
    errors: Vec<String>,
}

struct ImportSummary {
    added: usize,
    skipped: usize,
    errors: Vec<String>,
    target_label: String,
}

fn read_clipboard_audio_paths() -> Result<Option<ClipboardPasteInput>, String> {
    let paths = crate::external_clipboard::read_file_paths()?;
    if paths.is_empty() {
        return Ok(None);
    }
    let (supported, skipped) = filter_supported_audio_paths(paths);
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

fn report_import_summary(controller: &mut EguiController, summary: ImportSummary) {
    if summary.added == 0 && summary.errors.is_empty() {
        controller.set_status("No files imported", StatusTone::Warning);
        return;
    }
    let tone = if summary.errors.is_empty() {
        StatusTone::Info
    } else {
        StatusTone::Warning
    };
    let mut message = format!(
        "Imported {} file(s) into {}",
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
        eprintln!("Import error: {err}");
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
    copy_file_to_source_folder(
        controller,
        source,
        Path::new(""),
        path,
        "paste",
        "pasted",
    )
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

fn import_files_into_source_folder(
    controller: &mut EguiController,
    source: &SampleSource,
    target_folder: &Path,
    paths: &[PathBuf],
) -> ImportResult {
    let mut errors = Vec::new();
    let mut added = 0usize;
    let mut last_relative = None;
    for path in paths {
        match import_file_to_source_folder(controller, source, target_folder, path) {
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
    ImportResult {
        added,
        errors,
    }
}

fn import_file_to_source_folder(
    controller: &mut EguiController,
    source: &SampleSource,
    target_folder: &Path,
    path: &Path,
) -> Result<PathBuf, String> {
    copy_file_to_source_folder(controller, source, target_folder, path, "import", "imported")
}

fn copy_file_to_source_folder(
    controller: &mut EguiController,
    source: &SampleSource,
    target_folder: &Path,
    path: &Path,
    action_label: &str,
    register_label: &str,
) -> Result<PathBuf, String> {
    if !source.root.is_dir() {
        return Err("Source folder is not available".into());
    }
    validate_relative_folder_path(target_folder)?;
    let target_root = source.root.join(target_folder);
    if !target_root.exists() {
        std::fs::create_dir_all(&target_root).map_err(|err| {
            format!(
                "Failed to create folder {}: {err}",
                target_root.display()
            )
        })?;
    } else if !target_root.is_dir() {
        return Err(format!(
            "Target folder is not a directory: {}",
            target_root.display()
        ));
    }
    let relative_name = unique_destination_name(&target_root, path)?;
    let relative = if target_folder.as_os_str().is_empty() {
        relative_name
    } else {
        target_folder.join(relative_name)
    };
    let absolute = source.root.join(&relative);
    std::fs::copy(path, &absolute).map_err(|err| {
        format!("Failed to {action_label} {}: {err}", path.display())
    })?;
    let result = (|| -> Result<(), String> {
        let (file_size, modified_ns) = file_metadata(&absolute)?;
        let db = controller
            .database_for(source)
            .map_err(|err| format!("Failed to open source DB: {err}"))?;
        db.upsert_file(&relative, file_size, modified_ns).map_err(|err| {
            format!("Failed to register {register_label} file: {err}")
        })?;
        controller.enqueue_similarity_for_new_sample(source, &relative, file_size, modified_ns);
        Ok(())
    })();
    match result {
        Ok(()) => Ok(relative),
        Err(err) => {
            if let Err(remove_err) = std::fs::remove_file(&absolute) {
                Err(format!(
                    "{err}; failed to remove {register_label} file {}: {remove_err}",
                    absolute.display()
                ))
            } else {
                Err(err)
            }
        }
    }
}

fn unique_destination_name(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| "File has no name".to_string())?;
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
    Err("Unable to find a unique destination name".into())
}

fn filter_supported_audio_paths(paths: Vec<PathBuf>) -> (Vec<PathBuf>, usize) {
    let mut supported = Vec::new();
    let mut skipped = 0usize;
    for path in paths {
        if !path.is_file() || !is_supported_audio(&path) {
            skipped += 1;
            continue;
        }
        supported.push(path);
    }
    (supported, skipped)
}

fn validate_relative_folder_path(path: &Path) -> Result<(), String> {
    if path.is_absolute() {
        return Err("Target folder must be a relative path".into());
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("Target folder cannot contain '..'".into());
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_relative_folder_path_blocks_parent_dirs() {
        assert!(validate_relative_folder_path(Path::new("..")).is_err());
        assert!(validate_relative_folder_path(Path::new("foo/../bar")).is_err());
    }

    #[test]
    fn validate_relative_folder_path_allows_relative() {
        assert!(validate_relative_folder_path(Path::new("")).is_ok());
        assert!(validate_relative_folder_path(Path::new("samples/drums")).is_ok());
    }
}
