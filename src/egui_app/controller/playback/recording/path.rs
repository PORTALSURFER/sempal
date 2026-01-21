use super::state::audio::RecordingTarget;
use super::*;
use std::path::{Path, PathBuf};
use time::format_description::FormatItem;
use time::macros::format_description;

pub(crate) fn next_recording_path_in_source(
    controller: &mut EguiController,
) -> Result<(SampleSource, PathBuf, PathBuf), String> {
    let source = controller
        .current_source()
        .ok_or_else(|| "Select a source to record into".to_string())?;
    let selected_folder = controller.selected_folder_paths().into_iter().next();
    let target_folder = resolve_recording_folder(&source.root, selected_folder)?;
    let base_name = format!("{RECORDING_FILE_PREFIX}{}", formatted_timestamp());
    let mut counter = 0_u32;
    let (relative_path, absolute_path) = loop {
        let suffix = if counter == 0 {
            String::new()
        } else {
            format!("_{counter}")
        };
        let filename = format!("{base_name}{suffix}.{RECORDING_FILE_EXT}");
        let relative_path = target_folder.join(filename);
        let absolute_path = source.root.join(&relative_path);
        if !absolute_path.exists() {
            break (relative_path, absolute_path);
        }
        counter += 1;
    };
    let absolute_path = ensure_recording_path(absolute_path)?;
    Ok((source, relative_path, absolute_path))
}

pub(crate) fn resolve_recording_target(
    controller: &mut EguiController,
    target: Option<&RecordingTarget>,
    recording_path: &PathBuf,
) -> Result<(SampleSource, PathBuf), String> {
    if let Some(target) = target
        && &target.absolute_path == recording_path
    {
        let source = source_by_id(controller, &target.source_id)
            .ok_or_else(|| "Recording source unavailable".to_string())?;
        return Ok((source, target.relative_path.clone()));
    }
    let source = ensure_recordings_source(controller, recording_path)?;
    let relative_path = recording_path
        .strip_prefix(&source.root)
        .map_err(|_| "Failed to resolve recording path".to_string())?
        .to_path_buf();
    Ok((source, relative_path))
}

pub(crate) fn register_recording_in_browser(
    controller: &mut EguiController,
    target: Option<&RecordingTarget>,
    recording_path: &PathBuf,
) -> Result<(), String> {
    let (source, relative_path) = resolve_recording_target(controller, target, recording_path)?;
    let (file_size, modified_ns) =
        crate::egui_app::controller::library::wav_io::file_metadata(recording_path)?;
    let db = controller
        .database_for(&source)
        .map_err(|err| format!("Database unavailable: {err}"))?;
    db.upsert_file(&relative_path, file_size, modified_ns)
        .map_err(|err| format!("Failed to register recording: {err}"))?;
    controller.enqueue_similarity_for_new_sample(&source, &relative_path, file_size, modified_ns);
    controller.selection_state.suppress_autoplay_once = true;
    controller.ui.browser.autoscroll = true;
    controller.focus_browser_context();
    controller
        .runtime
        .jobs
        .set_pending_select_path(Some(relative_path));
    controller.invalidate_wav_entries_for_source(&source);
    Ok(())
}

fn ensure_recordings_source(
    controller: &mut EguiController,
    recording_path: &PathBuf,
) -> Result<SampleSource, String> {
    let root = recording_path
        .parent()
        .ok_or_else(|| "Recording path missing parent".to_string())?
        .to_path_buf();
    if let Some(existing) = controller
        .library
        .sources
        .iter()
        .find(|s| s.root == root)
        .cloned()
    {
        controller.select_source(Some(existing.id.clone()));
        return Ok(existing);
    }
    let source = match crate::sample_sources::library::lookup_source_id_for_root(&root) {
        Ok(Some(id)) => SampleSource::new_with_id(id, root.clone()),
        Ok(None) => SampleSource::new(root.clone()),
        Err(err) => {
            controller.set_status(
                format!("Could not check library history (continuing): {err}"),
                StatusTone::Warning,
            );
            SampleSource::new(root.clone())
        }
    };
    SourceDatabase::open(&root)
        .map_err(|err| format!("Failed to create recordings database: {err}"))?;
    let _ = controller.cache_db(&source);
    controller.library.sources.push(source.clone());
    controller.select_source(Some(source.id.clone()));
    controller.persist_config("Failed to save config after adding recordings source")?;
    controller.prepare_similarity_for_selected_source();
    Ok(source)
}

fn resolve_recording_folder(
    source_root: &Path,
    selected_folder: Option<PathBuf>,
) -> Result<PathBuf, String> {
    let mut target_folder = selected_folder.unwrap_or_default();
    if target_folder.is_absolute() {
        target_folder = target_folder
            .strip_prefix(source_root)
            .map_err(|_| "Selected folder is outside the current source".to_string())?
            .to_path_buf();
    }
    Ok(target_folder)
}

fn ensure_recording_path(mut path: PathBuf) -> Result<PathBuf, String> {
    if path.extension().is_none() {
        path.set_extension(RECORDING_FILE_EXT);
    }
    let parent = path
        .parent()
        .ok_or_else(|| "Recording path missing parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|err| {
        format!(
            "Failed to create recordings folder {}: {err}",
            parent.display()
        )
    })?;
    Ok(path)
}

fn formatted_timestamp() -> String {
    const FORMAT: &[FormatItem<'_>] =
        format_description!("[year][month][day]_[hour][minute][second]");
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    now.format(&FORMAT).unwrap_or_else(|_| "unknown".into())
}

fn source_by_id(controller: &EguiController, source_id: &SourceId) -> Option<SampleSource> {
    controller
        .library
        .sources
        .iter()
        .find(|source| &source.id == source_id)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_app::controller::test_support::dummy_controller;
    use tempfile::tempdir;

    #[test]
    fn next_recording_path_generates_filename() {
        let (mut controller, source) = dummy_controller();
        controller.library.sources.push(source.clone());
        let (_, relative, absolute) = next_recording_path_in_source(&mut controller).unwrap();
        let file_name = relative.file_name().unwrap().to_string_lossy();
        assert!(file_name.starts_with(RECORDING_FILE_PREFIX));
        assert_eq!(
            relative.extension().and_then(|ext| ext.to_str()),
            Some(RECORDING_FILE_EXT)
        );
        assert!(absolute.starts_with(&source.root));
    }

    #[test]
    fn resolve_recording_folder_strips_source_root() {
        let root = tempdir().unwrap();
        let absolute = root.path().join("subdir");
        let resolved = resolve_recording_folder(root.path(), Some(absolute)).unwrap();
        assert_eq!(resolved, PathBuf::from("subdir"));
    }
}
