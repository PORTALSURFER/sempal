use std::{collections::HashMap, path::Path, sync::Arc};

use crate::native_app::app::{
    NativeAppState, SampleBrowserDisplayMode, SampleNameViewMode, StarmapAuditionDragState,
    StarmapViewport,
};
use crate::native_app::sample_library::folder_browser::projection::{
    FileColumnDragFeedback, VisibleSampleList, VisibleSampleQuery, VisibleSampleWindowPolicy,
};
use crate::native_app::sample_library::folder_browser::starmap::{
    StarmapItem, StarmapProjection, StarmapStatus,
};
use crate::native_app::sample_library::sample_list::{
    SAMPLE_BROWSER_EDGE_CONTEXT_ROWS, SAMPLE_BROWSER_OVERSCAN_ROWS,
    SAMPLE_BROWSER_PROJECTED_VIEWPORT_ROWS,
};

pub(in crate::native_app) struct SampleBrowserViewModel<'a> {
    pub(in crate::native_app) visible_samples: VisibleSampleList<'a>,
    pub(in crate::native_app) map_items: Arc<[StarmapItem]>,
    pub(in crate::native_app) map_status: StarmapStatus,
    pub(in crate::native_app) map_prep_running: bool,
    pub(in crate::native_app) map_audition_drag: Option<StarmapAuditionDragState>,
    pub(in crate::native_app) map_active_audition_file_id: Option<String>,
    pub(in crate::native_app) map_viewport: StarmapViewport,
    pub(in crate::native_app) name_filter: String,
    pub(in crate::native_app) display_mode: SampleBrowserDisplayMode,
    pub(in crate::native_app) name_view_mode: SampleNameViewMode,
    pub(in crate::native_app) random_navigation_enabled: bool,
    pub(in crate::native_app) curation_mode_enabled: bool,
    pub(in crate::native_app) metadata_tags_by_file: &'a HashMap<String, Vec<String>>,
    pub(in crate::native_app) cut_file_ids: Option<&'a [String]>,
    pub(in crate::native_app) file_drag_active: bool,
    pub(in crate::native_app) extracted_file_drag_active: bool,
    pub(in crate::native_app) hovered_folder_drop_target: bool,
    pub(in crate::native_app) drag_feedback: Option<FileColumnDragFeedback>,
    pub(in crate::native_app) help_tooltips_enabled: bool,
}

pub(in crate::native_app) struct SampleBrowserViewProjection<'a> {
    visible_samples: VisibleSampleList<'a>,
    map_items: Arc<[StarmapItem]>,
    map_status: StarmapStatus,
    map_prep_running: bool,
    map_audition_drag: Option<StarmapAuditionDragState>,
    map_active_audition_file_id: Option<String>,
    map_viewport: StarmapViewport,
    name_filter: String,
    display_mode: SampleBrowserDisplayMode,
    name_view_mode: SampleNameViewMode,
    random_navigation_enabled: bool,
    curation_mode_enabled: bool,
    metadata_tags_by_file: &'a HashMap<String, Vec<String>>,
    cut_file_ids: Option<&'a [String]>,
    file_drag_active: bool,
    extracted_file_drag_active: bool,
    hovered_folder_drop_target: bool,
    drag_feedback: Option<FileColumnDragFeedback>,
    help_tooltips_enabled: bool,
}

#[derive(Clone, Copy)]
pub(in crate::native_app) struct SampleBrowserFramePreparationState {
    copy_flash_active: bool,
    selection_flash_active: bool,
    startup_auto_load_pending: bool,
}

impl SampleBrowserFramePreparationState {
    pub(in crate::native_app) fn capture(state: &NativeAppState) -> Self {
        Self {
            copy_flash_active: state.library.folder_browser.copy_flash_active(),
            selection_flash_active: state.library.folder_browser.selection_flash_active(),
            startup_auto_load_pending: state.ui.startup.auto_load_pending,
        }
    }

    pub(in crate::native_app) fn requires_preparation(self, state: &NativeAppState) -> bool {
        if live_drag_defers_sample_browser_preparation(state) {
            return false;
        }
        state
            .library
            .folder_browser
            .visible_sample_window_needs_content_refresh()
            || self.copy_flash_active != state.library.folder_browser.copy_flash_active()
            || self.selection_flash_active != state.library.folder_browser.selection_flash_active()
            || self.startup_auto_load_pending != state.ui.startup.auto_load_pending
            || (state.ui.chrome.sample_browser_display == SampleBrowserDisplayMode::Map
                && !state.library.folder_browser.starmap_projection_prepared())
    }
}

