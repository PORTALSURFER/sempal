use super::*;
use crate::egui_app::view_model;

mod apply;
mod query;
mod resolve;

const DEFAULT_SIMILAR_COUNT: usize = 40;
const SIMILAR_RE_RANK_CANDIDATES: usize = 200;
const EMBED_WEIGHT: f32 = 0.8;
const DSP_WEIGHT: f32 = 0.2;
const DUPLICATE_SCORE_THRESHOLD: f32 = 0.995;
const DUPLICATE_RMS_MIN: f32 = 1.0e-4;
const FEATURE_RMS_INDEX: usize = 2;
const MISSING_SIMILARITY_SCORE: f32 = -2.0;

pub(crate) fn find_similar_for_visible_row(
    controller: &mut EguiController,
    visible_row: usize,
) -> Result<(), String> {
    let (sample_id, entry_index) =
        resolve::resolve_sample_id_for_visible_row(controller, visible_row)?;
    apply_similarity_for_sample_id(
        controller,
        &sample_id,
        None,
        |path| view_model::sample_display_label(path),
        Some(entry_index),
        "No similar samples found in the current source",
    )
}

pub(crate) fn find_duplicates_for_visible_row(
    controller: &mut EguiController,
    visible_row: usize,
) -> Result<(), String> {
    let (sample_id, entry_index) =
        resolve::resolve_sample_id_for_visible_row(controller, visible_row)?;
    apply_similarity_for_sample_id(
        controller,
        &sample_id,
        Some(DUPLICATE_SCORE_THRESHOLD),
        |path| format!("Duplicates of {}", view_model::sample_display_label(path)),
        Some(entry_index),
        "No duplicates found in the current source",
    )
}

pub(crate) fn find_similar_for_sample_id(
    controller: &mut EguiController,
    sample_id: &str,
) -> Result<(), String> {
    apply_similarity_for_sample_id(
        controller,
        sample_id,
        None,
        |path| view_model::sample_display_label(path),
        None,
        "No similar samples found in the current source",
    )
}

pub(crate) fn clear_similar_filter(controller: &mut EguiController) {
    apply::clear_similar_filter(controller);
}

fn apply_similarity_for_sample_id(
    controller: &mut EguiController,
    sample_id: &str,
    score_cutoff: Option<f32>,
    label_builder: impl FnOnce(&Path) -> String,
    anchor_override: Option<usize>,
    empty_error: &str,
) -> Result<(), String> {
    let query = query::build_similar_query_for_sample_id(
        controller,
        sample_id,
        score_cutoff,
        label_builder,
        anchor_override,
        empty_error,
    )?;
    apply::apply_similarity_query(controller, query);
    Ok(())
}

pub(crate) fn find_similar_for_audio_path(
    controller: &mut EguiController,
    path: &Path,
) -> Result<(), String> {
    let query = query::build_similarity_query_for_audio_path(controller, path)?;
    apply::apply_similarity_query(controller, query);
    Ok(())
}

pub(crate) fn enable_loaded_similarity_sort(controller: &mut EguiController) -> Result<(), String> {
    let query = query::build_similarity_query_for_loaded_sample(controller)?;
    apply::apply_similarity_query(controller, query);
    controller.ui.browser.similarity_sort_follow_loaded = true;
    Ok(())
}

pub(crate) fn disable_similarity_sort(controller: &mut EguiController) {
    apply::disable_similarity_sort(controller);
}

pub(crate) fn refresh_similarity_sort_for_loaded(
    controller: &mut EguiController,
) -> Result<(), String> {
    if !controller.ui.browser.similarity_sort_follow_loaded {
        return Ok(());
    }
    if controller.ui.browser.sort != SampleBrowserSort::Similarity {
        return Ok(());
    }
    if controller.ui.browser.similar_query.is_some() {
        return Ok(());
    }
    let query = query::build_similarity_query_for_loaded_sample(controller)?;
    controller.ui.browser.similar_query = Some(query);
    controller.rebuild_browser_lists();
    Ok(())
}
