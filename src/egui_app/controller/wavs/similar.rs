use super::*;
use crate::egui_app::view_model;

const DEFAULT_SIMILAR_COUNT: usize = 40;

pub(super) fn find_similar_for_visible_row(
    controller: &mut EguiController,
    visible_row: usize,
) -> Result<(), String> {
    let source_id = controller
        .selection_state
        .ctx
        .selected_source
        .clone()
        .ok_or_else(|| "No active source selected".to_string())?;
    let entry_index = controller
        .ui
        .browser
        .visible
        .get(visible_row)
        .copied()
        .ok_or_else(|| "Selected row is out of range".to_string())?;
    let entry = controller
        .wav_entries
        .entries
        .get(entry_index)
        .ok_or_else(|| "Sample entry missing".to_string())?;
    let sample_id = super::super::analysis_jobs::build_sample_id(
        source_id.as_str(),
        &entry.relative_path,
    );
    let db_path = crate::app_dirs::app_root_dir()
        .map_err(|err| err.to_string())?
        .join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
    let conn = super::super::analysis_jobs::open_library_db(&db_path)?;
    let neighbours =
        crate::analysis::ann_index::find_similar(&conn, &sample_id, DEFAULT_SIMILAR_COUNT)?;

    let mut indices = Vec::new();
    for neighbour in neighbours {
        let (candidate_source, relative_path) =
            super::super::analysis_jobs::parse_sample_id(&neighbour.sample_id)?;
        if candidate_source.as_str() != source_id.as_str() {
            continue;
        }
        if let Some(index) = controller.wav_entries.lookup.get(&relative_path) {
            indices.push(*index);
        }
    }
    if indices.is_empty() {
        return Err("No similar samples found in the current source".to_string());
    }
    controller.ui.browser.similar_query = Some(crate::egui_app::state::SimilarQuery {
        sample_id,
        label: view_model::sample_display_label(&entry.relative_path),
        indices,
    });
    controller.ui.browser.search_query.clear();
    controller.ui.browser.search_focus_requested = false;
    controller.rebuild_browser_lists();
    Ok(())
}

pub(super) fn clear_similar_filter(controller: &mut EguiController) {
    if controller.ui.browser.similar_query.take().is_some() {
        controller.rebuild_browser_lists();
    }
}

pub(super) fn find_similar_for_audio_path(
    controller: &mut EguiController,
    path: &Path,
) -> Result<(), String> {
    let source_id = controller
        .selection_state
        .ctx
        .selected_source
        .clone()
        .ok_or_else(|| "No active source selected".to_string())?;
    let decoded = crate::analysis::audio::decode_for_analysis(path)?;
    let processed = crate::analysis::audio::preprocess_mono_for_embedding(
        &decoded.mono,
        decoded.sample_rate_used,
    );
    let embedding =
        crate::analysis::embedding::infer_embedding_query(&processed, decoded.sample_rate_used)?;
    let db_path = crate::app_dirs::app_root_dir()
        .map_err(|err| err.to_string())?
        .join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
    let conn = super::super::analysis_jobs::open_library_db(&db_path)?;
    let neighbours =
        crate::analysis::ann_index::find_similar_for_embedding(&conn, &embedding, DEFAULT_SIMILAR_COUNT)?;

    let mut indices = Vec::new();
    for neighbour in neighbours {
        let (candidate_source, relative_path) =
            super::super::analysis_jobs::parse_sample_id(&neighbour.sample_id)?;
        if candidate_source.as_str() != source_id.as_str() {
            continue;
        }
        if let Some(index) = controller.wav_entries.lookup.get(&relative_path) {
            indices.push(*index);
        }
    }
    if indices.is_empty() {
        return Err("No similar samples found in the current source".to_string());
    }
    let label = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("Clip: {name}"))
        .unwrap_or_else(|| "Clip".to_string());
    controller.ui.browser.similar_query = Some(crate::egui_app::state::SimilarQuery {
        sample_id: format!("clip::{}", path.display()),
        label,
        indices,
    });
    controller.ui.browser.search_query.clear();
    controller.ui.browser.search_focus_requested = false;
    controller.rebuild_browser_lists();
    Ok(())
}