impl<'a> SampleBrowserViewProjection<'a> {
    pub(in crate::native_app) fn from_prepared_app_state(state: &'a NativeAppState) -> Self {
        let file_drag_active = state.library.folder_browser.file_drag_active();
        let extracted_file_drag_active = state.library.folder_browser.extracted_file_drag_active();
        let hovered_folder_drop_target = state
            .library
            .folder_browser
            .hovered_drop_target_folder_id()
            .is_some();
        let drag_feedback = state.library.folder_browser.file_column_drag_feedback();
        let display_mode = state.ui.chrome.sample_browser_display;
        let waveform_drag_active = state.waveform.current.active_drag_kind().is_some();
        let starmap_audition_drag_active = state.ui.chrome.starmap_audition_drag.is_some();
        let visible_samples = state
            .library
            .folder_browser
            .visible_samples(VisibleSampleQuery {
                tags_by_file: &state.metadata.tags_by_file,
                cached_sample_paths: &state.waveform.cache.cached_sample_paths,
            });
        let map_items = starmap_items_for_display(
            state,
            display_mode,
            waveform_drag_active || starmap_audition_drag_active,
        );

        Self {
            visible_samples,
            map_items,
            map_status: state.library.folder_browser.starmap_status(),
            map_prep_running: state
                .background
                .source_processing_progress
                .as_ref()
                .is_some_and(|progress| progress.active),
            map_audition_drag: state.ui.chrome.starmap_audition_drag.clone(),
            map_active_audition_file_id: active_starmap_audition_file_id(state),
            map_viewport: state.ui.chrome.starmap_viewport,
            name_filter: state.library.folder_browser.name_filter().to_owned(),
            display_mode,
            name_view_mode: state.metadata.sample_name_view_mode,
            random_navigation_enabled: state.library.folder_browser.random_navigation_enabled(),
            curation_mode_enabled: state.library.folder_browser.curation_mode_enabled(),
            metadata_tags_by_file: &state.metadata.tags_by_file,
            cut_file_ids: state
                .ui
                .browser_interaction
                .cut_file_clipboard
                .as_ref()
                .map(|clipboard| clipboard.file_ids.as_slice()),
            file_drag_active,
            extracted_file_drag_active,
            hovered_folder_drop_target,
            drag_feedback,
            help_tooltips_enabled: state.ui.chrome.help_tooltips_enabled,
        }
    }
}

impl<'a> SampleBrowserViewModel<'a> {
    pub(in crate::native_app) fn from_projection(
        projection: SampleBrowserViewProjection<'a>,
    ) -> Self {
        Self {
            visible_samples: projection.visible_samples,
            map_items: projection.map_items,
            map_status: projection.map_status,
            map_prep_running: projection.map_prep_running,
            map_audition_drag: projection.map_audition_drag,
            map_active_audition_file_id: projection.map_active_audition_file_id,
            map_viewport: projection.map_viewport,
            name_filter: projection.name_filter,
            display_mode: projection.display_mode,
            name_view_mode: projection.name_view_mode,
            random_navigation_enabled: projection.random_navigation_enabled,
            curation_mode_enabled: projection.curation_mode_enabled,
            metadata_tags_by_file: projection.metadata_tags_by_file,
            cut_file_ids: projection.cut_file_ids,
            file_drag_active: projection.file_drag_active,
            extracted_file_drag_active: projection.extracted_file_drag_active,
            hovered_folder_drop_target: projection.hovered_folder_drop_target,
            drag_feedback: projection.drag_feedback,
            help_tooltips_enabled: projection.help_tooltips_enabled,
        }
    }
}

fn active_starmap_audition_file_id(state: &NativeAppState) -> Option<String> {
    state
        .ui
        .chrome
        .starmap_audition_drag
        .as_ref()
        .and_then(|drag| drag.last_hit_file_id.clone())
        .or_else(|| {
            state
                .ui
                .chrome
                .starmap_audition_queue
                .active_file_id
                .clone()
        })
        .or_else(|| {
            state
                .audio
                .sample_playback_session
                .as_ref()
                .filter(|session| session.request.origin == "starmap_drag")
                .map(|session| session.request.path.clone())
        })
}

