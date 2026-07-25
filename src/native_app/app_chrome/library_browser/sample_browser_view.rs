use radiant::prelude as ui;

use crate::native_app::app::GuiMessage;
#[cfg(test)]
use crate::native_app::app::NativeAppState;
use crate::native_app::app::SampleBrowserDisplayMode;
#[cfg(test)]
use crate::native_app::app_chrome::palette::ACCENT;
use crate::native_app::app_chrome::view_models::sample_browser::SampleBrowserViewModel;
#[cfg(test)]
use crate::native_app::app_chrome::view_models::sample_browser::SampleBrowserViewProjection;
use crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage;
use wavecrate_analysis::aspects::SimilarityAspect;

mod header;
mod hit_target;
#[cfg(test)]
pub(in crate::native_app) use hit_target::{
    SampleFileHitTargetModel, sample_file_hit_target_for_tests,
};
mod cells;
mod identity;
mod row_projection;
mod row_widgets;
mod rows;
mod starmap_view;
use header::{SampleBrowserHeaderBar, sample_browser_header_bar, sample_similarity_controls_bar};
use rows::sample_browser_rows;
pub(in crate::native_app) use starmap_view::paint_active_starmap_audition_overlay;
use starmap_view::starmap_view;

pub(super) const SAMPLE_SIMILARITY_SCORE_COLUMN_WIDTH: f32 = 190.0;

#[cfg(test)]
pub(in crate::native_app) fn sample_browser_from_state(
    state: &NativeAppState,
) -> ui::View<GuiMessage> {
    sample_browser(SampleBrowserViewModel::from_projection(
        SampleBrowserViewProjection::from_prepared_app_state(state),
    ))
}

pub(in crate::native_app) fn sample_browser(
    model: SampleBrowserViewModel<'_>,
) -> ui::View<GuiMessage> {
    let mut sections = Vec::with_capacity(3);
    sections.push(sample_browser_header_bar(SampleBrowserHeaderBar {
        columns: model.visible_samples.columns.as_slice(),
        sort: model.visible_samples.sort,
        drag_feedback: model.drag_feedback.as_ref(),
        mode: model.name_view_mode,
        random_navigation_enabled: model.random_navigation_enabled,
        map_view_active: model.display_mode == SampleBrowserDisplayMode::Map,
        similarity_mode_active: model.visible_samples.similarity_mode_active,
        similarity_controls: model.visible_samples.similarity_controls,
        help_tooltips_enabled: model.help_tooltips_enabled,
    }));
    if model.visible_samples.similarity_mode_active {
        sections.push(sample_similarity_controls_bar(
            model.visible_samples.similarity_controls,
        ));
    }
    sections.push(match model.display_mode {
        SampleBrowserDisplayMode::List => sample_browser_rows(
            &model.visible_samples,
            model.name_view_mode,
            model.curation_mode_enabled,
            model.metadata_tags_by_file,
            model.cut_file_ids,
            model.help_tooltips_enabled,
        )
        .fill(),
        SampleBrowserDisplayMode::Map => starmap_view(
            model.map_items,
            model.map_viewport,
            model.name_filter,
            model.visible_samples.similarity_controls,
            model.map_status,
            model.map_prep_running,
            model.curation_mode_enabled,
            model.map_audition_drag,
            model.map_active_audition_file_id,
        )
        .fill(),
    });
    ui::column(sections)
        .spacing(0.0)
        .fill()
        .pointer_target_if(
            model.file_drag_active,
            sample_list_browser_drag_cancel_target,
        )
        .pointer_target_if(
            model.extracted_file_drag_active,
            sample_list_waveform_drop_target,
        )
        .pointer_target_if(
            model.hovered_folder_drop_target,
            sample_list_clear_folder_drop_target,
        )
}

fn sample_list_browser_drag_cancel_target() -> ui::PointerTarget<GuiMessage> {
    ui::pointer_target(true)
        .pointer_move(false)
        .pointer_press(false)
        .wheel(false)
        .filter_map(|message| match message {
            ui::PointerShieldMessage::PointerRelease { .. }
            | ui::PointerShieldMessage::PointerDrop { .. } => {
                Some(GuiMessage::CancelBrowserDragOnSampleList)
            }
            ui::PointerShieldMessage::PointerMove { .. }
            | ui::PointerShieldMessage::PointerPress { .. }
            | ui::PointerShieldMessage::Wheel { .. } => None,
        })
        .key("sample-list-browser-drag-cancel-target")
}

