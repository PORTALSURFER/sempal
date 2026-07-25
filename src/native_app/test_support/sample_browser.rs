use crate::native_app::app::{GuiMessage, NativeAppState};
use crate::native_app::app_chrome::library_browser::sample_browser_view::{
    SampleFileHitTargetModel, sample_browser_from_state, sample_file_hit_target_for_tests,
};
use crate::native_app::app_chrome::view_models::sample_browser::{
    SampleBrowserViewModel, SampleBrowserViewProjection,
    prepare_sample_browser_view as prepare_chrome_sample_browser_view,
};
use radiant::prelude as ui;
use radiant::prelude::IntoView;
use radiant::runtime::UiSurface;
use std::collections::HashMap;
use wavecrate::sample_sources::{StarmapLayoutLoadResult, StarmapLayoutPoint};

pub(in crate::native_app) use crate::native_app::sample_library::folder_browser::view_contract::{
    DEFAULT_FOLDER_WIDTH, MAX_FOLDER_WIDTH, MIN_FOLDER_WIDTH,
};
pub(in crate::native_app) use crate::native_app::sample_library::sample_list::{
    SAMPLE_BROWSER_EDGE_CONTEXT_ROWS, SAMPLE_BROWSER_ROW_HEIGHT,
    SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
};

pub(in crate::native_app) const SAMPLE_FILE_HIT_TARGET_TEST_ID: u64 = 99_701;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct SampleBrowserWindowProjection {
    pub(in crate::native_app) total_items: usize,
    pub(in crate::native_app) total_count: usize,
    pub(in crate::native_app) visible_rows: usize,
    pub(in crate::native_app) window_len: usize,
    pub(in crate::native_app) first_stems: Vec<String>,
}

pub(in crate::native_app) fn sample_browser(state: &NativeAppState) -> ui::View<GuiMessage> {
    sample_browser_from_state(state)
}

pub(in crate::native_app) fn prepare_sample_browser_view(state: &mut NativeAppState) {
    prepare_chrome_sample_browser_view(state);
}

pub(in crate::native_app) fn complete_starmap_layout(
    state: &mut NativeAppState,
    points_by_file: HashMap<String, StarmapLayoutPoint>,
) {
    let tags_by_file = state.metadata.tags_by_file.clone();
    state
        .library
        .folder_browser
        .prepare_starmap_layout(&tags_by_file);
    let Some(request) = state
        .library
        .folder_browser
        .take_starmap_layout_load_request(&tags_by_file)
    else {
        return;
    };
    state
        .library
        .folder_browser
        .apply_starmap_layout_load_result(StarmapLayoutLoadResult {
            signature: request.signature,
            result: Ok(points_by_file),
        });
}

pub(in crate::native_app) fn complete_starmap_layout_for_selected_source(
    state: &mut NativeAppState,
) -> Vec<String> {
    let file_ids = state
        .library
        .folder_browser
        .selected_source_audio_files()
        .into_iter()
        .map(|file| file.id.clone())
        .collect::<Vec<_>>();
    let total = file_ids.len().max(1) as f32;
    let points_by_file = file_ids
        .iter()
        .enumerate()
        .map(|(index, file_id)| {
            let t = (index as f32 + 0.5) / total;
            (
                file_id.clone(),
                StarmapLayoutPoint {
                    x: 0.10 + 0.80 * t,
                    y: 0.50 + ((index % 7) as f32 - 3.0) * 0.035,
                    cluster_id: None,
                },
            )
        })
        .collect();
    complete_starmap_layout(state, points_by_file);
    file_ids
}

pub(in crate::native_app) fn sample_file_hit_target(
    path: String,
    selected: bool,
    drag_active: bool,
    drag_source: bool,
    cached: bool,
) -> UiSurface<GuiMessage> {
    sample_file_hit_target_for_tests(
        ui::empty(),
        SampleFileHitTargetModel {
            file_id: "sample.wav",
            selected,
            focused: false,
            focus_alpha: 0,
            selection_flash: false,
            copy_flash: false,
            protected_source_error_flash: false,
            cut_pending: false,
            drag_active,
            drag_source,
            cached,
            missing: false,
            hit_path: path,
            help_tooltips_enabled: false,
        },
        SAMPLE_FILE_HIT_TARGET_TEST_ID,
    )
    .size(160.0, SAMPLE_BROWSER_ROW_HEIGHT)
    .into_surface()
}

pub(in crate::native_app) fn sample_browser_window_projection(
    state: &NativeAppState,
    take: usize,
) -> SampleBrowserWindowProjection {
    let model = SampleBrowserViewModel::from_projection(
        SampleBrowserViewProjection::from_prepared_app_state(state),
    );
    SampleBrowserWindowProjection {
        total_items: model.visible_samples.window.total_items,
        total_count: model.visible_samples.total_count,
        visible_rows: model.visible_samples.rows.len(),
        window_len: model.visible_samples.window.window_len(),
        first_stems: model
            .visible_samples
            .rows
            .iter()
            .take(take)
            .map(|row| row.file.stem.clone())
            .collect(),
    }
}