pub(in crate::native_app) fn prepare_sample_browser_view(state: &mut NativeAppState) {
    if live_drag_defers_sample_browser_preparation(state) {
        return;
    }
    state
        .library
        .folder_browser
        .prepare_visible_sample_window(VisibleSampleWindowPolicy {
            tags_by_file: &state.metadata.tags_by_file,
            viewport_rows: SAMPLE_BROWSER_PROJECTED_VIEWPORT_ROWS,
            overscan_rows: SAMPLE_BROWSER_OVERSCAN_ROWS,
            guard_rows: SAMPLE_BROWSER_EDGE_CONTEXT_ROWS,
        });
    let visible_paths = state
        .library
        .folder_browser
        .prepared_visible_sample_file_ids_matching_tags(
            &state.metadata.tags_by_file,
            SAMPLE_BROWSER_PROJECTED_VIEWPORT_ROWS
                + SAMPLE_BROWSER_OVERSCAN_ROWS
                + SAMPLE_BROWSER_EDGE_CONTEXT_ROWS,
        )
        .unwrap_or_default()
        .into_iter()
        .filter_map(|file_id| {
            state
                .library
                .folder_browser
                .sample_source_for_file_path(Path::new(&file_id))
        })
        .map(|(source, relative_path)| {
            (
                source.id.as_str().to_owned(),
                relative_path.to_string_lossy().into_owned(),
            )
        });
    state
        .background
        .source_processing
        .set_visible_paths(visible_paths);
    if state.ui.chrome.sample_browser_display == SampleBrowserDisplayMode::Map {
        state
            .library
            .folder_browser
            .prepare_starmap_layout(&state.metadata.tags_by_file);
        state
            .library
            .folder_browser
            .prepare_starmap_projection(StarmapProjection {
                tags_by_file: &state.metadata.tags_by_file,
                instant_audition_sample_paths: &state.waveform.cache.instant_audition_sample_paths,
                preview_audition_sample_paths: state.waveform.cache.preview_audition_sample_paths(),
            });
    }
}

fn starmap_items_for_display(
    state: &NativeAppState,
    display_mode: SampleBrowserDisplayMode,
    waveform_drag_active: bool,
) -> Arc<[StarmapItem]> {
    if display_mode != SampleBrowserDisplayMode::Map {
        return empty_starmap_items();
    }
    let cached = state.library.folder_browser.cached_starmap_projection();
    if let Some(cached) = cached {
        return cached;
    }
    if waveform_drag_active {
        return empty_starmap_items();
    }
    state
        .library
        .folder_browser
        .starmap_projection(StarmapProjection {
            tags_by_file: &state.metadata.tags_by_file,
            instant_audition_sample_paths: &state.waveform.cache.instant_audition_sample_paths,
            preview_audition_sample_paths: state.waveform.cache.preview_audition_sample_paths(),
        })
        .into()
}

fn empty_starmap_items() -> Arc<[StarmapItem]> {
    Arc::from(Vec::<StarmapItem>::new())
}

fn waveform_drag_defers_sample_browser_preparation(state: &NativeAppState) -> bool {
    state.waveform.current.active_drag_kind().is_some()
        && !state
            .library
            .folder_browser
            .visible_sample_window_needs_content_refresh()
}

fn starmap_audition_drag_defers_sample_browser_preparation(state: &NativeAppState) -> bool {
    state.ui.chrome.starmap_audition_drag.is_some()
        && !state
            .library
            .folder_browser
            .visible_sample_window_needs_content_refresh()
}