fn sample_list_waveform_drop_target() -> ui::PointerTarget<GuiMessage> {
    ui::pointer_drop_target(true)
        .on_drop(GuiMessage::DropWaveformSelectionOnSampleList)
        .key("sample-list-waveform-drop-target")
}

fn sample_list_clear_folder_drop_target() -> ui::PointerTarget<GuiMessage> {
    ui::pointer_move_target(true)
        .on_pointer_move(|position| {
            GuiMessage::FolderBrowser(FolderBrowserMessage::ClearDropTarget(position))
        })
        .key("sample-list-clear-folder-drop-target")
}

pub(super) fn similarity_aspect_color(aspect: SimilarityAspect) -> ui::Rgba8 {
    match aspect {
        SimilarityAspect::Overall => ui::Rgba8::new(105, 172, 116, 230),
        SimilarityAspect::Spectrum => ui::Rgba8::new(233, 211, 98, 235),
        SimilarityAspect::Timbre => ui::Rgba8::new(235, 149, 73, 235),
        SimilarityAspect::Pitch => ui::Rgba8::new(226, 82, 111, 235),
        SimilarityAspect::Amplitude => ui::Rgba8::new(93, 158, 221, 235),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use radiant::{
        layout::Vector2,
        prelude::IntoView,
        runtime::{DeclarativeOwnedRuntimeBridge, SurfaceRuntime},
        theme::ThemeTokens,
        widgets::{PointerModifiers, WidgetInput},
    };
    use wavecrate::sample_sources::config::SimilarityAspectSettings;

    use super::*;
    use crate::native_app::app::{SampleNameViewMode, StarmapViewportChange};
    use crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage;
    use crate::native_app::sample_library::folder_browser::projection::VisibleSampleList;
    use crate::native_app::sample_library::folder_browser::starmap::{
        StarmapItem, StarmapStatus, starmap_cluster_palette_color,
    };
    use crate::native_app::ui::ids as widget_ids;

    #[test]
    fn similarity_anchor_gutter_paints_no_boxed_column_chrome() {
        let frame =
            super::cells::similarity_anchor_toggle(String::from("sample.wav"), false, None, false)
                .view_frame_at_size_with_default_theme(Vector2::new(22.0, 22.0));

        assert_eq!(frame.paint_plan.svgs().count(), 1);
        assert!(frame.paint_plan.fill_rects().next().is_none());
        assert!(frame.paint_plan.stroke_rects().next().is_none());
    }

    #[test]
    fn sample_list_stacked_pointer_targets_route_each_event_to_matching_domain_action() {
        let metadata_tags_by_file = HashMap::<String, Vec<String>>::new();
        let sort = radiant::application::DetailsSort::new(
            "name",
            radiant::application::SortDirection::Ascending,
        );
        let similarity_controls = SimilarityAspectSettings::default();
        let columns = Vec::new();
        let position = ui::Point::new(12.0, 12.0);

        let bridge = DeclarativeOwnedRuntimeBridge::new(
            Vec::<GuiMessage>::new(),
            |_| {
                sample_browser(SampleBrowserViewModel {
                    visible_samples: VisibleSampleList {
                        total_count: 0,
                        window: ui::VirtualListWindow::default(),
                        rows: Vec::new(),
                        columns: columns.clone(),
                        sort: &sort,
                        similarity_mode_active: false,
                        similarity_controls: &similarity_controls,
                    },
                    map_items: Vec::<StarmapItem>::new().into(),
                    map_status: Default::default(),
                    map_prep_running: false,
                    map_audition_drag: None,
                    map_active_audition_file_id: None,
                    map_viewport: crate::native_app::app::StarmapViewport::default(),
                    name_filter: String::new(),
                    display_mode: SampleBrowserDisplayMode::List,
                    name_view_mode: SampleNameViewMode::DiskFilename,
                    random_navigation_enabled: false,
                    curation_mode_enabled: false,
                    metadata_tags_by_file: &metadata_tags_by_file,
                    cut_file_ids: None,
                    file_drag_active: true,
                    extracted_file_drag_active: true,
                    hovered_folder_drop_target: true,
                    drag_feedback: None,
                    help_tooltips_enabled: false,
                })
                .into_surface()
            },
            |messages, message| messages.push(message),
        );
        let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(240.0, 160.0));

        runtime.dispatch_pointer_move_with_outcome(position);
        runtime.dispatch_input_at(position, WidgetInput::primary_release(position));
        runtime.dispatch_input_at(position, WidgetInput::primary_drop(position));

        assert_eq!(
            runtime.bridge().state(),
            &[
                GuiMessage::FolderBrowser(FolderBrowserMessage::ClearDropTarget(position)),
                GuiMessage::CancelBrowserDragOnSampleList,
                GuiMessage::DropWaveformSelectionOnSampleList,
            ]
        );
    }

    #[test]
    fn starmap_mode_paints_search_input_bound_to_name_filter() {
        let metadata_tags_by_file = HashMap::<String, Vec<String>>::new();
        let sort = radiant::application::DetailsSort::new(
            "name",
            radiant::application::SortDirection::Ascending,
        );
        let similarity_controls = SimilarityAspectSettings::default();
        let columns = Vec::new();

        let frame = sample_browser(SampleBrowserViewModel {
            visible_samples: VisibleSampleList {
                total_count: 1,
                window: ui::VirtualListWindow::default(),
                rows: Vec::new(),
                columns,
                sort: &sort,
                similarity_mode_active: false,
                similarity_controls: &similarity_controls,
            },
            map_items: vec![StarmapItem {
                file_id: String::from("/samples/kick.wav"),
                label: String::from("kick"),
                x: 0.5,
                y: 0.5,
                color: ACCENT.with_alpha(220),
                selected: false,
                focused: false,
                selection_flash: false,
                copy_flash: false,
                similarity_anchor: false,
                instant_audition_ready: true,
                preview_audition_ready: false,
                preview_audition_candidate: true,
                missing: false,
            }]
            .into(),
            map_status: Default::default(),
            map_prep_running: false,
            map_audition_drag: None,
            map_active_audition_file_id: None,
            map_viewport: crate::native_app::app::StarmapViewport::default(),
            name_filter: String::from("kick"),
            display_mode: SampleBrowserDisplayMode::Map,
            name_view_mode: SampleNameViewMode::DiskFilename,
            random_navigation_enabled: false,
            curation_mode_enabled: false,
            metadata_tags_by_file: &metadata_tags_by_file,
            cut_file_ids: None,
            file_drag_active: false,
            extracted_file_drag_active: false,
            hovered_folder_drop_target: false,
            drag_feedback: None,
            help_tooltips_enabled: false,
        })
        .view_frame_at_size_with_default_theme(Vector2::new(520.0, 320.0));

        let input = frame
            .paint_plan
            .text_inputs()
            .find(|input| input.widget_id == widget_ids::SAMPLE_BROWSER_MAP_SEARCH_INPUT_ID)
            .expect("map search input should paint");
        assert_eq!(input.state.value, "kick");
    }

    #[test]
    fn starmap_mode_paints_incomplete_similarity_status() {
        let metadata_tags_by_file = HashMap::<String, Vec<String>>::new();
        let sort = radiant::application::DetailsSort::new(
            "name",
            radiant::application::SortDirection::Ascending,
        );
        let similarity_controls = SimilarityAspectSettings::default();

        let frame = sample_browser(SampleBrowserViewModel {
            visible_samples: VisibleSampleList {
                total_count: 2,
                window: ui::VirtualListWindow::default(),
                rows: Vec::new(),
                columns: Vec::new(),
                sort: &sort,
                similarity_mode_active: false,
                similarity_controls: &similarity_controls,
            },
            map_items: vec![StarmapItem {
                file_id: String::from("/samples/kick.wav"),
                label: String::from("kick"),
                x: 0.5,
                y: 0.5,
                color: ACCENT.with_alpha(220),
                selected: false,
                focused: false,
                selection_flash: false,
                copy_flash: false,
                similarity_anchor: false,
                instant_audition_ready: true,
                preview_audition_ready: false,
                preview_audition_candidate: true,
                missing: false,
            }]
            .into(),
            map_status: StarmapStatus {
                listed_count: 2,
                layout_count: 1,
                clustered_count: 0,
                cluster_color_count: 0,
            },
            map_prep_running: true,
            map_audition_drag: None,
            map_active_audition_file_id: None,
            map_viewport: crate::native_app::app::StarmapViewport::default(),
            name_filter: String::new(),
            display_mode: SampleBrowserDisplayMode::Map,
            name_view_mode: SampleNameViewMode::DiskFilename,
            random_navigation_enabled: false,
            curation_mode_enabled: false,
            metadata_tags_by_file: &metadata_tags_by_file,
            cut_file_ids: None,
            file_drag_active: false,
            extracted_file_drag_active: false,
            hovered_folder_drop_target: false,
            drag_feedback: None,
            help_tooltips_enabled: false,
        })
        .view_frame_at_size_with_default_theme(Vector2::new(520.0, 320.0));

        assert!(frame.paint_plan.contains_text("Preparing Starmap 1 / 2"));
    }

    #[test]
    fn starmap_mode_paints_viewport_controls() {
        let metadata_tags_by_file = HashMap::<String, Vec<String>>::new();
        let sort = radiant::application::DetailsSort::new(
            "name",
            radiant::application::SortDirection::Ascending,
        );
        let similarity_controls = SimilarityAspectSettings::default();

        let frame = sample_browser(SampleBrowserViewModel {
            visible_samples: VisibleSampleList {
                total_count: 1,
                window: ui::VirtualListWindow::default(),
                rows: Vec::new(),
                columns: Vec::new(),
                sort: &sort,
                similarity_mode_active: false,
                similarity_controls: &similarity_controls,
            },
            map_items: vec![StarmapItem {
                file_id: String::from("/samples/kick.wav"),
                label: String::from("kick"),
                x: 0.5,
                y: 0.5,
                color: ACCENT.with_alpha(220),
                selected: false,
                focused: false,
                selection_flash: false,
                copy_flash: false,
                similarity_anchor: false,
                instant_audition_ready: true,
                preview_audition_ready: false,
                preview_audition_candidate: true,
                missing: false,
            }]
            .into(),
            map_status: Default::default(),
            map_prep_running: false,
            map_audition_drag: None,
            map_active_audition_file_id: None,
            map_viewport: crate::native_app::app::StarmapViewport::default(),
            name_filter: String::new(),
            display_mode: SampleBrowserDisplayMode::Map,
            name_view_mode: SampleNameViewMode::DiskFilename,
            random_navigation_enabled: false,
            curation_mode_enabled: false,
            metadata_tags_by_file: &metadata_tags_by_file,
            cut_file_ids: None,
            file_drag_active: false,
            extracted_file_drag_active: false,
            hovered_folder_drop_target: false,
            drag_feedback: None,
            help_tooltips_enabled: true,
        })
        .view_frame_at_size_with_default_theme(Vector2::new(520.0, 320.0));

        assert!(
            frame.paint_plan.svgs().count() >= 4,
            "map controls should paint zoom/focus/reset icon buttons"
        );
    }

    #[test]
    fn starmap_mode_paints_enabled_similarity_color_legend() {
        let mut similarity_controls = SimilarityAspectSettings::default();
        similarity_controls.set_aspect_enabled(SimilarityAspect::Pitch, false);

        let frame = starmap_view(
            vec![StarmapItem {
                file_id: String::from("/samples/kick.wav"),
                label: String::from("kick"),
                x: 0.5,
                y: 0.5,
                color: ACCENT.with_alpha(220),
                selected: false,
                focused: false,
                selection_flash: false,
                copy_flash: false,
                similarity_anchor: false,
                instant_audition_ready: true,
                preview_audition_ready: false,
                preview_audition_candidate: true,
                missing: false,
            }],
            crate::native_app::app::StarmapViewport::default(),
            String::new(),
            &similarity_controls,
            Default::default(),
            false,
            false,
            None,
            None,
        )
        .view_frame_at_size_with_default_theme(Vector2::new(520.0, 320.0));

        assert!(frame.paint_plan.contains_text("Spectrum"));
        assert!(frame.paint_plan.contains_text("Timbre"));
        assert!(
            !frame.paint_plan.contains_text("Pitch"),
            "disabled similarity aspects should not be listed in the map legend"
        );
        assert!(frame.paint_plan.primitives.iter().any(|primitive| matches!(
            primitive,
            radiant::runtime::PaintPrimitive::FillRect(fill)
                if fill.color == similarity_aspect_color(SimilarityAspect::Spectrum)
        )));
    }

    #[test]
    fn starmap_mode_uses_cluster_legend_when_cluster_colors_are_active() {
        let similarity_controls = SimilarityAspectSettings::default();

        let frame = starmap_view(
            vec![StarmapItem {
                file_id: String::from("/samples/kick.wav"),
                label: String::from("kick"),
                x: 0.5,
                y: 0.5,
                color: starmap_cluster_palette_color(0),
                selected: false,
                focused: false,
                selection_flash: false,
                copy_flash: false,
                similarity_anchor: false,
                instant_audition_ready: true,
                preview_audition_ready: false,
                preview_audition_candidate: true,
                missing: false,
            }],
            crate::native_app::app::StarmapViewport::default(),
            String::new(),
            &similarity_controls,
            StarmapStatus {
                listed_count: 1,
                layout_count: 1,
                clustered_count: 8,
                cluster_color_count: 1,
            },
            false,
            false,
            None,
            None,
        )
        .view_frame_at_size_with_default_theme(Vector2::new(520.0, 320.0));

        assert!(frame.paint_plan.contains_text("Similarity clusters"));
        assert!(
            !frame.paint_plan.contains_text("Spectrum"),
            "cluster-colored maps should not label points as aspect colors"
        );
        assert!(frame.paint_plan.primitives.iter().any(|primitive| matches!(
            primitive,
            radiant::runtime::PaintPrimitive::FillRect(fill)
                if fill.color
                    == starmap_cluster_palette_color(0)
        )));
        assert_eq!(
            frame
                .paint_plan
                .fill_rects()
                .filter(|fill| fill.color == starmap_cluster_palette_color(0))
                .count(),
            1,
            "legend should reflect distinct cluster colors, not clustered sample count"
        );
    }

    #[test]
    fn starmap_mode_empty_state_matches_curation_context() {
        let metadata_tags_by_file = HashMap::<String, Vec<String>>::new();
        let sort = radiant::application::DetailsSort::new(
            "name",
            radiant::application::SortDirection::Ascending,
        );
        let similarity_controls = SimilarityAspectSettings::default();

        let frame = sample_browser(SampleBrowserViewModel {
            visible_samples: VisibleSampleList {
                total_count: 0,
                window: ui::VirtualListWindow::default(),
                rows: Vec::new(),
                columns: Vec::new(),
                sort: &sort,
                similarity_mode_active: false,
                similarity_controls: &similarity_controls,
            },
            map_items: Vec::<StarmapItem>::new().into(),
            map_status: Default::default(),
            map_prep_running: false,
            map_audition_drag: None,
            map_active_audition_file_id: None,
            map_viewport: crate::native_app::app::StarmapViewport::default(),
            name_filter: String::new(),
            display_mode: SampleBrowserDisplayMode::Map,
            name_view_mode: SampleNameViewMode::DiskFilename,
            random_navigation_enabled: false,
            curation_mode_enabled: true,
            metadata_tags_by_file: &metadata_tags_by_file,
            cut_file_ids: None,
            file_drag_active: false,
            extracted_file_drag_active: false,
            hovered_folder_drop_target: false,
            drag_feedback: None,
            help_tooltips_enabled: false,
        })
        .view_frame_at_size_with_default_theme(Vector2::new(520.0, 320.0));

        assert!(frame.paint_plan.contains_text("No files left to curate"));
        assert!(
            !frame
                .paint_plan
                .contains_text("No audio files in selected folder")
        );
    }

    #[test]
    fn starmap_status_overlay_does_not_block_node_selection() {
        let metadata_tags_by_file = HashMap::<String, Vec<String>>::new();
        let sort = radiant::application::DetailsSort::new(
            "name",
            radiant::application::SortDirection::Ascending,
        );
        let similarity_controls = SimilarityAspectSettings::default();
        let sample_path = String::from("/samples/kick.wav");
        let bridge = DeclarativeOwnedRuntimeBridge::new(
            Vec::<GuiMessage>::new(),
            {
                let sample_path = sample_path.clone();
                move |_| {
                    sample_browser(SampleBrowserViewModel {
                        visible_samples: VisibleSampleList {
                            total_count: 2,
                            window: ui::VirtualListWindow::default(),
                            rows: Vec::new(),
                            columns: Vec::new(),
                            sort: &sort,
                            similarity_mode_active: false,
                            similarity_controls: &similarity_controls,
                        },
                        map_items: vec![StarmapItem {
                            file_id: sample_path.clone(),
                            label: String::from("kick"),
                            x: 0.5,
                            y: 0.5,
                            color: ACCENT.with_alpha(220),
                            selected: false,
                            focused: false,
                            selection_flash: false,
                            copy_flash: false,
                            similarity_anchor: false,
                            instant_audition_ready: true,
                            preview_audition_ready: false,
                            preview_audition_candidate: true,
                            missing: false,
                        }]
                        .into(),
                        map_status: StarmapStatus {
                            listed_count: 2,
                            layout_count: 1,
                            clustered_count: 0,
                            cluster_color_count: 0,
                        },
                        map_prep_running: true,
                        map_audition_drag: None,
                        map_active_audition_file_id: None,
                        map_viewport: crate::native_app::app::StarmapViewport::default(),
                        name_filter: String::new(),
                        display_mode: SampleBrowserDisplayMode::Map,
                        name_view_mode: SampleNameViewMode::DiskFilename,
                        random_navigation_enabled: false,
                        curation_mode_enabled: false,
                        metadata_tags_by_file: &metadata_tags_by_file,
                        cut_file_ids: None,
                        file_drag_active: false,
                        extracted_file_drag_active: false,
                        hovered_folder_drop_target: false,
                        drag_feedback: None,
                        help_tooltips_enabled: false,
                    })
                    .into_surface()
                }
            },
            |messages, message| messages.push(message),
        );
        let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(520.0, 320.0));

        runtime.dispatch_input_at(
            ui::Point::new(260.0, 172.0),
            WidgetInput::primary_press(ui::Point::new(260.0, 172.0)),
        );

        assert_eq!(
            runtime.bridge().state(),
            &[GuiMessage::BeginStarmapAuditionDrag {
                path: Some(sample_path),
                position: ui::Point::new(260.0, 172.0),
                modifiers: PointerModifiers::default(),
            }]
        );
    }

    #[test]
    fn starmap_status_overlay_does_not_block_wheel_zoom() {
        let metadata_tags_by_file = HashMap::<String, Vec<String>>::new();
        let sort = radiant::application::DetailsSort::new(
            "name",
            radiant::application::SortDirection::Ascending,
        );
        let similarity_controls = SimilarityAspectSettings::default();
        let bridge = DeclarativeOwnedRuntimeBridge::new(
            Vec::<GuiMessage>::new(),
            |_| {
                sample_browser(SampleBrowserViewModel {
                    visible_samples: VisibleSampleList {
                        total_count: 2,
                        window: ui::VirtualListWindow::default(),
                        rows: Vec::new(),
                        columns: Vec::new(),
                        sort: &sort,
                        similarity_mode_active: false,
                        similarity_controls: &similarity_controls,
                    },
                    map_items: vec![StarmapItem {
                        file_id: String::from("/samples/kick.wav"),
                        label: String::from("kick"),
                        x: 0.5,
                        y: 0.5,
                        color: ACCENT.with_alpha(220),
                        selected: false,
                        focused: false,
                        selection_flash: false,
                        copy_flash: false,
                        similarity_anchor: false,
                        instant_audition_ready: true,
                        preview_audition_ready: false,
                        preview_audition_candidate: true,
                        missing: false,
                    }]
                    .into(),
                    map_status: StarmapStatus {
                        listed_count: 2,
                        layout_count: 1,
                        clustered_count: 0,
                        cluster_color_count: 0,
                    },
                    map_prep_running: true,
                    map_audition_drag: None,
                    map_active_audition_file_id: None,
                    map_viewport: crate::native_app::app::StarmapViewport::default(),
                    name_filter: String::new(),
                    display_mode: SampleBrowserDisplayMode::Map,
                    name_view_mode: SampleNameViewMode::DiskFilename,
                    random_navigation_enabled: false,
                    curation_mode_enabled: false,
                    metadata_tags_by_file: &metadata_tags_by_file,
                    cut_file_ids: None,
                    file_drag_active: false,
                    extracted_file_drag_active: false,
                    hovered_folder_drop_target: false,
                    drag_feedback: None,
                    help_tooltips_enabled: false,
                })
                .into_surface()
            },
            |messages, message| messages.push(message),
        );
        let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(520.0, 320.0));

        assert!(
            runtime
                .wheel_or_scroll_at(ui::Point::new(260.0, 172.0), ui::Vector2::new(0.0, -120.0),)
        );

        assert_eq!(
            runtime.bridge().state(),
            &[GuiMessage::ChangeStarmapViewport(
                StarmapViewportChange::Zoom {
                    anchor: ui::Vector2::new(0.5, 0.5),
                    factor: 1.15,
                }
            )]
        );
    }

    #[test]
    fn starmap_runtime_drag_auditions_each_crossed_node() {
        let metadata_tags_by_file = HashMap::<String, Vec<String>>::new();
        let sort = radiant::application::DetailsSort::new(
            "name",
            radiant::application::SortDirection::Ascending,
        );
        let similarity_controls = SimilarityAspectSettings::default();
        let left_path = String::from("/samples/kick.wav");
        let right_path = String::from("/samples/snare.wav");
        let bridge = DeclarativeOwnedRuntimeBridge::new(
            Vec::<GuiMessage>::new(),
            {
                let left_path = left_path.clone();
                let right_path = right_path.clone();
                move |messages| {
                    sample_browser(SampleBrowserViewModel {
                        visible_samples: VisibleSampleList {
                            total_count: 2,
                            window: ui::VirtualListWindow::default(),
                            rows: Vec::new(),
                            columns: Vec::new(),
                            sort: &sort,
                            similarity_mode_active: false,
                            similarity_controls: &similarity_controls,
                        },
                        map_items: vec![
                            StarmapItem {
                                file_id: left_path.clone(),
                                label: String::from("kick"),
                                x: 0.30,
                                y: 0.5,
                                color: ACCENT.with_alpha(220),
                                selected: false,
                                focused: false,
                                selection_flash: false,
                                copy_flash: false,
                                similarity_anchor: false,
                                instant_audition_ready: true,
                                preview_audition_ready: false,
                                preview_audition_candidate: true,
                                missing: false,
                            },
                            StarmapItem {
                                file_id: right_path.clone(),
                                label: String::from("snare"),
                                x: 0.70,
                                y: 0.5,
                                color: ui::Rgba8::new(57, 187, 245, 220),
                                selected: false,
                                focused: false,
                                selection_flash: false,
                                copy_flash: false,
                                similarity_anchor: false,
                                instant_audition_ready: true,
                                preview_audition_ready: false,
                                preview_audition_candidate: true,
                                missing: false,
                            },
                        ]
                        .into(),
                        map_status: Default::default(),
                        map_prep_running: false,
                        map_audition_drag: messages
                            .iter()
                            .rev()
                            .find_map(starmap_drag_state_from_message),
                        map_active_audition_file_id: None,
                        map_viewport: crate::native_app::app::StarmapViewport::default(),
                        name_filter: String::new(),
                        display_mode: SampleBrowserDisplayMode::Map,
                        name_view_mode: SampleNameViewMode::DiskFilename,
                        random_navigation_enabled: false,
                        curation_mode_enabled: false,
                        metadata_tags_by_file: &metadata_tags_by_file,
                        cut_file_ids: None,
                        file_drag_active: false,
                        extracted_file_drag_active: false,
                        hovered_folder_drop_target: false,
                        drag_feedback: None,
                        help_tooltips_enabled: false,
                    })
                    .into_surface()
                }
            },
            |messages, message| messages.push(message),
        );
        let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(520.0, 320.0));

        runtime.dispatch_input_at(
            ui::Point::new(156.0, 172.0),
            WidgetInput::primary_press(ui::Point::new(156.0, 172.0)),
        );
        runtime.dispatch_pointer_move_with_outcome(ui::Point::new(364.0, 172.0));

        assert_eq!(
            runtime.bridge().state(),
            &[
                GuiMessage::BeginStarmapAuditionDrag {
                    path: Some(left_path),
                    position: ui::Point::new(156.0, 172.0),
                    modifiers: PointerModifiers::default(),
                },
                GuiMessage::UpdateStarmapAuditionDrag {
                    paths: vec![right_path],
                    position: ui::Point::new(364.0, 172.0),
                    modifiers: PointerModifiers::default(),
                },
            ]
        );
    }

    #[test]
    fn starmap_runtime_drag_overlay_tracks_deferred_pointer_move_before_surface_refresh() {
        let metadata_tags_by_file = HashMap::<String, Vec<String>>::new();
        let sort = radiant::application::DetailsSort::new(
            "name",
            radiant::application::SortDirection::Ascending,
        );
        let similarity_controls = SimilarityAspectSettings::default();
        let left_path = String::from("/samples/kick.wav");
        let right_path = String::from("/samples/snare.wav");
        let right_color = ui::Rgba8::new(57, 187, 245, 220);
        let bridge = DeclarativeOwnedRuntimeBridge::new(
            Vec::<GuiMessage>::new(),
            {
                let left_path = left_path.clone();
                let right_path = right_path.clone();
                move |messages| {
                    sample_browser(SampleBrowserViewModel {
                        visible_samples: VisibleSampleList {
                            total_count: 2,
                            window: ui::VirtualListWindow::default(),
                            rows: Vec::new(),
                            columns: Vec::new(),
                            sort: &sort,
                            similarity_mode_active: false,
                            similarity_controls: &similarity_controls,
                        },
                        map_items: vec![
                            StarmapItem {
                                file_id: left_path.clone(),
                                label: String::from("kick"),
                                x: 0.30,
                                y: 0.5,
                                color: ACCENT.with_alpha(220),
                                selected: false,
                                focused: false,
                                selection_flash: false,
                                copy_flash: false,
                                similarity_anchor: false,
                                instant_audition_ready: true,
                                preview_audition_ready: false,
                                preview_audition_candidate: true,
                                missing: false,
                            },
                            StarmapItem {
                                file_id: right_path.clone(),
                                label: String::from("snare"),
                                x: 0.70,
                                y: 0.5,
                                color: right_color,
                                selected: false,
                                focused: false,
                                selection_flash: false,
                                copy_flash: false,
                                similarity_anchor: false,
                                instant_audition_ready: true,
                                preview_audition_ready: false,
                                preview_audition_candidate: true,
                                missing: false,
                            },
                        ]
                        .into(),
                        map_status: Default::default(),
                        map_prep_running: false,
                        map_audition_drag: messages
                            .iter()
                            .rev()
                            .find_map(starmap_drag_state_from_message),
                        map_active_audition_file_id: None,
                        map_viewport: crate::native_app::app::StarmapViewport::default(),
                        name_filter: String::new(),
                        display_mode: SampleBrowserDisplayMode::Map,
                        name_view_mode: SampleNameViewMode::DiskFilename,
                        random_navigation_enabled: false,
                        curation_mode_enabled: false,
                        metadata_tags_by_file: &metadata_tags_by_file,
                        cut_file_ids: None,
                        file_drag_active: false,
                        extracted_file_drag_active: false,
                        hovered_folder_drop_target: false,
                        drag_feedback: None,
                        help_tooltips_enabled: false,
                    })
                    .into_surface()
                }
            },
            |messages, message| messages.push(message),
        );
        let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(520.0, 320.0));

        runtime.dispatch_input_at(
            ui::Point::new(156.0, 172.0),
            WidgetInput::primary_press(ui::Point::new(156.0, 172.0)),
        );
        runtime.dispatch_pointer_move_deferred_refresh_with_outcome(ui::Point::new(364.0, 172.0));

        let mut overlay = Vec::new();
        runtime.runtime_overlay_paint_into(&ThemeTokens::default(), &mut overlay);
        let expected_right_center_x = 520.0 * 0.70;

        assert!(
            overlay.iter().any(|primitive| matches!(
                primitive,
                radiant::runtime::PaintPrimitive::FillPolygon(fill)
                    if fill.color == right_color.with_alpha(255)
                        && (fill.points.iter().map(|point| point.x).sum::<f32>()
                            / fill.points.len() as f32
                            - expected_right_center_x)
                            .abs()
                            < 0.01
            )),
            "deferred drag motion should paint the newly hit starmap node before app-state refresh"
        );
    }

    fn starmap_drag_state_from_message(
        message: &GuiMessage,
    ) -> Option<crate::native_app::app::StarmapAuditionDragState> {
        match message {
            GuiMessage::BeginStarmapAuditionDrag {
                path,
                position,
                modifiers,
            } => Some(crate::native_app::app::StarmapAuditionDragState {
                last_hit_file_id: path.clone(),
                last_position: *position,
                modifiers: *modifiers,
            }),
            GuiMessage::UpdateStarmapAuditionDrag {
                paths,
                position,
                modifiers,
            } => Some(crate::native_app::app::StarmapAuditionDragState {
                last_hit_file_id: paths.last().cloned(),
                last_position: *position,
                modifiers: *modifiers,
            }),
            GuiMessage::FinishStarmapAuditionDrag => None,
            _ => None,
        }
    }
}