fn live_drag_defers_sample_browser_preparation(state: &NativeAppState) -> bool {
    waveform_drag_defers_sample_browser_preparation(state)
        || starmap_audition_drag_defers_sample_browser_preparation(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_app::test_support::state::{NativeAppStateFixture, WaveformInteraction};
    use crate::native_app::waveform::WaveformSelectionKind;
    use crate::native_app::{
        app::SampleBrowserDisplayMode, sample_library::folder_browser::FolderBrowserState,
    };
    use radiant::{
        gui::types::Point,
        widgets::{PointerModifiers, TextInputMessage},
    };
    use std::fs;

    #[test]
    fn list_mode_projection_does_not_build_starmap_items() {
        let state = NativeAppStateFixture::default()
            .with_synthetic_waveform()
            .build();

        let projection = SampleBrowserViewProjection::from_prepared_app_state(&state);

        assert!(projection.map_items.is_empty());
    }

    #[test]
    fn active_waveform_drag_defers_sample_browser_preparation() {
        let mut state = NativeAppStateFixture::default()
            .with_synthetic_waveform()
            .build();

        state
            .waveform
            .current
            .apply_interaction(WaveformInteraction::BeginSelection {
                kind: WaveformSelectionKind::Play,
                visible_ratio: 0.25,
            });

        assert!(waveform_drag_defers_sample_browser_preparation(&state));
    }

    #[test]
    fn active_waveform_drag_allows_sample_browser_preparation_after_file_refresh() {
        let root = tempfile::tempdir().expect("source root");
        let source = root.path().join("source");
        fs::create_dir_all(&source).expect("create source folder");
        let original = source.join("original.wav");
        let extracted = source.join("original_extraction.wav");
        fs::write(&original, [0_u8; 8]).expect("write original");
        let mut state = NativeAppStateFixture::default()
            .with_folder_browser(FolderBrowserState::from_root(source.clone()))
            .with_synthetic_waveform()
            .build();
        state.waveform.current.set_play_selection_range(0.2, 0.4);
        prepare_sample_browser_view(&mut state);

        state
            .waveform
            .current
            .apply_interaction(WaveformInteraction::BeginSelectionMove {
                kind: WaveformSelectionKind::Play,
                visible_ratio: 0.25,
            });
        fs::write(&extracted, [1_u8; 8]).expect("write extraction");
        assert!(state.library.folder_browser.refresh_file_path(&extracted));

        assert!(!waveform_drag_defers_sample_browser_preparation(&state));
        prepare_sample_browser_view(&mut state);
        let projection = SampleBrowserViewProjection::from_prepared_app_state(&state);

        assert!(
            projection
                .visible_samples
                .rows
                .iter()
                .any(|row| row.file.id == extracted.to_string_lossy().as_ref()),
            "extracted row should be visible before the active playmark drag ends"
        );
        assert!(waveform_drag_defers_sample_browser_preparation(&state));
    }

    #[test]
    fn active_waveform_drag_reuses_cached_starmap_projection() {
        let root = tempfile::tempdir().expect("source root");
        let source = root.path().join("source");
        fs::create_dir_all(&source).expect("create source folder");
        let kick = source.join("kick.wav");
        let snare = source.join("snare.wav");
        fs::write(&kick, [0_u8; 8]).expect("write kick");
        fs::write(&snare, [1_u8; 8]).expect("write snare");
        let mut state = NativeAppStateFixture::default()
            .with_folder_browser(FolderBrowserState::from_root(source.clone()))
            .with_synthetic_waveform()
            .build();
        state.ui.chrome.sample_browser_display = SampleBrowserDisplayMode::Map;
        crate::native_app::test_support::sample_browser::complete_starmap_layout_for_selected_source(
            &mut state,
        );
        prepare_sample_browser_view(&mut state);
        let before_drag_ids = {
            let before_drag = SampleBrowserViewProjection::from_prepared_app_state(&state);
            assert_eq!(before_drag.map_items.len(), 2);
            before_drag
                .map_items
                .iter()
                .map(|item| item.file_id.clone())
                .collect::<Vec<_>>()
        };

        state
            .waveform
            .current
            .apply_interaction(WaveformInteraction::BeginSelection {
                kind: WaveformSelectionKind::Play,
                visible_ratio: 0.25,
            });
        state
            .library
            .folder_browser
            .apply_name_filter_input(TextInputMessage::Changed {
                value: String::from("does-not-match"),
            });
        assert!(waveform_drag_defers_sample_browser_preparation(&state));
        prepare_sample_browser_view(&mut state);
        let during_drag = SampleBrowserViewProjection::from_prepared_app_state(&state);

        assert_eq!(
            during_drag
                .map_items
                .iter()
                .map(|item| item.file_id.clone())
                .collect::<Vec<_>>(),
            before_drag_ids,
            "live playmark drags should reuse the prepared starmap projection instead of rebuilding it"
        );

        state
            .waveform
            .current
            .apply_interaction(WaveformInteraction::FinishSelection {
                visible_ratio: 0.45,
            });
        prepare_sample_browser_view(&mut state);
        let after_drag = SampleBrowserViewProjection::from_prepared_app_state(&state);
        assert!(
            after_drag.map_items.is_empty(),
            "starmap projection should refresh normally after the live playmark drag ends"
        );
    }

    #[test]
    fn active_starmap_audition_drag_reuses_cached_starmap_projection() {
        let root = tempfile::tempdir().expect("source root");
        let source = root.path().join("source");
        fs::create_dir_all(&source).expect("create source folder");
        let kick = source.join("kick.wav");
        let snare = source.join("snare.wav");
        fs::write(&kick, [0_u8; 8]).expect("write kick");
        fs::write(&snare, [1_u8; 8]).expect("write snare");
        let snare_id = snare.to_string_lossy().into_owned();
        let mut state = NativeAppStateFixture::default()
            .with_folder_browser(FolderBrowserState::from_root(source.clone()))
            .build();
        state.ui.chrome.sample_browser_display = SampleBrowserDisplayMode::Map;
        crate::native_app::test_support::sample_browser::complete_starmap_layout_for_selected_source(
            &mut state,
        );
        prepare_sample_browser_view(&mut state);
        let before_drag = SampleBrowserViewProjection::from_prepared_app_state(&state);
        let before_drag_items = before_drag.map_items.clone();
        let before_drag_ids = before_drag
            .map_items
            .iter()
            .map(|item| item.file_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(before_drag_items.len(), 2);

        state.ui.chrome.starmap_audition_drag = Some(StarmapAuditionDragState {
            last_hit_file_id: Some(snare_id.clone()),
            last_position: Point::new(90.0, 50.0),
            modifiers: PointerModifiers::default(),
        });
        state
            .library
            .folder_browser
            .select_known_starmap_file_for_audition(snare_id);
        state
            .library
            .folder_browser
            .apply_name_filter_input(TextInputMessage::Changed {
                value: String::from("does-not-match"),
            });
        assert!(starmap_audition_drag_defers_sample_browser_preparation(
            &state
        ));
        prepare_sample_browser_view(&mut state);
        let during_drag = SampleBrowserViewProjection::from_prepared_app_state(&state);

        assert!(Arc::ptr_eq(&during_drag.map_items, &before_drag_items));
        assert_eq!(
            during_drag
                .map_items
                .iter()
                .map(|item| item.file_id.clone())
                .collect::<Vec<_>>(),
            before_drag_ids,
            "starmap audition drags should reuse the prepared projection instead of rebuilding it for live focus churn"
        );

        state.ui.chrome.starmap_audition_drag = None;
        prepare_sample_browser_view(&mut state);
        let after_drag = SampleBrowserViewProjection::from_prepared_app_state(&state);
        assert!(
            after_drag.map_items.is_empty(),
            "starmap projection should refresh normally after the audition drag ends"
        );
    }

    #[test]
    fn idle_waveform_allows_sample_browser_preparation() {
        let state = NativeAppStateFixture::default()
            .with_synthetic_waveform()
            .build();

        assert!(!waveform_drag_defers_sample_browser_preparation(&state));
    }

    #[test]
    fn map_mode_projection_reuses_prepared_starmap_items() {
        let root = tempfile::tempdir().expect("source root");
        let source = root.path().join("source");
        fs::create_dir_all(&source).expect("create source folder");
        fs::write(source.join("kick.wav"), [0_u8; 8]).expect("write kick");
        fs::write(source.join("snare.wav"), [1_u8; 8]).expect("write snare");
        let mut state = NativeAppStateFixture::default()
            .with_folder_browser(FolderBrowserState::from_root(source))
            .build();
        state.ui.chrome.sample_browser_display = SampleBrowserDisplayMode::Map;
        crate::native_app::test_support::sample_browser::complete_starmap_layout_for_selected_source(
            &mut state,
        );

        prepare_sample_browser_view(&mut state);
        let cached = state
            .library
            .folder_browser
            .cached_starmap_projection()
            .expect("prepared starmap projection");
        let projection = SampleBrowserViewProjection::from_prepared_app_state(&state);

        assert!(Arc::ptr_eq(&projection.map_items, &cached));
        assert_eq!(projection.map_items.len(), 2);
    }
}
